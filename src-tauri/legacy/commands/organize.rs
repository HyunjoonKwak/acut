use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::core::organizer::{self, MoveOperation};
use crate::core::undo;
use crate::db::Database;

// Extract year/month from various date formats
// EXIF: "2024:07:15 14:32:00" or "2024-07-15T14:32:00"
// File: "2024-07-15T14:32:00"
pub fn extract_date_folder(date_str: &str) -> String {
    let cleaned = date_str.trim();
    if cleaned.is_empty() {
        return "미분류".to_string();
    }

    // Split by any separator: ':', '-', 'T', ' '
    let parts: Vec<&str> = cleaned
        .split(|c: char| c == ':' || c == '-' || c == 'T' || c == ' ')
        .collect();

    if parts.len() >= 3 {
        let year = parts[0].trim();
        let month = parts[1].trim();
        let day = parts[2].trim();

        if year.len() == 4 && (year.starts_with("20") || year.starts_with("19")) {
            return format!("{}-{:0>2}-{:0>2}", year, month, day);
        }
    }

    "미분류".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizePlan {
    pub moves: Vec<PlannedMove>,
    pub total_files: usize,
    pub total_size: i64,
    pub new_folders: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedMove {
    pub source: String,
    pub target: String,
    pub file_name: String,
    pub file_size: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizeProgress {
    pub phase: String,
    pub total: usize,
    pub current: usize,
    pub current_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizeResultResponse {
    pub moved: usize,
    pub failed: usize,
    pub batch_id: String,
}

#[tauri::command]
pub async fn preview_organize(
    db: State<'_, Arc<Database>>,
    target_dir: String,
    strategy: String,
) -> Result<OrganizePlan, String> {
    let db_ref = db.inner().clone();

    let files_with_exif = db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT mf.id, mf.file_path, mf.file_name, mf.file_size, mf.media_type,
                        me.date_taken, me.camera_model, mf.modified_at, mf.created_at
                 FROM media_files mf
                 LEFT JOIN media_exif me ON mf.id = me.media_id
                 ORDER BY COALESCE(me.date_taken, mf.modified_at) ASC"
            )?;

            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(rows)
        })
        .map_err(|e| format!("DB error: {}", e))?;

    let mut moves = Vec::new();
    let mut folders: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (_id, file_path, file_name, file_size, _media_type, date_taken, _camera, modified_at, _created_at) in
        &files_with_exif
    {
        // Use EXIF date first, fallback to file modified/created date
        let best_date = date_taken
            .as_ref()
            .map(|d| d.as_str())
            .unwrap_or(modified_at.as_str());

        let subfolder = match strategy.as_str() {
            "date" => {
                extract_date_folder(best_date)
            }
            "type" => {
                let ext = std::path::Path::new(file_name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("기타")
                    .to_uppercase();
                ext
            }
            _ => "미분류".to_string(),
        };

        let target = format!("{}/{}/{}", target_dir, subfolder, file_name);

        if *file_path != target {
            folders.insert(subfolder.clone());
            moves.push(PlannedMove {
                source: file_path.clone(),
                target,
                file_name: file_name.clone(),
                file_size: *file_size,
                reason: format!("Classified by {}", strategy),
            });
        }
    }

    let total_size: i64 = moves.iter().map(|m| m.file_size).sum();

    Ok(OrganizePlan {
        total_files: moves.len(),
        total_size,
        new_folders: folders.len(),
        moves,
    })
}

#[tauri::command]
pub async fn execute_organize(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
    plan: OrganizePlan,
    batch_name: String,
) -> Result<OrganizeResultResponse, String> {
    let batch_id = uuid::Uuid::new_v4().to_string();
    let db_ref = db.inner().clone();

    let total = plan.moves.len();

    // Record undo journal entries and execute moves
    for (i, planned) in plan.moves.iter().enumerate() {
        app.emit("organize-progress", OrganizeProgress {
            phase: "moving".to_string(),
            total,
            current: i + 1,
            current_file: planned.file_name.clone(),
        }).ok();

        // Record in undo journal
        db_ref
            .with_conn(|conn| {
                undo::record_operation(
                    conn,
                    &batch_id,
                    &batch_name,
                    i as i32,
                    "move",
                    Some(&planned.source),
                    Some(&planned.target),
                    None,
                )?;
                Ok(())
            })
            .map_err(|e| format!("DB error: {}", e))?;
    }

    // Execute actual file moves
    let operations: Vec<MoveOperation> = plan
        .moves
        .iter()
        .map(|m| MoveOperation {
            source: PathBuf::from(&m.source),
            target: PathBuf::from(&m.target),
            reason: m.reason.clone(),
        })
        .collect();

    let result = organizer::execute_moves(&operations);

    // Update DB paths for moved files
    for moved in &result.moved {
        let source_str = moved.source.to_string_lossy().to_string();
        let target_str = moved.target.to_string_lossy().to_string();
        db_ref
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE media_files SET file_path = ?1 WHERE file_path = ?2",
                    rusqlite::params![target_str, source_str],
                )?;
                Ok(())
            })
            .ok();
    }

    app.emit("organize-progress", OrganizeProgress {
        phase: "complete".to_string(),
        total,
        current: total,
        current_file: String::new(),
    }).ok();

    Ok(OrganizeResultResponse {
        moved: result.moved.len(),
        failed: result.failed.len(),
        batch_id,
    })
}

#[cfg(test)]
mod tests {
    use super::extract_date_folder;

    #[test]
    fn parses_exif_colon_format() {
        assert_eq!(extract_date_folder("2024:07:15 14:32:00"), "2024-07-15");
    }

    #[test]
    fn parses_iso_format() {
        assert_eq!(extract_date_folder("2024-07-15T14:32:00"), "2024-07-15");
    }

    #[test]
    fn zero_pads_month_and_day() {
        assert_eq!(extract_date_folder("2024:7:5 09:00:00"), "2024-07-05");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(extract_date_folder(""), "미분류");
        assert_eq!(extract_date_folder("not a date"), "미분류");
        assert_eq!(extract_date_folder("0000:00"), "미분류");
    }
}
