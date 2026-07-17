use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::queries::{self, MediaFile};
use crate::db::Database;

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaListResponse {
    pub files: Vec<MediaFile>,
    pub total: i64,
    pub total_size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaStats {
    pub total_count: i64,
    pub total_size: i64,
    pub image_count: i64,
    pub video_count: i64,
}

#[tauri::command]
pub async fn get_media_list(
    db: State<'_, Arc<Database>>,
    folder_path: Option<String>,
    media_type: Option<String>,
    search_query: Option<String>,
    tag_id: Option<String>,
    album_id: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Result<MediaListResponse, String> {
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(500);
    let folder = folder_path.as_deref();
    let mtype = media_type.as_deref();
    let sq = search_query.as_deref();
    let tag = tag_id.as_deref();
    let album = album_id.as_deref();

    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let files =
                queries::get_media_files(conn, folder, mtype, sq, tag, album, offset, limit)?;
            // Count under the same filters, otherwise pagination never ends
            // on a filtered result set
            let total = queries::count_media_files(conn, folder, mtype, sq, tag, album)?;
            let (_, total_size) = queries::get_media_stats(conn)?;
            Ok(MediaListResponse {
                files,
                total,
                total_size,
            })
        })
        .map_err(|e| format!("DB error: {}", e))
}

#[tauri::command]
pub async fn get_media_stats(db: State<'_, Arc<Database>>) -> Result<MediaStats, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let (total_count, total_size) = queries::get_media_stats(conn)?;

            let image_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM media_files WHERE media_type = 'image'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let video_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM media_files WHERE media_type = 'video'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            Ok(MediaStats {
                total_count,
                total_size,
                image_count,
                video_count,
            })
        })
        .map_err(|e| format!("DB error: {}", e))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GroupInfo {
    pub label: String,
    pub count: i64,
}

#[tauri::command]
pub async fn get_date_groups(db: State<'_, Arc<Database>>) -> Result<Vec<GroupInfo>, String> {
    let db_ref = db.inner().clone();
    db_ref.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT COALESCE(
                REPLACE(SUBSTR(me.date_taken, 1, 10), ':', '-'),
                SUBSTR(mf.modified_at, 1, 10)
             ) as dt, COUNT(*) as cnt
             FROM media_files mf
             LEFT JOIN media_exif me ON mf.id = me.media_id
             GROUP BY dt ORDER BY dt DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GroupInfo { label: row.get(0)?, count: row.get(1)? })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }).map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn get_folder_groups(db: State<'_, Arc<Database>>) -> Result<Vec<GroupInfo>, String> {
    let db_ref = db.inner().clone();
    db_ref.with_conn(|conn| {
        // Extract parent folder from file_path
        let mut stmt = conn.prepare(
            "SELECT SUBSTR(file_path, 1, LENGTH(file_path) - LENGTH(file_name) - 1) as folder,
                    COUNT(*) as cnt
             FROM media_files
             GROUP BY folder ORDER BY folder ASC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GroupInfo { label: row.get(0)?, count: row.get(1)? })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }).map_err(|e| format!("DB: {}", e))
}

// Read image file and return as base64 for preview (resized to max 1024px)
#[tauri::command]
pub async fn get_preview_image(file_path: String) -> Result<String, String> {
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err("File not found".into());
    }

    // Use sips to create a preview-sized image
    let unique = uuid::Uuid::new_v4().to_string();
    let tmp = std::env::temp_dir().join(format!("sc_preview_{}.jpg", unique));

    let result = std::process::Command::new("sips")
        .args(["--resampleHeightWidthMax", "1024", "-s", "format", "jpeg",
               "-s", "formatOptions", "85", &file_path, "--out", tmp.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("sips error: {}", e))?;

    if result.success() {
        let bytes = std::fs::read(&tmp).map_err(|e| format!("Read error: {}", e))?;
        let _ = std::fs::remove_file(&tmp);
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
    } else {
        let _ = std::fs::remove_file(&tmp);
        Err("Failed to generate preview".into())
    }
}

// Get first frame of video as base64
#[tauri::command]
pub async fn get_preview_video_frame(file_path: String) -> Result<String, String> {
    let unique = uuid::Uuid::new_v4().to_string();
    let tmp = std::env::temp_dir().join(format!("sc_vpreview_{}.jpg", unique));

    let result = std::process::Command::new(crate::commands::scan::find_ffmpeg())
        .args(["-ss", "1", "-i", &file_path, "-vframes", "1",
               "-vf", "scale=1024:-1", "-q:v", "3", "-y", tmp.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("ffmpeg error: {}", e))?;

    if result.success() {
        let bytes = std::fs::read(&tmp).map_err(|e| format!("Read error: {}", e))?;
        let _ = std::fs::remove_file(&tmp);
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
    } else {
        let _ = std::fs::remove_file(&tmp);
        Err("Failed to generate video preview".into())
    }
}

#[tauri::command]
pub async fn get_thumbnail(
    db: State<'_, Arc<Database>>,
    media_id: String,
) -> Result<Option<String>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let result: Option<String> = conn
                .query_row(
                    "SELECT thumbnail FROM media_files WHERE id = ?1",
                    [&media_id],
                    |row| row.get(0),
                )
                .ok()
                .flatten();
            Ok(result)
        })
        .map_err(|e| format!("DB error: {}", e))
}

