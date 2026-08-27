use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub cover_media_id: Option<String>,
    pub auto_generated: bool,
    pub created_at: String,
    pub media_count: i64,
}

#[tauri::command]
pub async fn get_albums(db: State<'_, Arc<Database>>) -> Result<Vec<Album>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT a.id, a.name, a.description, a.cover_media_id, a.auto_generated, a.created_at,
                        (SELECT COUNT(*) FROM album_media am WHERE am.album_id = a.id) as media_count
                 FROM albums a ORDER BY a.created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Album {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        cover_media_id: row.get(3)?,
                        auto_generated: row.get::<_, i32>(4)? != 0,
                        created_at: row.get(5)?,
                        media_count: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn create_album(
    db: State<'_, Arc<Database>>,
    name: String,
    description: Option<String>,
) -> Result<Album, String> {
    let db_ref = db.inner().clone();
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let album = Album {
        id: id.clone(),
        name: name.clone(),
        description: description.clone(),
        cover_media_id: None,
        auto_generated: false,
        created_at: now.clone(),
        media_count: 0,
    };

    db_ref
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO albums (id, name, description, auto_generated, created_at) VALUES (?1, ?2, ?3, 0, ?4)",
                params![id, name, description, now],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))?;

    Ok(album)
}

#[tauri::command]
pub async fn delete_album(db: State<'_, Arc<Database>>, album_id: String) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute("DELETE FROM album_media WHERE album_id = ?1", params![album_id])?;
            conn.execute("DELETE FROM albums WHERE id = ?1", params![album_id])?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn add_media_to_album(
    db: State<'_, Arc<Database>>,
    album_id: String,
    media_ids: Vec<String>,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for (i, media_id) in media_ids.iter().enumerate() {
                tx.execute(
                    "INSERT OR IGNORE INTO album_media (album_id, media_id, sort_order) VALUES (?1, ?2, ?3)",
                    params![album_id, media_id, i as i32],
                )?;
            }
            // Set cover to first media if not set
            tx.execute(
                "UPDATE albums SET cover_media_id = ?1 WHERE id = ?2 AND cover_media_id IS NULL",
                params![media_ids.first().unwrap_or(&String::new()), album_id],
            )?;
            tx.commit()
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn remove_media_from_album(
    db: State<'_, Arc<Database>>,
    album_id: String,
    media_ids: Vec<String>,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for media_id in &media_ids {
                tx.execute(
                    "DELETE FROM album_media WHERE album_id = ?1 AND media_id = ?2",
                    params![album_id, media_id],
                )?;
            }
            tx.commit()
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn get_album_media(
    db: State<'_, Arc<Database>>,
    album_id: String,
) -> Result<Vec<crate::db::queries::MediaFile>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT mf.id, mf.file_path, mf.original_path, mf.file_name, mf.file_size, mf.mime_type,
                        mf.sha256_hash, mf.quick_hash, mf.width, mf.height, mf.media_type,
                        mf.created_at, mf.modified_at, mf.scanned_at, mf.source_type, mf.thumbnail, mf.scan_phase,
                        me.date_taken
                 FROM album_media am
                 JOIN media_files mf ON am.media_id = mf.id
                 LEFT JOIN media_exif me ON mf.id = me.media_id
                 WHERE am.album_id = ?1
                 ORDER BY am.sort_order ASC",
            )?;
            let rows = stmt
                .query_map(params![album_id], |row| {
                    Ok(crate::db::queries::MediaFile {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        original_path: row.get(2)?,
                        file_name: row.get(3)?,
                        file_size: row.get(4)?,
                        mime_type: row.get(5)?,
                        sha256_hash: row.get(6)?,
                        quick_hash: row.get(7)?,
                        width: row.get(8)?,
                        height: row.get(9)?,
                        media_type: row.get(10)?,
                        created_at: row.get(11)?,
                        modified_at: row.get(12)?,
                        scanned_at: row.get(13)?,
                        source_type: row.get(14)?,
                        thumbnail: row.get(15)?,
                        scan_phase: row.get(16)?,
                        date_taken: row.get(17)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}

/// Albums a given media file belongs to (inspector reverse lookup).
#[tauri::command]
pub async fn get_media_albums(
    db: State<'_, Arc<Database>>,
    media_id: String,
) -> Result<Vec<Album>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT a.id, a.name, a.description, a.cover_media_id, a.auto_generated, a.created_at
                 FROM albums a
                 JOIN album_media am ON a.id = am.album_id
                 WHERE am.media_id = ?1
                 ORDER BY a.name ASC",
            )?;
            let rows = stmt
                .query_map(params![media_id], |row| {
                    Ok(Album {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        cover_media_id: row.get(3)?,
                        auto_generated: row.get(4)?,
                        created_at: row.get(5)?,
                        media_count: 0,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}
