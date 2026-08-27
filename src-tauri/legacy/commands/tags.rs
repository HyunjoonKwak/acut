use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub category: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaTag {
    pub media_id: String,
    pub tag_id: String,
    pub confidence: f64,
    pub source: String,
}

#[tauri::command]
pub async fn get_tags(db: State<'_, Arc<Database>>) -> Result<Vec<Tag>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, category, created_at FROM tags ORDER BY name ASC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Tag {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        category: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn create_tag(
    db: State<'_, Arc<Database>>,
    name: String,
    category: Option<String>,
) -> Result<Tag, String> {
    let db_ref = db.inner().clone();
    let id = uuid::Uuid::new_v4().to_string();
    let cat = category.unwrap_or_else(|| "user".to_string());
    let now = chrono::Utc::now().to_rfc3339();

    let tag = Tag {
        id: id.clone(),
        name: name.clone(),
        category: cat.clone(),
        created_at: now.clone(),
    };

    db_ref
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO tags (id, name, category, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, name, cat, now],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))?;

    Ok(tag)
}

#[tauri::command]
pub async fn delete_tag(db: State<'_, Arc<Database>>, tag_id: String) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute("DELETE FROM media_tags WHERE tag_id = ?1", params![tag_id])?;
            conn.execute("DELETE FROM tags WHERE id = ?1", params![tag_id])?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn tag_media(
    db: State<'_, Arc<Database>>,
    media_id: String,
    tag_id: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO media_tags (media_id, tag_id, confidence, source) VALUES (?1, ?2, 1.0, 'user')",
                params![media_id, tag_id],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn untag_media(
    db: State<'_, Arc<Database>>,
    media_id: String,
    tag_id: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "DELETE FROM media_tags WHERE media_id = ?1 AND tag_id = ?2",
                params![media_id, tag_id],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn get_media_tags(
    db: State<'_, Arc<Database>>,
    media_id: String,
) -> Result<Vec<Tag>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT t.id, t.name, t.category, t.created_at
                 FROM tags t
                 JOIN media_tags mt ON t.id = mt.tag_id
                 WHERE mt.media_id = ?1
                 ORDER BY t.name ASC",
            )?;
            let rows = stmt
                .query_map(params![media_id], |row| {
                    Ok(Tag {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        category: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}
