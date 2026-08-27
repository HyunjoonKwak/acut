use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter};

use crate::core::volume;

static VOLUME_MONITORING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn get_mounted_volumes(
    include_internal: Option<bool>,
) -> Result<Vec<volume::VolumeInfo>, String> {
    let include_internal = include_internal.unwrap_or(false);
    tokio::task::spawn_blocking(move || volume::detect_volumes(include_internal))
        .await
        .map_err(|e| format!("Task error: {}", e))
}

#[tauri::command]
pub async fn start_volume_monitoring(app: AppHandle) -> Result<(), String> {
    VOLUME_MONITORING.store(true, Ordering::SeqCst);

    // Monitor /Volumes/ for mount/unmount events using a polling approach
    tokio::spawn(async move {
        let mut known: Vec<String> = volume::detect_volumes(false)
            .iter()
            .map(|v| v.mount_point.clone())
            .collect();

        loop {
            if !VOLUME_MONITORING.load(Ordering::SeqCst) {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            if !VOLUME_MONITORING.load(Ordering::SeqCst) {
                break;
            }

            let current: Vec<String> = volume::detect_volumes(false)
                .iter()
                .map(|v| v.mount_point.clone())
                .collect();

            // Detect new mounts
            for mp in &current {
                if !known.contains(mp) {
                    let _ = app.emit("volume-mounted", mp);
                }
            }

            // Detect unmounts
            for mp in &known {
                if !current.contains(mp) {
                    let _ = app.emit("volume-unmounted", mp);
                }
            }

            known = current;
        }

        log::info!("[Volume] monitoring stopped");
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_volume_monitoring() -> Result<(), String> {
    VOLUME_MONITORING.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn eject_volume(mount_point: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || volume::eject_volume(&mount_point))
        .await
        .map_err(|e| format!("Task error: {}", e))?
}
