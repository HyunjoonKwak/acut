//! 선택 사진의 임의 이동·복사와 공용 발행.

use super::{err, AppState};
use crate::ops::transfer::{self, Preview, Request, TransferOutcome};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn transfer_preview(
    state: State<'_, AppState>,
    request: Request,
) -> Result<Preview, String> {
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || transfer::preview(&db, &request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(err)
}

#[tauri::command]
pub async fn transfer_execute(
    app: AppHandle,
    state: State<'_, AppState>,
    request: Request,
    label: String,
) -> Result<TransferOutcome, String> {
    let destination_library_id = request.destination_library_id;
    let db = std::sync::Arc::clone(&state.db);
    let running = std::sync::Arc::clone(&state.running);
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let Some(_guard) = super::job::try_start_wait(
            &running,
            "사진 이동·복사",
            std::time::Duration::from_secs(20),
        ) else {
            return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
        };
        transfer::execute(&db, &request, &label).map_err(err)
    })
    .await
    .map_err(|error| error.to_string())??;
    if outcome.completed > 0 {
        if let Err(error) = super::start_pending_thumbs(&app, destination_library_id) {
            log::warn!("이동·복사 뒤 썸네일 생성 보류: {error}");
        }
    }
    Ok(outcome)
}
