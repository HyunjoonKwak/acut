use crate::core::quality;
use crate::db::Database;
use rayon::prelude::*;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BcutProgress {
    pub phase: String,
    pub current: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BcutMember {
    pub media_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub thumbnail: Option<String>,
    pub date_taken: Option<String>,
    pub quality_score: f64,
    pub sharpness_score: f64,
    pub exposure_score: f64,
    pub is_best: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BcutGroup {
    pub id: String,
    pub group_reason: String,
    pub status: String,
    pub members: Vec<BcutMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BcutSummary {
    pub total_groups: usize,
    pub total_bcuts: usize,
    pub groups: Vec<BcutGroup>,
}

// Detect B-cut groups by time proximity of images
#[tauri::command]
pub async fn detect_bcuts(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
    time_gap_seconds: Option<i64>,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    let gap = time_gap_seconds.unwrap_or(5);

    app.emit("bcut-progress", BcutProgress {
        phase: "grouping".into(), current: 0, total: 0,
    }).ok();

    // Clear previous results
    db_ref.with_conn(|conn| {
        conn.execute("DELETE FROM bcut_members", [])?;
        conn.execute("DELETE FROM bcut_groups", [])?;
        Ok(())
    }).map_err(|e| format!("DB: {}", e))?;

    // Get all images with date_taken, ordered by date
    let photos: Vec<(String, String, String, i64, Option<i32>, Option<i32>)> = db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT mf.id, mf.file_path, me.date_taken, mf.file_size, mf.width, mf.height
                 FROM media_files mf
                 JOIN media_exif me ON mf.id = me.media_id
                 WHERE mf.media_type = 'image' AND me.date_taken IS NOT NULL
                 ORDER BY me.date_taken ASC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i32>>(4)?,
                        row.get::<_, Option<i32>>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))?;

    if photos.len() < 2 {
        return Ok(());
    }

    // Group by time proximity
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current_group: Vec<usize> = vec![0];

    for i in 1..photos.len() {
        let prev_time = parse_datetime(&photos[i - 1].2);
        let curr_time = parse_datetime(&photos[i].2);

        let within_gap = match (prev_time, curr_time) {
            (Some(p), Some(c)) => (c - p).num_seconds().abs() <= gap,
            _ => false,
        };

        if within_gap {
            current_group.push(i);
        } else {
            if current_group.len() >= 2 {
                groups.push(current_group.clone());
            }
            current_group = vec![i];
        }
    }
    if current_group.len() >= 2 {
        groups.push(current_group);
    }

    let total_groups = groups.len();
    app.emit("bcut-progress", BcutProgress {
        phase: "scoring".into(), current: 0, total: total_groups,
    }).ok();

    // Score each group
    for (gi, group_indices) in groups.iter().enumerate() {
        let group_id = uuid::Uuid::new_v4().to_string();

        // Find max resolution and filesize in group for normalization
        let max_resolution: f64 = group_indices
            .iter()
            .map(|&i| {
                let (w, h) = (photos[i].4.unwrap_or(0), photos[i].5.unwrap_or(0));
                w as f64 * h as f64
            })
            .fold(0.0f64, f64::max);

        let max_filesize: i64 = group_indices
            .iter()
            .map(|&i| photos[i].3)
            .max()
            .unwrap_or(1);

        // Compute quality scores in parallel
        let scored: Vec<(usize, quality::QualityScore)> = group_indices
            .par_iter()
            .map(|&i| {
                let path = Path::new(&photos[i].1);
                let score = quality::compute_quality(
                    path,
                    photos[i].4,
                    photos[i].5,
                    photos[i].3,
                    max_resolution,
                    max_filesize,
                );
                (i, score)
            })
            .collect();

        // Find best
        let best_idx = scored
            .iter()
            .max_by(|a, b| a.1.total.partial_cmp(&b.1.total).unwrap())
            .map(|s| s.0);

        // Insert group and members
        let member_count = scored.len() as i64;
        db_ref
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO bcut_groups (id, group_reason, member_count, status) VALUES (?1, 'time', ?2, 'pending')",
                    params![group_id, member_count],
                )?;
                for (idx, score) in &scored {
                    let is_best = Some(*idx) == best_idx;
                    conn.execute(
                        "INSERT INTO bcut_members (group_id, media_id, quality_score, sharpness_score, exposure_score, is_best)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            group_id,
                            photos[*idx].0,
                            score.total,
                            score.sharpness,
                            score.exposure,
                            is_best as i32,
                        ],
                    )?;
                }
                Ok(())
            })
            .map_err(|e| format!("DB: {}", e))?;

        if (gi + 1) % 5 == 0 || gi + 1 == total_groups {
            app.emit("bcut-progress", BcutProgress {
                phase: "scoring".into(), current: gi + 1, total: total_groups,
            }).ok();
        }
    }

    app.emit("bcut-progress", BcutProgress {
        phase: "done".into(), current: total_groups, total: total_groups,
    }).ok();

    Ok(())
}

