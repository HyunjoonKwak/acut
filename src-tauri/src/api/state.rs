use super::*;

#[derive(Debug, Serialize)]
pub struct LibraryStats {
    pub files: i64,
    pub bytes: i64,
    pub thumbs_done: i64,
    pub thumbs_pending: i64,
}

/// 상태바에 띄울 값들. `library_id`가 없으면 등록된 전부를 합친다.
///
/// **캐시 용량은 여기서 세지 않는다.** 디스크의 파일 12만 개를 훑는 일이라
/// 1초쯤 걸린다. 폴더를 누를 때마다 그걸 하면 앱이 멈춘 것처럼 보인다.
/// 캐시 용량은 [`cache_usage`]로 따로, 가끔만 부른다.
#[tauri::command]
pub async fn library_stats(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<LibraryStats, String> {
    let (files, bytes): (i64, i64) = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(fi.size),0)
                 FROM files fi JOIN folders fo ON fo.id=fi.folder_id
                 WHERE fi.trashed_at IS NULL AND (?1 IS NULL OR fo.library_id = ?1)",
                [library_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .map_err(err)?;
    // thumbs를 따로 센다. LEFT JOIN으로 14만 행을 훑는 것보다 빠르다 —
    // 이쪽은 thumbs 테이블만 보면 된다.
    let done: i64 = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM thumbs t
                 JOIN files fi ON fi.id = t.file_id
                 JOIN folders fo ON fo.id = fi.folder_id
                 WHERE t.state = 1 AND fi.trashed_at IS NULL
                   AND (?1 IS NULL OR fo.library_id = ?1)",
                [library_id],
                |r| r.get(0),
            )
        })
        .map_err(err)?;

    Ok(LibraryStats {
        files,
        bytes,
        thumbs_done: done,
        thumbs_pending: files - done,
    })
}

#[derive(Debug, Serialize)]
pub struct CacheUsage {
    pub bytes: u64,
    pub files: usize,
}

/// 썸네일·미리보기 캐시가 디스크에서 차지하는 용량.
///
/// 폴더를 통째로 훑으므로 느리다. 앱 시작과 썸네일 생성이 끝났을 때만 부른다.
#[tauri::command]
pub async fn cache_usage(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<CacheUsage, String> {
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;
    let (bytes, files) = libs
        .iter()
        .filter(|l| library_id.is_none_or(|id| l.id == id))
        .flat_map(|l| {
            [
                cache::cache_root(&state.cache_base, l.id),
                cache::preview_root(&state.cache_base, l.id),
            ]
        })
        .map(|root| cache::cache_stats(&root))
        .fold((0u64, 0usize), |(b, n), (rb, rn)| (b + rb, n + rn));
    Ok(CacheUsage { bytes, files })
}

/// 썸네일·미리보기를 모두 지운다.
///
/// 사진은 건드리지 않는다. 다음에 볼 때 다시 만들어지므로 되돌릴 것이 없다.
/// 캐시가 망가졌을 때(빈 그림, 옛 방향)의 마지막 수단이다.
#[tauri::command]
pub async fn cache_clear(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<(), String> {
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;
    for l in libs
        .iter()
        .filter(|l| library_id.is_none_or(|id| l.id == id))
    {
        for root in [
            cache::cache_root(&state.cache_base, l.id),
            cache::preview_root(&state.cache_base, l.id),
        ] {
            // 없으면 지울 것도 없다 — NotFound는 성공으로 본다.
            match std::fs::remove_dir_all(&root) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {e}", root.display())),
            }
        }
    }
    // 디스크에서 지웠으니 "만들어 뒀다"는 기록도 함께 지운다. 안 지우면
    // 다음 스캔이 이미 있는 줄 알고 건너뛰어 빈 자리만 남는다.
    clear_thumb_rows(&state.db, library_id).map_err(err)?;
    Ok(())
}

fn clear_thumb_rows(db: &Db, library_id: Option<i64>) -> crate::db::conn::Result<()> {
    db.write(|c| match library_id {
        Some(id) => c.execute(
            "DELETE FROM thumbs WHERE file_id IN (
                SELECT fi.id FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                WHERE fo.library_id = ?1
            )",
            [id],
        ),
        None => c.execute("DELETE FROM thumbs", []),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_one_library_keeps_other_thumbnail_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| c.execute_batch(
            "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
             INSERT INTO libraries(id,volume_uuid,rel_path,name) VALUES (1,'V','a','a'), (2,'V','b','b');
             INSERT INTO folders(id,volume_uuid,library_id,rel_path,name,area) VALUES
                (1,'V',1,'a','a',1), (2,'V',2,'b','b',1);
             INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at) VALUES
                (1,1,'a.jpg',1,0,1,0,0), (2,2,'b.jpg',1,0,1,0,0);
             INSERT INTO thumbs(file_id,src_size,src_mtime) VALUES (1,1,1), (2,1,1);"
        )).unwrap();

        clear_thumb_rows(&db, Some(1)).unwrap();
        let ids: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare("SELECT file_id FROM thumbs ORDER BY file_id")?;
                let ids = st.query_map([], |r| r.get(0))?.collect();
                ids
            })
            .unwrap();
        assert_eq!(ids, vec![2]);
    }
}