// Search media by filename, path, or camera model
#[tauri::command]
pub async fn search_media(
    db: State<'_, Arc<Database>>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<MediaFile>, String> {
    let db_ref = db.inner().clone();
    let limit = limit.unwrap_or(100);
    let pattern = format!("%{}%", query);

    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT mf.id, mf.file_path, mf.original_path, mf.file_name, mf.file_size, mf.mime_type,
                        mf.sha256_hash, mf.quick_hash, mf.width, mf.height, mf.media_type,
                        mf.created_at, mf.modified_at, mf.scanned_at, mf.source_type, mf.thumbnail, mf.scan_phase,
                        me.date_taken
                 FROM media_files mf
                 LEFT JOIN media_exif me ON mf.id = me.media_id
                 WHERE mf.file_name LIKE ?1 OR mf.file_path LIKE ?1
                    OR me.camera_model LIKE ?1 OR me.camera_make LIKE ?1
                    OR mf.comment LIKE ?1
                 ORDER BY COALESCE(me.date_taken, mf.modified_at) DESC
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![pattern, limit], |row| {
                    Ok(MediaFile {
                        id: row.get(0)?, file_path: row.get(1)?, original_path: row.get(2)?,
                        file_name: row.get(3)?, file_size: row.get(4)?, mime_type: row.get(5)?,
                        sha256_hash: row.get(6)?, quick_hash: row.get(7)?,
                        width: row.get(8)?, height: row.get(9)?, media_type: row.get(10)?,
                        created_at: row.get(11)?, modified_at: row.get(12)?,
                        scanned_at: row.get(13)?, source_type: row.get(14)?,
                        thumbnail: row.get(15)?, scan_phase: row.get(16)?, date_taken: row.get(17)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}

// Get media with GPS coordinates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsMediaItem {
    pub media_id: String,
    pub file_name: String,
    pub thumbnail: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub date_taken: Option<String>,
}

#[tauri::command]
pub async fn get_gps_media(
    db: State<'_, Arc<Database>>,
    limit: Option<i64>,
) -> Result<Vec<GpsMediaItem>, String> {
    let db_ref = db.inner().clone();
    let limit = limit.unwrap_or(1000);

    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT mf.id, mf.file_name, mf.thumbnail, me.gps_latitude, me.gps_longitude, me.date_taken
                 FROM media_files mf
                 JOIN media_exif me ON mf.id = me.media_id
                 WHERE me.gps_latitude IS NOT NULL AND me.gps_longitude IS NOT NULL
                 ORDER BY me.date_taken DESC
                 LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit], |row| {
                    Ok(GpsMediaItem {
                        media_id: row.get(0)?, file_name: row.get(1)?, thumbnail: row.get(2)?,
                        latitude: row.get(3)?, longitude: row.get(4)?, date_taken: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}

// ---------------------------------------------------------------------------
// Comments (A-Cut inspector)
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize)]
pub struct MediaComment {
    pub media_id: String,
    pub comment: String,
}

/// Set, update or clear (empty string) the user comment on a media file.
#[tauri::command]
pub async fn set_media_comment(
    db: State<'_, Arc<Database>>,
    media_id: String,
    comment: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    let trimmed = comment.trim().to_string();

    db_ref
        .with_conn(|conn| {
            if trimmed.is_empty() {
                conn.execute(
                    "UPDATE media_files SET comment = NULL WHERE id = ?1",
                    params![media_id],
                )?;
            } else {
                conn.execute(
                    "UPDATE media_files SET comment = ?1 WHERE id = ?2",
                    params![trimmed, media_id],
                )?;
            }
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

/// All media that carry a comment — used for grid badges and the inspector.
#[tauri::command]
pub async fn get_media_comments(
    db: State<'_, Arc<Database>>,
) -> Result<Vec<MediaComment>, String> {
    let db_ref = db.inner().clone();

    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, comment FROM media_files
                 WHERE comment IS NOT NULL AND comment != ''",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(MediaComment {
                        media_id: row.get(0)?,
                        comment: row.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}
