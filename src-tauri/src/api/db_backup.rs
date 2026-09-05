use super::*;

fn backup_dir(state: &AppState) -> PathBuf {
    state.cache_base.join("backups")
}

/// 지금 쓰는 DB 파일 — 어디에 있고 얼마나 큰가.
#[tauri::command]
pub async fn db_info(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let p = state.db.path().to_path_buf();
    let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    // WAL이 아직 안 합쳐진 만큼도 센다 — 켜 둔 동안은 여기에 쌓인다
    let wal = std::fs::metadata(p.with_extension("db-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    Ok(serde_json::json!({ "path": p.to_string_lossy(), "bytes": bytes + wal }))
}

/// DB 사본을 한 벌 만든다. 켜 둔 채로 해도 된다.
#[tauri::command]
pub async fn db_backup(state: State<'_, AppState>) -> Result<crate::db::backup::Backup, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    crate::db::backup::make(&state.db, &backup_dir(&state), now).map_err(err)
}

#[tauri::command]
pub async fn db_backups(
    state: State<'_, AppState>,
) -> Result<Vec<crate::db::backup::Backup>, String> {
    crate::db::backup::list(&backup_dir(&state)).map_err(err)
}

/// 사본으로 되돌린다. 먼저 지금 상태를 한 벌 떠 두고, 되돌린 뒤 프론트가
/// 화면을 다시 읽는다 (설정까지 바뀌므로 통째로 새로고침).
#[tauri::command]
pub async fn db_restore(
    state: State<'_, AppState>,
    path: String,
) -> Result<crate::db::backup::Backup, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let dir = backup_dir(&state);
    // 백업 폴더 안의 파일만 받는다 — 아무 경로나 부어 넣게 두지 않는다
    let p = PathBuf::from(&path);
    if p.parent() != Some(dir.as_path()) {
        return Err("백업 폴더 안의 사본만 되돌릴 수 있습니다".into());
    }
    let r = crate::db::backup::restore(&state.db, &dir, &p, now).map_err(err)?;
    state.forget_dirs();
    Ok(r)
}

/// 백업 폴더를 Finder에서 연다.
#[tauri::command]
pub async fn db_backups_reveal(state: State<'_, AppState>) -> Result<(), String> {
    let dir = backup_dir(&state);
    std::fs::create_dir_all(&dir).map_err(err)?;
    std::process::Command::new("open")
        .arg(&dir)
        .spawn()
        .map_err(err)?;
    Ok(())
}

/// 파일을 기본 앱으로 연다 — 뷰어가 못 트는 영상은 QuickTime이 튼다.
#[tauri::command]
pub async fn open_in_default_app(state: State<'_, AppState>, id: i64) -> Result<(), String> {
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
        .arg(&path)
        .spawn()
        .map_err(err)?;
    Ok(())
}
