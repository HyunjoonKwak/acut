//! 스마트 앨범 명령 — 실제 일은 `db::smart`가 한다.

use super::{err, AppState};
use crate::db::smart;
use tauri::State;

pub use crate::db::smart::SmartAlbum;

#[tauri::command]
pub async fn smart_list(state: State<'_, AppState>) -> Result<Vec<SmartAlbum>, String> {
    smart::list(&state.db).map_err(err)
}

#[tauri::command]
pub async fn smart_save(
    state: State<'_, AppState>,
    name: String,
    filter: serde_json::Value,
    sort: Option<serde_json::Value>,
) -> Result<i64, String> {
    smart::save(&state.db, &name, &filter, sort.as_ref()).map_err(err)
}

#[tauri::command]
pub async fn smart_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    smart::delete(&state.db, id).map_err(err)
}
