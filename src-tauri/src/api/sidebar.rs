use super::*;

/// 사이드바 폴더 트리. 라이브러리를 고른 뒤에만 의미가 있다.
///
/// 스캐너는 파일이 든 폴더만 기록하므로 중간 마디는 [`tree::build`]가 만든다.
#[derive(Debug, Serialize)]
pub struct FolderHit {
    /// `folders` 행 — 사진이 바로 아래 없는 중간 폴더는 행이 없어 None
    pub id: Option<i64>,
    pub library_id: i64,
    pub library: String,
    /// 라이브러리 기준 경로
    pub path: String,
    pub volume_uuid: String,
    /// 볼륨 기준 경로 — 견주기·질의의 열쇠
    pub vol_rel: String,
    /// Finder 가 준 절대경로 — 다음 고르기 창의 시작 자리
    pub abs: String,
    /// 이 폴더와 그 아래의 사진 수
    pub file_count: i64,
}

/// Finder 로 고른 절대경로 → 등록된 라이브러리 안의 폴더 행. 밖이면 None.
/// exFAT 은 목록을 NFD 로 주니 NFC 로 맞춰 견준다.
#[tauri::command]
pub async fn folder_by_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<Option<FolderHit>, String> {
    use unicode_normalization::UnicodeNormalization;
    let nfc = |s: &str| s.nfc().collect::<String>();
    let want = nfc(path.trim_end_matches('/'));
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;
    // 가장 깊이 맞는 라이브러리 — 라이브러리가 겹쳐 등록돼 있을 수 있다
    let mut best: Option<(&crate::db::libraries::Library, String)> = None;
    for l in &libs {
        let Some(dir) = &l.dir else { continue };
        let root = nfc(&dir.to_string_lossy())
            .trim_end_matches('/')
            .to_string();
        let sub = if want == root {
            Some(String::new())
        } else {
            want.strip_prefix(&format!("{root}/")).map(str::to_string)
        };
        if let Some(sub) = sub {
            if best
                .as_ref()
                .is_none_or(|(b, _)| b.dir.as_ref().map_or(0, |d| d.as_os_str().len()) < root.len())
            {
                best = Some((l, sub));
            }
        }
    }
    let Some((lib, sub)) = best else {
        return Ok(None);
    };
    let vol_rel = crate::media::cache::rel_path(&lib.rel_path, &sub);
    let esc = crate::db::query::escape_like(&vol_rel);
    let (id, n): (Option<i64>, i64) = state
        .db
        .read(|c| {
            use rusqlite::OptionalExtension;
            let id = c
                .query_row(
                    "SELECT id FROM folders WHERE volume_uuid = ?1 AND rel_path = ?2",
                    rusqlite::params![lib.volume_uuid, vol_rel],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            let n = c.query_row(
                "SELECT COUNT(*) FROM files f JOIN folders fo ON fo.id = f.folder_id
                  WHERE fo.volume_uuid = ?1 AND (fo.rel_path = ?2 OR fo.rel_path LIKE ?3 || '/%' ESCAPE '\\')
                    AND f.trashed_at IS NULL",
                rusqlite::params![lib.volume_uuid, vol_rel, esc],
                |r| r.get::<_, i64>(0),
            )?;
            Ok((id, n))
        })
        .map_err(err)?;
    Ok(Some(FolderHit {
        id,
        library_id: lib.id,
        library: lib.name.clone(),
        path: sub,
        volume_uuid: lib.volume_uuid.clone(),
        vol_rel,
        abs: path,
        file_count: n,
    }))
}

#[tauri::command]
pub async fn folders_list(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<Vec<tree::Node>, String> {
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;

    // 라이브러리를 고르면 그 트리만 준다. 안 고르면 라이브러리마다 머리
    // 마디를 얹어 하나로 잇는다 — 예전에는 여기서 빈 목록을 돌려주는 바람에
    // 「앨범」을 열어도 하위 폴더가 아무것도 안 보였다.
    //
    // 4,476줄이 한꺼번에 쏟아지지 않는 건 접혀 있기 때문이다. 프론트는
    // 펼친 마디의 자식만 그린다.
    let mut out = Vec::new();
    for l in libs
        .iter()
        .filter(|l| library_id.is_none_or(|id| l.id == id))
    {
        let nodes = tree::build(
            leaves_of(state.inner(), l.id, &l.rel_path)?,
            &l.rel_path,
            l.id,
        );
        if library_id.is_some() {
            out.extend(nodes);
        } else {
            out.extend(tree::under_root(nodes, l.id, &l.name, l.file_count));
        }
    }
    Ok(out)
}

/// 한 라이브러리의 "사진이 든 폴더"들. 중간 마디는 트리가 만들어 낸다.
fn leaves_of(
    state: &AppState,
    library_id: i64,
    library_rel: &str,
) -> Result<Vec<tree::Leaf>, String> {
    // rel_path는 **볼륨** 기준이라 라이브러리 루트만큼 앞이 길다. 그대로 쓰면
    // 들여쓰기가 통째로 밀린다. 여기서 잘라 낸다.
    //
    // 자르는 길이는 SQL의 `length()`로 센다. Rust의 `len()`은 **바이트**라
    // 「사진통합작업」 같은 한글 경로에서 세 배로 잘라 낸다.
    let acut_rel = cache::rel_path(library_rel, ".acut");
    let escaped_acut_rel = crate::db::query::escape_like(&acut_rel);
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT id,
                        CASE WHEN ?2 = '' THEN rel_path
                             ELSE substr(rel_path, length(?2) + 2) END,
                        rel_path, file_count
                 FROM folders
                 WHERE library_id = ?1
                   AND (file_count > 0 OR scanned_at = -1)
                   AND rel_path <> ?3
                   AND rel_path NOT LIKE ?4 || '/%' ESCAPE '\\'
                 ORDER BY rel_path",
            )?;
            let it = st.query_map(
                rusqlite::params![library_id, library_rel, acut_rel, escaped_acut_rel],
                |r| {
                    Ok(tree::Leaf {
                        id: r.get(0)?,
                        path: r.get(1)?,
                        rel_path: r.get(2)?,
                        file_count: r.get(3)?,
                    })
                },
            )?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_tree_escapes_like_wildcards_in_library_path() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO libraries(id,volume_uuid,rel_path,name) VALUES(1,'V','lib_100%','lib');
                 INSERT INTO folders(id,volume_uuid,library_id,rel_path,name,area,file_count) VALUES
                    (1,'V',1,'lib_100%/사진','사진',1,1),
                    (2,'V',1,'lib_100%/.acut/숨김','숨김',1,1),
                    (3,'V',1,'libX100Y/.acut/보임','보임',1,1);",
            )
        })
        .unwrap();
        let state = AppState::new(db, dir.path().to_path_buf());

        let rels: Vec<String> = leaves_of(&state, 1, "lib_100%")
            .unwrap()
            .into_iter()
            .map(|leaf| leaf.rel_path)
            .collect();
        assert_eq!(rels, vec!["libX100Y/.acut/보임", "lib_100%/사진"]);
    }
}
