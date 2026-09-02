//! 촬영일 감사·교정 커맨드. 긴 파일 쓰기는 공용 작업 잠금을 잡고 실행한다.

use super::{err, AppState};
use crate::ops::capture_date::{self, AuditItem, AuditTarget, CaptureOutcome, Change};
use tauri::State;

#[tauri::command]
pub async fn capture_date_audit(
    state: State<'_, AppState>,
    target: AuditTarget,
) -> Result<Vec<AuditItem>, String> {
    capture_date::audit(&state.db, &target).map_err(err)
}

#[tauri::command]
pub async fn capture_date_apply(
    state: State<'_, AppState>,
    changes: Vec<Change>,
    label: String,
) -> Result<CaptureOutcome, String> {
    let Some(_guard) = super::job::try_start_wait(
        &state.running,
        "촬영일 교정",
        std::time::Duration::from_secs(20),
    ) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    capture_date::apply(&state.db, &changes, &label).map_err(err)
}
