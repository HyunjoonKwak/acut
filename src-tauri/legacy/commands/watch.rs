use serde::{Deserialize, Serialize};
use tauri::State;
use std::sync::Arc;

use crate::core::watcher::WatcherManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchStatus {
    pub watched_folders: Vec<String>,
}

#[tauri::command]
pub async fn start_watch(
    watcher: State<'_, Arc<WatcherManager>>,
    app: tauri::AppHandle,
    folder_path: String,
) -> Result<(), String> {
    let app_handle = app.clone();
    let watched_path = folder_path.clone();

    watcher.start_watching(&folder_path, move |events| {
        use tauri::Emitter;
        let _ = app_handle.emit("watch-changes", &events);

        // If there are create/modify events, signal that a rescan is needed
        let has_changes = events.iter().any(|e| e.kind == "create" || e.kind == "modify");
        if has_changes {
            let _ = app_handle.emit("watch-rescan-needed", &watched_path);
        }
    })
}

#[tauri::command]
pub async fn stop_watch(
    watcher: State<'_, Arc<WatcherManager>>,
    folder_path: String,
) -> Result<(), String> {
    watcher.stop_watching(&folder_path)
}

#[tauri::command]
pub async fn get_watch_status(
    watcher: State<'_, Arc<WatcherManager>>,
) -> Result<WatchStatus, String> {
    Ok(WatchStatus {
        watched_folders: watcher.get_watched_folders(),
    })
}
