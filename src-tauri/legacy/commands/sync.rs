use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::RwLock;

use crate::core::config::{AppConfig, SyncPreset};
use crate::core::sync::{self, SyncPlan, SyncResult, SyncTask, SYNC_CANCELLED};
use crate::db::Database;

#[tauri::command]
pub async fn preview_sync(task: SyncTask) -> Result<SyncPlan, String> {
    tokio::task::spawn_blocking(move || sync::plan_sync(&task))
        .await
        .map_err(|e| format!("Task error: {}", e))?
}

#[tauri::command]
pub async fn execute_sync(
    app: AppHandle,
    plan: SyncPlan,
) -> Result<SyncResult, String> {
    SYNC_CANCELLED.store(false, std::sync::atomic::Ordering::SeqCst);

    let app_handle = app.clone();
    Ok(tokio::task::spawn_blocking(move || {
        sync::execute_sync(&plan, |progress| {
            let _ = app_handle.emit("sync-progress", &progress);
        })
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?)
}

#[tauri::command]
pub async fn cancel_sync() -> Result<(), String> {
    SYNC_CANCELLED.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn get_sync_presets(
    config: State<'_, Arc<RwLock<AppConfig>>>,
) -> Result<Vec<SyncPreset>, String> {
    let cfg = config.read().await;
    Ok(cfg.sync_presets.clone())
}

#[tauri::command]
pub async fn save_sync_preset(
    app: AppHandle,
    config: State<'_, Arc<RwLock<AppConfig>>>,
    preset: SyncPreset,
) -> Result<(), String> {
    let mut cfg = config.write().await;
    cfg.sync_presets.retain(|p| p.id != preset.id);
    cfg.sync_presets.push(preset);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Path error: {}", e))?;
    cfg.save(&app_data_dir.join("config.yaml"))?;

    Ok(())
}

#[tauri::command]
pub async fn delete_sync_preset(
    app: AppHandle,
    config: State<'_, Arc<RwLock<AppConfig>>>,
    preset_id: String,
) -> Result<(), String> {
    let mut cfg = config.write().await;
    cfg.sync_presets.retain(|p| p.id != preset_id);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Path error: {}", e))?;
    cfg.save(&app_data_dir.join("config.yaml"))?;

    Ok(())
}

#[tauri::command]
pub async fn get_sync_history(
    db: State<'_, Arc<Database>>,
    limit: Option<i64>,
) -> Result<Vec<SyncHistoryEntry>, String> {
    let db_ref = db.inner().clone();
    let limit = limit.unwrap_or(20);

    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, preset_id, source_dir, target_dir, started_at, finished_at,
                        files_copied, files_updated, files_skipped, bytes_transferred, status, error_message
                 FROM sync_history ORDER BY started_at DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![limit], |row| {
                    Ok(SyncHistoryEntry {
                        id: row.get(0)?,
                        preset_id: row.get(1)?,
                        source_dir: row.get(2)?,
                        target_dir: row.get(3)?,
                        started_at: row.get(4)?,
                        finished_at: row.get(5)?,
                        files_copied: row.get(6)?,
                        files_updated: row.get(7)?,
                        files_skipped: row.get(8)?,
                        bytes_transferred: row.get(9)?,
                        status: row.get(10)?,
                        error_message: row.get(11)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHistoryEntry {
    pub id: String,
    pub preset_id: Option<String>,
    pub source_dir: String,
    pub target_dir: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub files_copied: i64,
    pub files_updated: i64,
    pub files_skipped: i64,
    pub bytes_transferred: i64,
    pub status: String,
    pub error_message: Option<String>,
}