#[tauri::command]
pub async fn get_bcut_groups(db: State<'_, Arc<Database>>) -> Result<BcutSummary, String> {
    let db_ref = db.inner().clone();

    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, group_reason, status FROM bcut_groups WHERE status = 'pending' ORDER BY ROWID",
            )?;
            let groups: Vec<(String, String, String)> = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut result_groups = Vec::new();
            let mut total_bcuts = 0usize;

            for (gid, reason, status) in &groups {
                let mut mstmt = conn.prepare(
                    "SELECT bm.media_id, mf.file_path, mf.file_name, mf.file_size,
                            mf.width, mf.height, mf.thumbnail, me.date_taken,
                            bm.quality_score, bm.sharpness_score, bm.exposure_score, bm.is_best
                     FROM bcut_members bm
                     JOIN media_files mf ON bm.media_id = mf.id
                     LEFT JOIN media_exif me ON bm.media_id = me.media_id
                     WHERE bm.group_id = ?1
                     ORDER BY bm.quality_score DESC",
                )?;
                let members: Vec<BcutMember> = mstmt
                    .query_map(params![gid], |row| {
                        Ok(BcutMember {
                            media_id: row.get(0)?,
                            file_path: row.get(1)?,
                            file_name: row.get(2)?,
                            file_size: row.get(3)?,
                            width: row.get(4)?,
                            height: row.get(5)?,
                            thumbnail: row.get(6)?,
                            date_taken: row.get(7)?,
                            quality_score: row.get(8)?,
                            sharpness_score: row.get(9)?,
                            exposure_score: row.get(10)?,
                            is_best: row.get::<_, i32>(11)? != 0,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                total_bcuts += members.iter().filter(|m| !m.is_best).count();

                result_groups.push(BcutGroup {
                    id: gid.clone(),
                    group_reason: reason.clone(),
                    status: status.clone(),
                    members,
                });
            }

            Ok(BcutSummary {
                total_groups: result_groups.len(),
                total_bcuts,
                groups: result_groups,
            })
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn set_bcut_best(
    db: State<'_, Arc<Database>>,
    group_id: String,
    media_id: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "UPDATE bcut_members SET is_best = 0 WHERE group_id = ?1",
                params![group_id],
            )?;
            conn.execute(
                "UPDATE bcut_members SET is_best = 1 WHERE group_id = ?1 AND media_id = ?2",
                params![group_id, media_id],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn dismiss_bcut_group(
    db: State<'_, Arc<Database>>,
    group_id: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "UPDATE bcut_groups SET status = 'dismissed' WHERE id = ?1",
                params![group_id],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

// Dry-run preview: list B-cut files that would be trashed
#[tauri::command]
pub async fn preview_trash_bcuts(
    db: State<'_, Arc<Database>>,
) -> Result<Vec<super::duplicate::DryRunItem>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT bm.media_id, mf.file_path, mf.file_name, mf.file_size
                 FROM bcut_members bm
                 JOIN bcut_groups bg ON bm.group_id = bg.id
                 JOIN media_files mf ON bm.media_id = mf.id
                 WHERE bg.status = 'pending' AND bm.is_best = 0",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(super::duplicate::DryRunItem {
                        media_id: row.get(0)?,
                        file_path: row.get(1)?,
                        file_name: row.get(2)?,
                        file_size: row.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn trash_bcut_files(
    db: State<'_, Arc<Database>>,
) -> Result<crate::core::trash::TrashResult, String> {
    let db_ref = db.inner().clone();

    let files_to_trash: Vec<(String, String)> = db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT bm.media_id, mf.file_path
                 FROM bcut_members bm
                 JOIN bcut_groups bg ON bm.group_id = bg.id
                 JOIN media_files mf ON bm.media_id = mf.id
                 WHERE bg.status = 'pending' AND bm.is_best = 0",
            )?;
            let rows = stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))?;

    crate::core::trash::trash_and_cleanup_db(&db_ref, &files_to_trash)
}

// Quick quality score for a batch of media IDs (for photo reviewer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickScore {
    pub media_id: String,
    pub sharpness: f64,
    pub exposure: f64,
    pub total: f64,
}

#[tauri::command]
pub async fn compute_quality_scores(
    db: State<'_, Arc<Database>>,
    media_ids: Vec<String>,
) -> Result<Vec<QuickScore>, String> {
    let db_ref = db.inner().clone();

    let files: Vec<(String, String, Option<i32>, Option<i32>, i64)> = db_ref
        .with_conn(|conn| {
            let placeholders: Vec<String> = media_ids.iter().enumerate()
                .map(|(i, _)| format!("?{}", i + 1)).collect();
            let query = format!(
                "SELECT id, file_path, width, height, file_size FROM media_files WHERE id IN ({})",
                placeholders.join(",")
            );
            let mut stmt = conn.prepare(&query)?;
            let params: Vec<&dyn rusqlite::ToSql> = media_ids.iter()
                .map(|s| s as &dyn rusqlite::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))?;

    let scores: Vec<QuickScore> = files
        .par_iter()
        .filter_map(|(id, path, _w, _h, _size)| {
            let p = Path::new(path);
            if !p.exists() { return None; }
            let sharpness = quality::compute_sharpness(p).unwrap_or(0.0);
            let sharpness_norm = (sharpness / 20.0).min(100.0);
            let exposure = quality::compute_exposure(p).unwrap_or(50.0);
            let total = sharpness_norm * 0.5 + exposure * 0.5;
            Some(QuickScore {
                media_id: id.clone(),
                sharpness: sharpness_norm,
                exposure,
                total,
            })
        })
        .collect();

    Ok(scores)
}

fn parse_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S"))
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()
}
