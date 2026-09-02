//! 촬영일 감사·교정 커맨드. 긴 파일 쓰기는 공용 작업 잠금을 잡고 실행한다.

use super::{err, AppState};
use crate::ops::capture_date::{self, AuditItem, AuditTarget, CaptureOutcome, Change};
use tauri::State;

#[tauri::command]
pub async fn capture_date_audit(
    state: State<'_, AppState>,
    target: AuditTarget,
) -> Result<Vec<AuditItem>, String> {
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || capture_date::audit(&db, &target))
        .await
        .map_err(|error| error.to_string())?
        .map_err(err)
}

#[tauri::command]
pub async fn capture_date_apply(
    state: State<'_, AppState>,
    changes: Vec<Change>,
    label: String,
) -> Result<CaptureOutcome, String> {
    let db = std::sync::Arc::clone(&state.db);
    let running = std::sync::Arc::clone(&state.running);
    tauri::async_runtime::spawn_blocking(move || {
        let Some(_guard) =
            super::job::try_start_wait(&running, "촬영일 교정", std::time::Duration::from_secs(20))
        else {
            return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
        };
        capture_date::apply(&db, &changes, &label).map_err(err)
    })
    .await
    .map_err(|error| error.to_string())?
}
