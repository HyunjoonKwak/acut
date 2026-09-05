use super::*;

#[tauri::command]
pub async fn settings_get(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<String>, String> {
    crate::db::settings::get(&state.db, &key).map_err(err)
}

#[tauri::command]
pub async fn settings_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    crate::db::settings::set(&state.db, &key, &value).map_err(err)
}

#[tauri::command]
pub async fn settings_remove(state: State<'_, AppState>, key: String) -> Result<(), String> {
    crate::db::settings::remove(&state.db, &key).map_err(err)
}
