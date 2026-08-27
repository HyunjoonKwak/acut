use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use rusqlite::params;
use crate::db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashResult {
    pub success: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// Send files to macOS Finder trash via osascript
pub fn trash_via_finder(file_paths: &[String]) -> TrashResult {
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();

    for file_path in file_paths {
        let result = std::process::Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "tell application \"Finder\" to delete POSIX file \"{}\"",
                file_path
            ))
            .output();

        match result {
            Ok(output) if output.status.success() => {
                success += 1;
            }
            _ => {
                failed += 1;
                let fname = Path::new(file_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file_path.clone());
                errors.push(fname);
            }
        }
    }

    TrashResult { success, failed, errors }
}

/// Trash files and clean up all related DB records (media_files, duplicate_members, bcut_members)
pub fn trash_and_cleanup_db(
    db_ref: &Arc<Database>,
    files_to_trash: &[(String, String)], // (media_id, file_path)
) -> Result<TrashResult, String> {
    let file_paths: Vec<String> = files_to_trash.iter().map(|(_, p)| p.clone()).collect();
    let result = trash_via_finder(&file_paths);

    // Collect IDs of successfully trashed files
    let trashed_ids: Vec<String> = files_to_trash
        .iter()
        .zip(file_paths.iter())
        .enumerate()
        .filter_map(|(i, ((id, _), _))| {
            // We need to check which ones succeeded - they're in order
            if i < result.success {
                Some(id.clone())
            } else {
                None
            }
        })
        .collect();

    if !trashed_ids.is_empty() {
        db_ref
            .with_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                for id in &trashed_ids {
                    tx.execute("DELETE FROM bcut_members WHERE media_id = ?1", params![id])?;
                    tx.execute("DELETE FROM duplicate_members WHERE media_id = ?1", params![id])?;
                    tx.execute("DELETE FROM media_files WHERE id = ?1", params![id])?;
                }
                // Resolve duplicate groups with 0-1 members
                tx.execute(
                    "UPDATE duplicate_groups SET status = 'resolved'
                     WHERE id IN (
                       SELECT dg.id FROM duplicate_groups dg
                       LEFT JOIN duplicate_members dm ON dg.id = dm.group_id
                       WHERE dg.status = 'pending'
                       GROUP BY dg.id
                       HAVING COUNT(dm.media_id) <= 1
                     )",
                    [],
                )?;
                // Resolve bcut groups with 0-1 members
                tx.execute(
                    "UPDATE bcut_groups SET status = 'resolved'
                     WHERE id IN (
                       SELECT bg.id FROM bcut_groups bg
                       LEFT JOIN bcut_members bm ON bg.id = bm.group_id
                       WHERE bg.status = 'pending'
                       GROUP BY bg.id
                       HAVING COUNT(bm.media_id) <= 1
                     )",
                    [],
                )?;
                tx.commit()
            })
            .map_err(|e| format!("DB: {}", e))?;
    }

    Ok(result)
}
