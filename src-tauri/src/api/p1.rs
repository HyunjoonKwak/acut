//! Gallery→Desk P1 커맨드. 조회는 dry-run이고 적용만 전역 작업 잠금을 잡는다.

use super::{err, AppState};
use crate::ops::p1::{self, EventCandidate, FolderAuditItem, FolderAuditOutcome};
use tauri::State;

#[tauri::command]
pub async fn folder_name_audit(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<Vec<FolderAuditItem>, String> {
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || p1::audit_folder_names(&db, library_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(err)
}

#[tauri::command]
pub async fn folder_name_apply(
    state: State<'_, AppState>,
    library_id: i64,
    source_dirs: Vec<String>,
) -> Result<FolderAuditOutcome, String> {
    let db = std::sync::Arc::clone(&state.db);
    let running = std::sync::Arc::clone(&state.running);
    tauri::async_runtime::spawn_blocking(move || {
        let Some(_guard) = super::job::try_start_wait(
            &running,
            "폴더 이름 감사",
            std::time::Duration::from_secs(20),
        ) else {
            return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
        };
        p1::apply_folder_names(&db, library_id, &source_dirs).map_err(err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn event_candidates(
    state: State<'_, AppState>,
    library_id: i64,
    gap_minutes: u32,
    min_count: usize,
) -> Result<Vec<EventCandidate>, String> {
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        p1::event_candidates(&db, library_id, gap_minutes, min_count)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(err)
}