/// Finder에서 그 파일을 골라 연다.
///
/// 우리가 못 하는 일(이름 바꾸기·다른 앱으로 열기)은 Finder에 맡기는 게 낫다.
/// `open -R`은 파일을 **고른 상태로** 폴더를 연다.
#[tauri::command]
pub async fn reveal_in_finder(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let (uuid, rel): (String, String) = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT fo.volume_uuid,
                        fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name
                 FROM files fi JOIN folders fo ON fo.id = fi.folder_id WHERE fi.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .map_err(err)?;
    let mount = crate::db::volumes::find_mount(&uuid).ok_or("디스크가 연결되어 있지 않습니다")?;
    let path = mount.join(&rel);
    if !path.exists() {
        return Err(format!("파일이 없습니다: {}", path.display()));
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 파일 하나의 상세 (인스펙터용).
#[tauri::command]
pub async fn file_detail(state: State<'_, AppState>, id: i64) -> Result<serde_json::Value, String> {
    state
        .db
        .read(|c| {
            c.query_row(
                "SELECT fi.name, fo.rel_path, fi.size, fi.taken_at, fi.taken_at_source,
                        fi.width, fi.height, fi.cam_make, fi.cam_model, fi.lens,
                        fi.iso, fi.aperture, fi.shutter, fi.focal_mm,
                        fi.gps_lat, fi.gps_lon, fi.rating, fi.culling_flag, fi.favorite,
                        fi.comment, fi.kind, fi.duration_ms
                 FROM files fi JOIN folders fo ON fo.id=fi.folder_id WHERE fi.id=?1",
                [id],
                |r| {
                    Ok(serde_json::json!({
                        "name": r.get::<_, String>(0)?,
                        "folder": r.get::<_, String>(1)?,
                        "size": r.get::<_, i64>(2)?,
                        "takenAt": r.get::<_, i64>(3)?,
                        "takenAtSource": r.get::<_, i32>(4)?,
                        "width": r.get::<_, Option<i64>>(5)?,
                        "height": r.get::<_, Option<i64>>(6)?,
                        "camMake": r.get::<_, Option<String>>(7)?,
                        "camModel": r.get::<_, Option<String>>(8)?,
                        "lens": r.get::<_, Option<String>>(9)?,
                        "iso": r.get::<_, Option<i64>>(10)?,
                        "aperture": r.get::<_, Option<f64>>(11)?,
                        "shutter": r.get::<_, Option<String>>(12)?,
                        "focalMm": r.get::<_, Option<f64>>(13)?,
                        "gpsLat": r.get::<_, Option<f64>>(14)?,
                        "gpsLon": r.get::<_, Option<f64>>(15)?,
                        "rating": r.get::<_, i32>(16)?,
                        "cullingFlag": r.get::<_, i32>(17)?,
                        "favorite": r.get::<_, i32>(18)? != 0,
                        "comment": r.get::<_, Option<String>>(19)?,
                        "kind": r.get::<_, i32>(20)?,
                        "durationMs": r.get::<_, Option<i64>>(21)?,
                    }))
                },
            )
        })
        .map_err(err)
}
