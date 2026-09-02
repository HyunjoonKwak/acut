//! 일반 폴더 작업의 미리보기와 실행.

use super::{err, AppState};
use crate::ops::folder::{self, FolderOutcome, Preview, Request};
use tauri::State;

#[tauri::command]
pub async fn folder_operation_preview(
    state: State<'_, AppState>,
    request: Request,
) -> Result<Preview, String> {
    folder::preview(&state.db, &request).map_err(err)
}

#[tauri::command]
pub async fn folder_operation_execute(
    state: State<'_, AppState>,
    request: Request,
    label: String,
) -> Result<FolderOutcome, String> {
    let Some(_guard) = super::job::try_start_wait(
        &state.running,
        "폴더 작업",
        std::time::Duration::from_secs(20),
    ) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    folder::execute(&state.db, &request, &label).map_err(err)
}
