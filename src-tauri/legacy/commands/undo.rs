use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::core::{organizer, undo as undo_core};
use crate::db::queries::UndoBatch;
use crate::db::Database;

#[derive(Debug, Serialize, Deserialize)]
pub struct UndoResult {
    pub success: usize,
    pub failed: usize,
}

#[tauri::command]
pub async fn get_undo_history(
    db: State<'_, Arc<Database>>,
    limit: Option<i64>,
) -> Result<Vec<UndoBatch>, String> {
    let limit = limit.unwrap_or(50);
    let db_ref = db.inner().clone();

    db_ref
        .with_conn(|conn| undo_core::get_batches(conn, limit))
        .map_err(|e| format!("DB error: {}", e))
}

#[tauri::command]
pub async fn undo_batch(
    db: State<'_, Arc<Database>>,
    batch_id: String,
) -> Result<UndoResult, String> {
    let db_ref = db.inner().clone();

    let entries = db_ref
        .with_conn(|conn| undo_core::get_batch_entries(conn, &batch_id))
        .map_err(|e| format!("DB error: {}", e))?;

    if entries.is_empty() {
        return Err("No entries found for this batch".to_string());
    }

    let results = organizer::undo_moves(&entries);

    let success = results.iter().filter(|(_, r)| r.is_ok()).count();
    let failed = results.iter().filter(|(_, r)| r.is_err()).count();

    // Revert DB paths
    for entry in &entries {
        if entry.operation == "move" {
            if let (Some(source), Some(target)) = (&entry.source_path, &entry.target_path) {
                db_ref
                    .with_conn(|conn| {
                        conn.execute(
                            "UPDATE media_files SET file_path = ?1 WHERE file_path = ?2",
                            rusqlite::params![source, target],
                        )?;
                        Ok(())
                    })
                    .ok();
            }
        } else if entry.operation == "move_dir" {
            // Folder move/rename: revert every path under the old target prefix
            if let (Some(source), Some(target)) = (&entry.source_path, &entry.target_path) {
                let old_prefix = format!("{}/", target.trim_end_matches('/'));
                let new_prefix = format!("{}/", source.trim_end_matches('/'));
                db_ref
                    .with_conn(|conn| {
                        conn.execute(
                            "UPDATE media_files
                             SET file_path = ?2 || SUBSTR(file_path, LENGTH(?1) + 1)
                             WHERE file_path LIKE ?1 || '%'",
                            rusqlite::params![old_prefix, new_prefix],
                        )?;
                        conn.execute(
                            "UPDATE source_folders
                             SET path = ?2 || SUBSTR(path, LENGTH(?1) + 1)
                             WHERE path LIKE ?1 || '%'",
                            rusqlite::params![old_prefix, new_prefix],
                        )?;
                        conn.execute(
                            "UPDATE source_folders SET path = ?1 WHERE path = ?2",
                            rusqlite::params![source, target],
                        )?;
                        Ok(())
                    })
                    .ok();
            }
        }
    }

    // Mark batch as undone
    db_ref
        .with_conn(|conn| undo_core::mark_batch_undone(conn, &batch_id))
        .map_err(|e| format!("DB error: {}", e))?;

    Ok(UndoResult { success, failed })
}
