//! 일반 폴더 작업의 미리보기와 실행.

use super::{err, AppState};
use crate::ops::folder::{self, Action, FolderOutcome, Preview, Request};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn folder_operation_preview(
    state: State<'_, AppState>,
    request: Request,
) -> Result<Preview, String> {
    folder::preview(&state.db, &request).map_err(err)
}

#[tauri::command]
pub async fn folder_operation_execute(
    app: AppHandle,
    state: State<'_, AppState>,
    request: Request,
    label: String,
) -> Result<FolderOutcome, String> {
    let destination_library_id = request
        .destination_library_id
        .unwrap_or(request.source_library_id);
    let needs_thumbnails = matches!(request.action, Action::Copy)
        || (matches!(request.action, Action::Move)
            && destination_library_id != request.source_library_id);
    let Some(guard) = super::job::try_start_wait(
        &state.running,
        "폴더 작업",
        std::time::Duration::from_secs(20),
    ) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let outcome = folder::execute(&state.db, &request, &label).map_err(err)?;
    drop(guard);
    if outcome.completed > 0 && needs_thumbnails {
        if let Err(error) = super::start_pending_thumbs(&app, destination_library_id) {
            log::warn!("폴더 작업 뒤 썸네일 생성 보류: {error}");
        }
    }
    Ok(outcome)
}
