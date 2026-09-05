use super::*;

fn copy_verified(
    from: &Path,
    to: &Path,
    expected_source_hash: Option<&str>,
) -> std::io::Result<String> {
    if to.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("목적지가 이미 있습니다: {}", to.display()),
        ));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = to.with_file_name(format!(
        ".{}.photo-desk-copy.tmp",
        to.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&temp);
    let got = std::fs::copy(from, &temp)?;
    let want = std::fs::metadata(from)?.len();
    if got != want {
        let _ = std::fs::remove_file(&temp);
        return Err(std::io::Error::other("사본 크기가 원본과 다릅니다"));
    }
    // 발행은 미리보기 직후 실행 계획을 만들며 원본을 이미 해시했다. 그 값을
    // 재사용해 같은 큰 파일을 실행 안에서 또 읽지 않는다. 일반 복사는 여기서
    // 한 번 계산한다.
    let source_hash = match expected_source_hash {
        Some(hash) => hash.to_string(),
        None => crate::cull::hash::full(from)?,
    };
    let dest_hash = crate::cull::hash::full(&temp)?;
    if source_hash != dest_hash {
        let _ = std::fs::remove_file(&temp);
        // 계획 해시와 다르면 «사본이 틀렸다»가 아니라 «원본이 그새 바뀌었다»는 뜻이다
        return Err(std::io::Error::other(if expected_source_hash.is_some() {
            "미리보기 뒤 원본 내용이 바뀌었습니다. 다시 미리보기 하세요"
        } else {
            "사본 SHA-256이 원본과 다릅니다"
        }));
    }
    copy_mtime(from, &temp);
    std::fs::File::open(&temp)?.sync_all()?;
    if to.exists() {
        let _ = std::fs::remove_file(&temp);
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("복사 중 목적지가 생겼습니다: {}", to.display()),
        ));
    }
    std::fs::rename(&temp, to)?;
    if let Some(parent) = to.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(source_hash)
}

#[derive(Debug)]
struct CopiedSidecar {
    target: PathBuf,
    sha256: String,
}

fn copy_with_sidecars(
    from: &Path,
    to: &Path,
    expected_source_hash: Option<&str>,
) -> std::io::Result<(String, Vec<CopiedSidecar>)> {
    let pairs = sidecars(from, to);
    if let Some((_, target)) = pairs.iter().find(|(_, target)| target.exists()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("목적지 사이드카가 이미 있습니다: {}", target.display()),
        ));
    }
    let hash = copy_verified(from, to, expected_source_hash)?;
    let mut copied = Vec::new();
    for (source, target) in pairs {
        match copy_verified(&source, &target, None) {
            Ok(sha256) => copied.push(CopiedSidecar { target, sha256 }),
            Err(error) => {
                for sidecar in &copied {
                    let _ = std::fs::remove_file(&sidecar.target);
                }
                let _ = std::fs::remove_file(to);
                return Err(error);
            }
        }
    }
    Ok((hash, copied))
}

pub(super) fn ensure_folder(
    db: &Db,
    lib: &crate::db::libraries::Library,
    rel: &str,
) -> Result<i64> {
    let vol_rel = crate::media::cache::rel_path(&lib.rel_path, rel);
    let name = rel.rsplit('/').next().unwrap_or(&lib.name);
    // 겹쳐 등록된 다른 라이브러리의 행은 빼앗지 않는다 — organize::ensure_folder 와 같은 규칙
    db.write(|c|c.execute("INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at) VALUES(?1,?2,?3,?4,?5,strftime('%s','now')) ON CONFLICT(volume_uuid,rel_path) DO NOTHING",rusqlite::params![lib.volume_uuid,lib.id,vol_rel,name,lib.area]))?;
    let (id, owner): (i64, i64) = db.read(|c| {
        c.query_row(
            "SELECT id, library_id FROM folders WHERE volume_uuid=?1 AND rel_path=?2",
            rusqlite::params![lib.volume_uuid, vol_rel],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    })?;
    if owner != lib.id {
        return Err(DbError::Invalid(
            "목적지 폴더가 이미 다른 라이브러리에 속해 있습니다".into(),
        ));
    }
    Ok(id)
}

pub(crate) fn clone_row(db: &Db, source: i64, folder: i64, name: &str, hash: &str) -> Result<i64> {
    db.transaction(|tx|{
        tx.execute(
            "INSERT INTO files(folder_id,name,ext,size,kind,taken_at,taken_at_source,created_at,modified_at,
             quick_hash,full_hash,image_hash,phash,psig,width,height,orientation,duration_ms,culling_flag,rating,favorite,comment,
             cam_make,cam_model,lens,iso,aperture,shutter,focal_mm,gps_lat,gps_lon,gps_alt,geo_name,geo_country,geo_admin1,geo_admin2,
             sharpness,exposure,embedding,inode,scanned_at)
             SELECT ?2,?3,ext,size,kind,taken_at,taken_at_source,created_at,modified_at,NULL,?4,image_hash,phash,psig,width,height,orientation,duration_ms,
             culling_flag,rating,favorite,comment,cam_make,cam_model,lens,iso,aperture,shutter,focal_mm,gps_lat,gps_lon,gps_alt,geo_name,geo_country,
             geo_admin1,geo_admin2,sharpness,exposure,embedding,NULL,strftime('%s','now') FROM files WHERE id=?1",
            rusqlite::params![source,folder,name,hash],
        )?;
        let id=tx.last_insert_rowid();
        tx.execute("INSERT OR IGNORE INTO file_tags(file_id,tag_id) SELECT ?2,tag_id FROM file_tags WHERE file_id=?1",rusqlite::params![source,id])?;
        tx.execute("INSERT OR IGNORE INTO capture_date_overrides(file_id,taken_at) SELECT ?2,taken_at FROM capture_date_overrides WHERE file_id=?1",rusqlite::params![source,id])?;
        Ok(id)
    })
}

