use std::sync::Arc;

use tauri::{Manager, State};
use tokio::sync::RwLock;

use crate::core::config::{AppConfig, PartialConfig};

#[tauri::command]
pub async fn get_config(
    config: State<'_, Arc<RwLock<AppConfig>>>,
) -> Result<AppConfig, String> {
    let cfg = config.read().await;
    Ok(cfg.clone())
}

#[tauri::command]
pub async fn update_config(
    app: tauri::AppHandle,
    config: State<'_, Arc<RwLock<AppConfig>>>,
    partial: PartialConfig,
) -> Result<AppConfig, String> {
    let mut cfg = config.write().await;
    cfg.merge_update(partial);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Path error: {}", e))?;
    cfg.save(&app_data_dir.join("config.yaml"))?;

    Ok(cfg.clone())
}

#[tauri::command]
pub async fn reset_config(
    app: tauri::AppHandle,
    config: State<'_, Arc<RwLock<AppConfig>>>,
) -> Result<AppConfig, String> {
    let mut cfg = config.write().await;
    *cfg = AppConfig::default();

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Path error: {}", e))?;
    cfg.save(&app_data_dir.join("config.yaml"))?;

    Ok(cfg.clone())
}
