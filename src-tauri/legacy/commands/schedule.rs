use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tokio::sync::RwLock;

use crate::core::config::{AppConfig, ScheduleConfig};
use crate::core::scheduler::SchedulerManager;
use crate::db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRun {
    pub id: String,
    pub schedule_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub result_json: Option<String>,
}

#[tauri::command]
pub async fn get_schedules(
    db: State<'_, Arc<Database>>,
    config: State<'_, Arc<RwLock<AppConfig>>>,
) -> Result<Vec<ScheduleConfig>, String> {
    let db_ref = db.inner().clone();

    let db_schedules: Vec<ScheduleConfig> = db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, cron_expression, task_type, task_params_json, enabled
                 FROM schedules ORDER BY created_at ASC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(ScheduleConfig {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        cron_expression: row.get(2)?,
                        task_type: row.get(3)?,
                        task_params_json: row.get(4)?,
                        enabled: row.get::<_, i32>(5)? != 0,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))?;

    // If DB has schedules, prefer those
    if !db_schedules.is_empty() {
        return Ok(db_schedules);
    }

    // Fallback to config for backward compatibility
    let cfg = config.read().await;
    Ok(cfg.schedules.clone())
}

#[tauri::command]
pub async fn add_schedule(
    app: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    config: State<'_, Arc<RwLock<AppConfig>>>,
    scheduler: State<'_, Arc<SchedulerManager>>,
    schedule: ScheduleConfig,
) -> Result<(), String> {
    let db_ref = db.inner().clone();

    // Add to scheduler with a callback that records runs in the DB
    let db_for_callback = db.inner().clone();
    let task_type = schedule.task_type.clone();
    scheduler.add_schedule(&schedule, move |id| {
        let task = task_type.clone();
        log::info!("Schedule triggered: {} (task: {})", id, task);

        // Record run in DB
        let run_id = uuid::Uuid::new_v4().to_string();
        let _ = db_for_callback.with_conn(|conn| {
            conn.execute(
                "INSERT INTO schedule_runs (id, schedule_id, started_at, status) VALUES (?1, ?2, datetime('now'), 'running')",
                params![run_id, id],
            )?;
            Ok(())
        });

        // Record the run completion and update schedule last_run_at
        let _ = db_for_callback.with_conn(|conn| {
            conn.execute(
                "UPDATE schedule_runs SET finished_at = datetime('now'), status = 'completed' WHERE id = ?1",
                params![run_id],
            )?;
            conn.execute(
                "UPDATE schedules SET last_run_at = datetime('now') WHERE id = ?1",
                params![id],
            )?;
            Ok(())
        });
    }).await?;

    // Persist to DB
    let sched = schedule.clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO schedules (id, name, cron_expression, task_type, task_params_json, enabled, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
                params![
                    sched.id,
                    sched.name,
                    sched.cron_expression,
                    sched.task_type,
                    sched.task_params_json,
                    sched.enabled as i32,
                ],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))?;

    // Also persist to config (dual-write for backward compatibility)
    let mut cfg = config.write().await;
    cfg.schedules.retain(|s| s.id != schedule.id);
    cfg.schedules.push(schedule);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Path error: {}", e))?;
    cfg.save(&app_data_dir.join("config.yaml"))?;

    Ok(())
}

#[tauri::command]
pub async fn remove_schedule(
    app: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    config: State<'_, Arc<RwLock<AppConfig>>>,
    scheduler: State<'_, Arc<SchedulerManager>>,
    schedule_id: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();

    scheduler.remove_schedule(&schedule_id).await?;

    // Delete from DB
    let sid = schedule_id.clone();
    db_ref
        .with_conn(|conn| {
            conn.execute("DELETE FROM schedules WHERE id = ?1", params![sid])?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))?;

    // Also delete from config
    let mut cfg = config.write().await;
    cfg.schedules.retain(|s| s.id != schedule_id);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Path error: {}", e))?;
    cfg.save(&app_data_dir.join("config.yaml"))?;

    Ok(())
}

#[tauri::command]
pub async fn toggle_schedule(
    app: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    config: State<'_, Arc<RwLock<AppConfig>>>,
    scheduler: State<'_, Arc<SchedulerManager>>,
    schedule_id: String,
    enabled: bool,
) -> Result<(), String> {
    let db_ref = db.inner().clone();

    let mut cfg = config.write().await;

    if let Some(sched) = cfg.schedules.iter_mut().find(|s| s.id == schedule_id) {
        sched.enabled = enabled;
        if !enabled {
            scheduler.remove_schedule(&schedule_id).await?;
        } else {
            let sched_clone = sched.clone();
            let db_for_callback = db.inner().clone();
            let task_type = sched_clone.task_type.clone();
            scheduler.add_schedule(&sched_clone, move |id| {
                let task = task_type.clone();
                log::info!("Schedule triggered: {} (task: {})", id, task);

                let run_id = uuid::Uuid::new_v4().to_string();
                let _ = db_for_callback.with_conn(|conn| {
                    conn.execute(
                        "INSERT INTO schedule_runs (id, schedule_id, started_at, status) VALUES (?1, ?2, datetime('now'), 'running')",
                        params![run_id, id],
                    )?;
                    Ok(())
                });

                let _ = db_for_callback.with_conn(|conn| {
                    conn.execute(
                        "UPDATE schedule_runs SET finished_at = datetime('now'), status = 'completed' WHERE id = ?1",
                        params![run_id],
                    )?;
                    conn.execute(
                        "UPDATE schedules SET last_run_at = datetime('now') WHERE id = ?1",
                        params![id],
                    )?;
                    Ok(())
                });
            }).await?;
        }
    }

    // Update DB
    let sid = schedule_id.clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "UPDATE schedules SET enabled = ?1 WHERE id = ?2",
                params![enabled as i32, sid],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))?;

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Path error: {}", e))?;
    cfg.save(&app_data_dir.join("config.yaml"))?;

    Ok(())
}

#[tauri::command]
pub async fn get_schedule_runs(
    db: State<'_, Arc<Database>>,
    schedule_id: Option<String>,
) -> Result<Vec<ScheduleRun>, String> {
    let db_ref = db.inner().clone();

    db_ref
        .with_conn(|conn| {
            if let Some(sid) = schedule_id {
                let mut stmt = conn.prepare(
                    "SELECT id, schedule_id, started_at, finished_at, status, result_json
                     FROM schedule_runs
                     WHERE schedule_id = ?1
                     ORDER BY started_at DESC
                     LIMIT 100",
                )?;
                let rows = stmt
                    .query_map(params![sid], |row| {
                        Ok(ScheduleRun {
                            id: row.get(0)?,
                            schedule_id: row.get(1)?,
                            started_at: row.get(2)?,
                            finished_at: row.get(3)?,
                            status: row.get(4)?,
                            result_json: row.get(5)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, schedule_id, started_at, finished_at, status, result_json
                     FROM schedule_runs
                     ORDER BY started_at DESC
                     LIMIT 100",
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok(ScheduleRun {
                            id: row.get(0)?,
                            schedule_id: row.get(1)?,
                            started_at: row.get(2)?,
                            finished_at: row.get(3)?,
                            status: row.get(4)?,
                            result_json: row.get(5)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        })
        .map_err(|e| format!("DB: {}", e))
}
