use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFolder {
    pub id: String,
    pub path: String,
    pub name: String,
    pub added_at: String,
    pub last_scanned_at: Option<String>,
}

#[tauri::command]
pub async fn add_source_folder(
    db: State<'_, Arc<Database>>,
    path: String,
    name: String,
) -> Result<SourceFolder, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    let folder = SourceFolder {
        id: id.clone(),
        path: path.clone(),
        name,
        added_at: now.clone(),
        last_scanned_at: Some(now),
    };

    let db_ref = db.inner().clone();
    let f = folder.clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO source_folders (id, path, name, added_at, last_scanned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![f.id, f.path, f.name, f.added_at, f.last_scanned_at],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB error: {}", e))?;

    Ok(folder)
}

#[tauri::command]
pub async fn get_source_folders(
    db: State<'_, Arc<Database>>,
) -> Result<Vec<SourceFolder>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, path, name, added_at, last_scanned_at
                 FROM source_folders ORDER BY added_at ASC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(SourceFolder {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        name: row.get(2)?,
                        added_at: row.get(3)?,
                        last_scanned_at: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB error: {}", e))
}

#[tauri::command]
pub async fn remove_source_folder(
    db: State<'_, Arc<Database>>,
    path: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    let pattern = format!("{}%", path);
    db_ref
        .with_conn(|conn| {
            // Delete scan data for files under this folder
            conn.execute(
                "DELETE FROM media_exif WHERE media_id IN (SELECT id FROM media_files WHERE file_path LIKE ?1)",
                params![pattern],
            )?;
            conn.execute(
                "DELETE FROM duplicate_members WHERE media_id IN (SELECT id FROM media_files WHERE file_path LIKE ?1)",
                params![pattern],
            )?;
            conn.execute("DELETE FROM media_files WHERE file_path LIKE ?1", params![pattern])?;
            // Clean up empty duplicate groups
            conn.execute(
                "DELETE FROM duplicate_groups WHERE id NOT IN (SELECT DISTINCT group_id FROM duplicate_members)",
                [],
            )?;
            // Remove folder from sources
            conn.execute("DELETE FROM source_folders WHERE path = ?1", params![path])?;
            Ok(())
        })
        .map_err(|e| format!("DB error: {}", e))
}

// Clear all library data
#[tauri::command]
pub async fn reset_library(db: State<'_, Arc<Database>>) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute("DELETE FROM media_tags", [])?;
            conn.execute("DELETE FROM media_exif", [])?;
            conn.execute("DELETE FROM duplicate_members", [])?;
            conn.execute("DELETE FROM duplicate_groups", [])?;
            conn.execute("DELETE FROM album_media", [])?;
            conn.execute("DELETE FROM albums", [])?;
            conn.execute("DELETE FROM undo_journal", [])?;
            conn.execute("DELETE FROM media_files", [])?;
            conn.execute("DELETE FROM source_folders", [])?;
            conn.execute("VACUUM", [])?;
            Ok(())
        })
        .map_err(|e| format!("DB error: {}", e))
}

#[tauri::command]
pub async fn update_folder_scan_time(
    db: State<'_, Arc<Database>>,
    path: String,
) -> Result<(), String> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "UPDATE source_folders SET last_scanned_at = ?1 WHERE path = ?2",
                params![now, path],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB error: {}", e))
}
