use super::*;

/// 한 장의 코멘트. 비우면 NULL로.
#[tauri::command]
pub async fn file_comment(state: State<'_, AppState>, id: i64, text: String) -> Result<(), String> {
    let t = crate::scan::nfc(text.trim());
    let v: Option<String> = if t.is_empty() { None } else { Some(t) };
    state
        .db
        .write(|c| {
            c.execute(
                "UPDATE files SET comment = ?2 WHERE id = ?1",
                rusqlite::params![id, v],
            )
        })
        .map_err(err)?;
    Ok(())
}

/// 이름을 바꾼다. 같은 이름이 있으면 거절한다. 새 이름을 돌려준다.
#[tauri::command]
pub async fn file_rename(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> Result<String, String> {
    let db = Arc::clone(&state.db);
    let running = Arc::clone(&state.running);
    tauri::async_runtime::spawn_blocking(move || {
        // 스캔·정리가 같은 파일을 옮기는 사이에 끼어들지 않게 — 다른 파일 작업과 같은 잠금
        let Some(_guard) =
            job::try_start_wait(&running, "이름 바꾸기", std::time::Duration::from_secs(20))
        else {
            return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
        };
        crate::ops::rename::rename(&db, id, &name).map_err(err)
    })
    .await
    .map_err(|error| error.to_string())?
}
