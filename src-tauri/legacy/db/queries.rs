use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: String,
    pub file_path: String,
    pub original_path: String,
    pub file_name: String,
    pub file_size: i64,
    pub mime_type: Option<String>,
    pub sha256_hash: Option<String>,
    pub quick_hash: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub media_type: String,
    pub created_at: String,
    pub modified_at: String,
    pub scanned_at: String,
    pub source_type: String,
    pub thumbnail: Option<String>,
    pub scan_phase: i32,
    pub date_taken: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaExif {
    pub media_id: String,
    pub date_taken: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub orientation: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub id: String,
    pub match_type: String,
    pub similarity_score: Option<f64>,
    pub status: String,
    pub members: Vec<DuplicateMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateMember {
    pub media_id: String,
    pub is_preferred: bool,
    pub file_path: String,
    pub file_name: String,
    pub file_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoBatch {
    pub batch_id: String,
    pub batch_name: Option<String>,
    pub entries: Vec<UndoEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub id: String,
    pub sequence: i32,
    pub operation: String,
    pub source_path: Option<String>,
    pub target_path: Option<String>,
    pub status: String,
    pub executed_at: Option<String>,
}

// Phase 0: lightweight insert (file metadata only, no image processing)
pub fn insert_media_phase0(conn: &Connection, file: &MediaFile) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO media_files
         (id, file_path, original_path, file_name, file_size, mime_type,
          media_type, created_at, modified_at, scanned_at, source_type, scan_phase)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0)",
        params![
            file.id, file.file_path, file.original_path, file.file_name,
            file.file_size, file.mime_type, file.media_type,
            file.created_at, file.modified_at, file.scanned_at, file.source_type,
        ],
    )?;
    Ok(())
}

// Phase 1: update with EXIF + dimensions + thumbnail
pub fn update_media_phase1(
    conn: &Connection,
    id: &str,
    width: Option<i32>,
    height: Option<i32>,
    thumbnail: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE media_files SET width = ?1, height = ?2, thumbnail = ?3, scan_phase = 1 WHERE id = ?4",
        params![width, height, thumbnail, id],
    )?;
    Ok(())
}

pub fn insert_media_exif(conn: &Connection, exif: &MediaExif) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO media_exif
         (media_id, date_taken, camera_make, camera_model, gps_latitude, gps_longitude, orientation)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            exif.media_id, exif.date_taken, exif.camera_make, exif.camera_model,
            exif.gps_latitude, exif.gps_longitude, exif.orientation,
        ],
    )?;
    Ok(())
}

pub fn is_file_unchanged(
    conn: &Connection,
    file_path: &str,
    modified_at: &str,
    file_size: i64,
) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_files WHERE file_path = ?1 AND modified_at = ?2 AND file_size = ?3",
        params![file_path, modified_at, file_size],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn get_phase0_files(conn: &Connection, limit: i64) -> Result<Vec<(String, String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, media_type FROM media_files WHERE scan_phase = 0 ORDER BY modified_at DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_media_files(
    conn: &Connection,
    folder_path: Option<&str>,
    media_type: Option<&str>,
    search: Option<&str>,
    tag_id: Option<&str>,
    album_id: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<Vec<MediaFile>, rusqlite::Error> {
    let map_row = |row: &rusqlite::Row| -> Result<MediaFile, rusqlite::Error> {
        Ok(MediaFile {
            id: row.get(0)?, file_path: row.get(1)?, original_path: row.get(2)?,
            file_name: row.get(3)?, file_size: row.get(4)?, mime_type: row.get(5)?,
            sha256_hash: row.get(6)?, quick_hash: row.get(7)?,
            width: row.get(8)?, height: row.get(9)?, media_type: row.get(10)?,
            created_at: row.get(11)?, modified_at: row.get(12)?,
            scanned_at: row.get(13)?, source_type: row.get(14)?,
            thumbnail: row.get(15)?, scan_phase: row.get(16)?,
            date_taken: row.get(17)?,
        })
    };

    let base = "SELECT mf.id, mf.file_path, mf.original_path, mf.file_name, mf.file_size, mf.mime_type,
                mf.sha256_hash, mf.quick_hash, mf.width, mf.height, mf.media_type,
                mf.created_at, mf.modified_at, mf.scanned_at, mf.source_type, mf.thumbnail, mf.scan_phase,
                me.date_taken
         FROM media_files mf LEFT JOIN media_exif me ON mf.id = me.media_id";

    // Build WHERE clauses
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(folder) = folder_path {
        // Match only files inside the folder: "/a/b" must not match "/a/bc/x",
        // and "_" in folder names must not act as a LIKE wildcard
        conditions.push(format!(
            "mf.file_path LIKE ?{} ESCAPE '\\'",
            param_values.len() + 1
        ));
        param_values.push(Box::new(format!(
            "{}/%",
            escape_like(folder.trim_end_matches('/'))
        )));
    }
    if let Some(mtype) = media_type {
        if mtype != "all" {
            conditions.push(format!("mf.media_type = ?{}", param_values.len() + 1));
            param_values.push(Box::new(mtype.to_string()));
        }
    }
    if let Some(query) = search.map(str::trim).filter(|q| !q.is_empty()) {
        // Match the term anywhere in the file name or its folder path
        let idx = param_values.len() + 1;
        conditions.push(format!(
            "(mf.file_name LIKE ?{idx} ESCAPE '\\' OR mf.file_path LIKE ?{idx} ESCAPE '\\')"
        ));
        param_values.push(Box::new(format!("%{}%", escape_like(query))));
    }
    if let Some(tag) = tag_id {
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM media_tags mt WHERE mt.media_id = mf.id AND mt.tag_id = ?{})",
            param_values.len() + 1
        ));
        param_values.push(Box::new(tag.to_string()));
    }
    if let Some(album) = album_id {
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM album_media am WHERE am.media_id = mf.id AND am.album_id = ?{})",
            param_values.len() + 1
        ));
        param_values.push(Box::new(album.to_string()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let query = format!(
        "{}{} ORDER BY COALESCE(me.date_taken, mf.modified_at) DESC LIMIT ?{} OFFSET ?{}",
        base, where_clause,
        param_values.len() + 1,
        param_values.len() + 2,
    );

    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let mut stmt = conn.prepare(&query)?;
    let params_ref: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), map_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn update_media_sha256(conn: &Connection, id: &str, sha256: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE media_files SET sha256_hash = ?1 WHERE id = ?2",
        params![sha256, id],
    )?;
    Ok(())
}