pub fn execute(db: &Db, request: &Request, label: &str) -> Result<TransferOutcome> {
    let plan = preview(db, request)?;
    let items = load(db, &request.ids)?;
    if items.is_empty() {
        return Ok(TransferOutcome {
            first_error: Some("옮기거나 복사할 사진이 없습니다".into()),
            ..Default::default()
        });
    }
    let planned = plan
        .items
        .iter()
        .map(|item| (item.id, item))
        .collect::<std::collections::HashMap<_, _>>();
    if planned.len() != items.len() || items.iter().any(|item| !planned.contains_key(&item.id)) {
        return Err(DbError::Invalid(
            "실행 계획과 현재 사진 목록이 달라졌습니다. 다시 미리보기 하세요".into(),
        ));
    }
    let (lib, rel, dir) = dest(db, request)?;
    // 목적지 폴더는 실제로 옮길 사진이 있을 때 만든다 — 전부 건너뛰는 실행이 빈 폴더를
    // 디스크에 남기면 안 된다
    let mut folder: Option<i64> = None;
    let kind = if plan.publish {
        "publish"
    } else if request.mode == Mode::Copy {
        "copy"
    } else {
        "move"
    };
    let batch = super::super::open_batch(db, kind, label)?;
    let mut out = TransferOutcome {
        batch_id: batch,
        ..Default::default()
    };
    for it in &items {
        let p = planned
            .get(&it.id)
            .expect("위에서 실행 계획과 사진 ID를 모두 대조했다");
        if p.action == "skip" {
            out.skipped += 1;
            if p.conflict == "already_published" {
                out.already_published += 1;
            }
            continue;
        }
        let result = (|| -> Result<()> {
            let source = mount_path(it)?;
            let target = dir.join(&p.planned_name);
            if source == target {
                return Ok(());
            }
            if target.exists() {
                return Err(DbError::Invalid(format!(
                    "실행 직전 같은 이름이 생겼습니다: {}",
                    p.planned_name
                )));
            }
            let folder = match folder {
                Some(folder) => folder,
                None => {
                    std::fs::create_dir_all(&dir)
                        .io_context("전송 목적지 폴더를 만들다가 실패했습니다")?;
                    let made = ensure_folder(db, &lib, &rel)?;
                    folder = Some(made);
                    made
                }
            };
            let dest_vol_rel = crate::media::cache::rel_path(
                &crate::media::cache::rel_path(&lib.rel_path, &rel),
                &p.planned_name,
            );
            match request.mode {
                Mode::Move => {
                    let occupied: bool = db.read(|c| {
                        c.query_row(
                            "SELECT EXISTS(SELECT 1 FROM files WHERE folder_id=?1 AND name=?2 AND id<>?3)",
                            rusqlite::params![folder,p.planned_name,it.id],
                            |r| r.get(0),
                        )
                    })?;
                    if occupied {
                        return Err(DbError::Invalid(format!(
                            "목적지 DB에 같은 이름 기록이 있습니다: {}",
                            p.planned_name
                        )));
                    }
                    move_with_sidecars(&source, &target)
                        .io_context("파일과 사이드카를 목적지로 옮기다가 실패했습니다")?;
                    let (to_size, to_mtime) = super::super::file_stat(&target);
                    let changed = db.transaction(|tx| {
                        tx.execute(
                            "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok,to_size,to_mtime)
                             VALUES(?1,?2,'move',?3,?4,?5,?6,1,?7,?8)",
                            rusqlite::params![batch,it.id,it.volume_uuid,it.vol_rel,lib.volume_uuid,dest_vol_rel,to_size,to_mtime],
                        )?;
                        tx.execute(
                            "UPDATE files SET folder_id=?2,name=?3 WHERE id=?1",
                            rusqlite::params![it.id, folder, p.planned_name],
                        )?;
                        // 썸네일 캐시는 라이브러리별 루트에 있다. 다른 라이브러리로
                        // 옮긴 뒤 예전 완료 행을 남기면 통계는 완료인데 UI는 빈 그림이
                        // 된다. 행을 지워 다음 썸네일 생성에서 정확히 다시 만들게 한다.
                        if it.library_id != lib.id {
                            tx.execute("DELETE FROM thumbs WHERE file_id=?1", [it.id])?;
                        }
                        Ok(())
                    });
                    if let Err(error) = changed {
                        return match move_with_sidecars(&target, &source) {
                            Ok(()) => Err(error),
                            Err(rollback) => Err(DbError::Invalid(format!(
                                "DB 갱신 실패: {error}; 파일 원위치 복구도 실패: {rollback}"
                            ))),
                        };
                    }
                }
                Mode::Copy => {
                    // 계획 해시를 넘기면 copy_verified 가 사본과 대조해 원본 변경을 잡는다
                    let (full, copied_sidecars) =
                        copy_with_sidecars(&source, &target, p.source_sha256.as_deref())
                            .io_context("파일과 사이드카를 목적지로 복사하다가 실패했습니다")?;
                    let new_id = match clone_row(db, it.id, folder, &p.planned_name, &full) {
                        Ok(id) => id,
                        Err(e) => {
                            let _ = std::fs::remove_file(&target);
                            for sidecar in &copied_sidecars {
                                let _ = std::fs::remove_file(&sidecar.target);
                            }
                            return Err(e);
                        }
                    };
                    let destination_vol_dir = crate::media::cache::rel_path(&lib.rel_path, &rel);
                    let recorded = db.transaction(|tx| {
                        tx.execute(
                            "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok)
                             VALUES(?1,?2,'copy',?3,?4,?5,?6,1)",
                            rusqlite::params![batch,new_id,it.volume_uuid,it.vol_rel,lib.volume_uuid,dest_vol_rel],
                        )?;
                        tx.execute(
                            "INSERT INTO copy_manifest(batch_id,file_id,seq,to_vol,to_path,sha256,is_main)
                             VALUES(?1,?2,0,?3,?4,?5,1)",
                            rusqlite::params![batch,new_id,lib.volume_uuid,dest_vol_rel,full],
                        )?;
                        for (index, sidecar) in copied_sidecars.iter().enumerate() {
                            let name = sidecar
                                .target
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            let path = crate::media::cache::rel_path(&destination_vol_dir, &name);
                            tx.execute(
                                "INSERT INTO copy_manifest(batch_id,file_id,seq,to_vol,to_path,sha256,is_main)
                                 VALUES(?1,?2,?3,?4,?5,?6,0)",
                                rusqlite::params![batch,new_id,index as i64+1,lib.volume_uuid,path,sidecar.sha256],
                            )?;
                        }
                        if plan.publish {
                            let dest_path=if rel.is_empty(){p.planned_name.clone()}else{format!("{rel}/{}",p.planned_name)};
                            tx.execute("INSERT INTO publication_ledger(source_file_id,source_sha256,destination_library_id,destination_path,destination_sha256,batch_id) VALUES(?1,?2,?3,?4,?2,?5) ON CONFLICT(source_sha256,destination_library_id,destination_path) DO UPDATE SET destination_sha256=excluded.destination_sha256,batch_id=excluded.batch_id,created_at=strftime('%s','now')",rusqlite::params![it.id,full,lib.id,dest_path,batch])?;
                        }
                        Ok(())
                    });
                    if let Err(e) = recorded {
                        let _ = db.write(|c| c.execute("DELETE FROM files WHERE id=?1", [new_id]));
                        let _ = std::fs::remove_file(&target);
                        for sidecar in &copied_sidecars {
                            let _ = std::fs::remove_file(&sidecar.target);
                        }
                        return Err(e);
                    }
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                out.completed += 1;
                out.bytes += it.size;
            }
            Err(e) => {
                out.failed += 1;
                out.failed_ids.push(it.id);
                out.first_error.get_or_insert(e.to_string());
                let msg = e.to_string();
                let _ = super::super::record(
                    db,
                    batch,
                    kind,
                    it.id,
                    &it.volume_uuid,
                    &it.vol_rel,
                    None,
                    Err(&msg),
                );
            }
        }
    }
    super::super::close_batch(db, batch, out.completed)?;
    let affected = items
        .iter()
        .map(|i| i.library_id)
        .chain(std::iter::once(lib.id))
        .collect::<std::collections::BTreeSet<_>>();
    for id in &affected {
        if let Err(error) = db.write(|c|c.execute("UPDATE folders SET file_count=(SELECT COUNT(*) FROM files WHERE files.folder_id=folders.id AND files.trashed_at IS NULL) WHERE library_id=?1",[id])) {
            log::warn!("이동·복사 뒤 폴더 장수 갱신 보류: {error}");
        }
    }
    for id in affected {
        if let Err(error) = crate::ops::organize::forget_empty_folders(db, id) {
            log::warn!("이동·복사 뒤 빈 폴더 정리 보류: {error}");
        }
    }
    Ok(out)
}
