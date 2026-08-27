//! 태그 명령 — 실제 일은 `db::tags`가 한다.
//!
//! 여기는 프론트가 부르는 이름과 `State`를 붙이는 얇은 껍데기다. 로직을
//! 아래로 내려 두면 DB만 있으면 시험할 수 있다.

use super::{err, AppState};
use crate::db::tags;
use tauri::State;

pub use crate::db::tags::Tag;

#[tauri::command]
pub fn tags_list(state: State<'_, AppState>) -> Result<Vec<Tag>, String> {
    tags::list(&state.db).map_err(err)
}

#[tauri::command]
pub fn tags_of(state: State<'_, AppState>, id: i64) -> Result<Vec<Tag>, String> {
    tags::of_file(&state.db, id).map_err(err)
}

#[tauri::command]
pub fn tag_add(state: State<'_, AppState>, ids: Vec<i64>, name: String) -> Result<i64, String> {
    tags::add(&state.db, &ids, &name).map_err(err)
}

#[tauri::command]
pub fn tag_remove(state: State<'_, AppState>, ids: Vec<i64>, tag_id: i64) -> Result<(), String> {
    tags::remove(&state.db, &ids, tag_id).map_err(err)
}

#[tauri::command]
pub fn tag_delete(state: State<'_, AppState>, tag_id: i64) -> Result<(), String> {
    tags::delete(&state.db, tag_id).map_err(err)
}