pub fn insert_nas_upload(
    conn: &Connection,
    media_id: &str,
    sha256: Option<&str>,
    source_path: &str,
    remote_path: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO nas_uploads (id, media_id, sha256, source_path, remote_path, uploaded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            uuid::Uuid::new_v4().to_string(),
            media_id,
            sha256,
            source_path,
            remote_path,
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        ],
    )?;
    Ok(())
}

pub fn get_uploaded_sha256s(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT sha256 FROM nas_uploads WHERE sha256 IS NOT NULL")?;
    let rows = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_uploaded_media_ids(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT media_id FROM nas_uploads WHERE media_id IS NOT NULL
         UNION
         SELECT mf.id FROM media_files mf
         JOIN nas_uploads nu ON nu.sha256 = mf.sha256_hash
         WHERE mf.sha256_hash IS NOT NULL",
    )?;
    let rows = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// LIKE patterns treat %, _ and the escape char specially — neutralize them
// so user-provided folder names and search terms match literally
pub(crate) fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

// Count with the same filters as get_media_files so pagination knows the
// real end of a filtered result set
pub fn count_media_files(
    conn: &Connection,
    folder_path: Option<&str>,
    media_type: Option<&str>,
    search: Option<&str>,
    tag_id: Option<&str>,
    album_id: Option<&str>,
) -> Result<i64, rusqlite::Error> {
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(folder) = folder_path {
        conditions.push(format!(
            "file_path LIKE ?{} ESCAPE '\\'",
            param_values.len() + 1
        ));
        param_values.push(Box::new(format!(
            "{}/%",
            escape_like(folder.trim_end_matches('/'))
        )));
    }
    if let Some(mtype) = media_type {
        if mtype != "all" {
            conditions.push(format!("media_type = ?{}", param_values.len() + 1));
            param_values.push(Box::new(mtype.to_string()));
        }
    }
    if let Some(query) = search.map(str::trim).filter(|q| !q.is_empty()) {
        // Must mirror get_media_files so pagination sees the same result set
        let idx = param_values.len() + 1;
        conditions.push(format!(
            "(file_name LIKE ?{idx} ESCAPE '\\' OR file_path LIKE ?{idx} ESCAPE '\\')"
        ));
        param_values.push(Box::new(format!("%{}%", escape_like(query))));
    }
    if let Some(tag) = tag_id {
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM media_tags mt WHERE mt.media_id = mf.id AND mt.tag_id = ?{})",
            param_values.len() + 1
        ));
        param_values.push(Box::new(tag.to_string()));
    }
    if let Some(album) = album_id {
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM album_media am WHERE am.media_id = mf.id AND am.album_id = ?{})",
            param_values.len() + 1
        ));
        param_values.push(Box::new(album.to_string()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let query = format!("SELECT COUNT(*) FROM media_files mf{}", where_clause);
    let params_ref: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    conn.query_row(&query, params_ref.as_slice(), |row| row.get(0))
}

pub fn get_media_stats(conn: &Connection) -> Result<(i64, i64), rusqlite::Error> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM media_files", [], |row| row.get(0))?;
    let size: i64 = conn.query_row("SELECT COALESCE(SUM(file_size), 0) FROM media_files", [], |row| row.get(0))?;
    Ok((count, size))
}

#[cfg(test)]
mod tests {
    use super::escape_like;

    #[test]
    fn escapes_like_wildcards() {
        assert_eq!(escape_like("2024_01"), "2024\\_01");
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a\\b"), "a\\\\b");
        assert_eq!(escape_like("plain"), "plain");
    }
}
