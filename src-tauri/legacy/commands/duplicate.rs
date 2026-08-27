use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::core::hasher;
use crate::db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateProgress {
    pub phase: String,
    pub total: usize,
    pub current: usize,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateScanResult {
    pub exact_groups: usize,
    pub similar_groups: usize,
    pub total_duplicates: usize,
    pub space_savings: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupGroup {
    pub id: String,
    pub match_type: String,
    pub similarity_score: Option<f64>,
    pub members: Vec<DupMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupMember {
    pub media_id: String,
    pub is_preferred: bool,
    pub file_path: String,
    pub file_name: String,
    pub file_size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub date_taken: Option<String>,
    pub thumbnail: Option<String>,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupSummary {
    pub groups: Vec<DupGroup>,
    pub total_groups: usize,
    pub total_duplicates: usize,
    pub total_savings: i64,
    pub total_files_in_groups: usize,
}

#[tauri::command]
pub async fn detect_duplicates(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
) -> Result<DuplicateScanResult, String> {
    let db_ref = db.inner().clone();

    db_ref.with_conn(|conn| {
        conn.execute("DELETE FROM duplicate_members", [])?;
        conn.execute("DELETE FROM duplicate_groups", [])?;
        Ok(())
    }).map_err(|e| format!("DB: {}", e))?;

    // Phase 1: Count total files
    let (total_files, min_size): (i64, i64) = db_ref.with_conn(|conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM media_files WHERE file_size > ?1",
            params![100 * 1024_i64],
            |r| r.get(0),
        )?;
        Ok((count, 100 * 1024))
    }).map_err(|e| format!("DB: {}", e))?;

    app.emit("duplicate-progress", DuplicateProgress {
        phase: "파일 크기 분석".into(),
        total: total_files as usize,
        current: 0,
        detail: format!("전체 {} 파일 중 100KB 이상 분석 중...", total_files),
    }).ok();

    // Phase 2: Group by file size (images + videos)
    let size_groups: Vec<(i64, Vec<(String, String)>)> = db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT file_size, id, file_path FROM media_files
                 WHERE file_size > ?1 ORDER BY file_size"
            )?;
            let mut groups: HashMap<i64, Vec<(String, String)>> = HashMap::new();
            let rows = stmt
                .query_map(params![min_size], |row| Ok((row.get::<_,i64>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            for (size, id, path) in rows {
                groups.entry(size).or_default().push((id, path));
            }
            Ok(groups.into_iter().filter(|(_, v)| v.len() > 1).collect::<Vec<_>>())
        })
        .map_err(|e| format!("DB: {}", e))?;

    let total_candidates: usize = size_groups.iter().map(|(_, v)| v.len()).sum();
    let unique_count = total_files as usize - total_candidates;

    app.emit("duplicate-progress", DuplicateProgress {
        phase: "크기 분석 완료".into(),
        total: total_files as usize,
        current: total_files as usize,
        detail: format!(
            "전체 {}개 중 크기 일치 {}개 발견 ({}개는 고유 파일)",
            total_files, total_candidates, unique_count
        ),
    }).ok();

    if total_candidates == 0 {
        app.emit("duplicate-progress", DuplicateProgress {
            phase: "complete".into(), total: total_files as usize, current: total_files as usize,
            detail: "중복 후보 없음".into(),
        }).ok();
        return Ok(DuplicateScanResult {
            exact_groups: 0, similar_groups: 0, total_duplicates: 0, space_savings: 0,
        });
    }

    // Phase 3: Hash comparison
    app.emit("duplicate-progress", DuplicateProgress {
        phase: "해시 비교".into(),
        total: total_candidates,
        current: 0,
        detail: format!("크기 일치 {}개 파일의 해시 비교 시작...", total_candidates),
    }).ok();

    let mut exact_groups = 0usize;
    let mut total_duplicates = 0usize;
    let mut space_savings = 0i64;
    let mut processed = 0usize;

    for (size, members) in &size_groups {
        // Quick hash (4KB) - parallel
        let quick_results: Vec<(String, String, Option<String>)> = members
            .par_iter()
            .map(|(id, path)| (id.clone(), path.clone(), hasher::quick_hash(Path::new(path))))
            .collect();

        let mut qh_groups: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (id, path, qh) in &quick_results {
            if let Some(h) = qh {
                qh_groups.entry(h.clone()).or_default().push((id.clone(), path.clone()));
            }
        }

        for (_qh, qh_members) in &qh_groups {
            if qh_members.len() < 2 { continue; }

            // Full hash - only for quick-hash matches
            let full_results: Vec<(String, Option<String>)> = qh_members
                .par_iter()
                .map(|(id, path)| (id.clone(), hasher::full_hash(Path::new(path))))
                .collect();

            let mut fh_groups: HashMap<String, Vec<String>> = HashMap::new();
            for (id, fh) in &full_results {
                if let Some(h) = fh { fh_groups.entry(h.clone()).or_default().push(id.clone()); }
            }

            for (hash, ids) in &fh_groups {
                if ids.len() < 2 { continue; }

                let group_id = uuid::Uuid::new_v4().to_string();
                db_ref.with_conn(|conn| {
                    conn.execute(
                        "INSERT INTO duplicate_groups (id, match_type, similarity_score, status) VALUES (?1, 'exact', 1.0, 'pending')",
                        params![group_id],
                    )?;
                    for (j, id) in ids.iter().enumerate() {
                        conn.execute(
                            "INSERT INTO duplicate_members (group_id, media_id, is_preferred) VALUES (?1, ?2, ?3)",
                            params![group_id, id, if j == 0 { 1 } else { 0 }],
                        )?;
                        conn.execute("UPDATE media_files SET sha256_hash = ?1 WHERE id = ?2", params![hash, id])?;
                    }
                    Ok(())
                }).map_err(|e| format!("DB: {}", e))?;

                exact_groups += 1;
                total_duplicates += ids.len() - 1;
                space_savings += *size * (ids.len() as i64 - 1);
            }
        }

        processed += members.len();
        app.emit("duplicate-progress", DuplicateProgress {
            phase: "해시 비교".into(),
            total: total_candidates,
            current: processed,
            detail: format!(
                "{}개 비교 완료 · 중복 {}그룹 발견",
                processed, exact_groups
            ),
        }).ok();
    }

    // Phase 4: perceptual-hash pass for visually similar (not byte-identical) images
    let (similar_groups, similar_dups, similar_savings) =
        detect_similar_groups(&app, &db_ref)?;
    total_duplicates += similar_dups;
    space_savings += similar_savings;

    app.emit("duplicate-progress", DuplicateProgress {
        phase: "complete".into(),
        total: total_files as usize,
        current: total_files as usize,
        detail: format!(
            "완료: 완전 일치 {}그룹 · 유사 {}그룹 · 중복 {}개 발견",
            exact_groups, similar_groups, total_duplicates
        ),
    }).ok();

    Ok(DuplicateScanResult { exact_groups, similar_groups, total_duplicates, space_savings })
}

// How different two 64-bit perceptual hashes may be to still count as similar
const SIMILAR_HAMMING_THRESHOLD: u32 = 5;
// Degenerate band buckets (e.g. flat black bands) would explode pairwise
// comparison; anything this common is useless as a discriminator anyway
const MAX_BAND_BUCKET: usize = 500;
const PHASH_CHUNK: usize = 200;

struct Dsu {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            size: vec![1; n],
        }
    }
    // Iterative with path halving — no recursion depth risk on long chains
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    // Union by size keeps trees shallow
    fn union(&mut self, a: usize, b: usize) {
        let (mut ra, mut rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        if self.size[ra] < self.size[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        self.size[ra] += self.size[rb];
    }
}

struct SimilarCandidate {
    id: String,
    file_path: String,
    thumb_path: Option<String>,
    phash: Option<Vec<u8>>,
    file_size: i64,
    pixels: i64,
}

fn load_similar_candidates(db_ref: &Arc<Database>) -> Result<Vec<SimilarCandidate>, String> {
    db_ref
        .with_conn(|conn| {
            // Non-preferred members of pending exact groups are byte-identical
            // to their keeper; including them would only duplicate exact groups
            let mut stmt = conn.prepare(
                "SELECT mf.id, mf.file_path, mf.thumbnail, mf.phash, mf.file_size,
                        COALESCE(mf.width, 0) * COALESCE(mf.height, 0)
                 FROM media_files mf
                 WHERE mf.media_type = 'image'
                   AND mf.id NOT IN (
                     SELECT dm.media_id FROM duplicate_members dm
                     JOIN duplicate_groups dg ON dm.group_id = dg.id
                     WHERE dg.status = 'pending' AND dm.is_preferred = 0
                   )",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    let thumb: Option<String> = row.get(2)?;
                    Ok(SimilarCandidate {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        // legacy rows may still hold base64 here — only cache file
                        // paths (always *.jpg; base64 has no '.') are usable
                        thumb_path: thumb.filter(|t| t.ends_with(".jpg")),
                        phash: row.get(3)?,
                        file_size: row.get(4)?,
                        pixels: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))
}

// Compute missing perceptual hashes (thumbnail file preferred — far cheaper
// to decode than originals), persist them, then group via 8-band LSH + DSU.
fn detect_similar_groups(
    app: &AppHandle,
    db_ref: &Arc<Database>,
) -> Result<(usize, usize, i64), String> {
    let mut candidates = load_similar_candidates(db_ref)?;
    let need_hash: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.phash.is_none())
        .map(|(i, _)| i)
        .collect();

    let total_to_hash = need_hash.len();
    for (done, chunk) in need_hash.chunks(PHASH_CHUNK).enumerate() {
        let computed: Vec<(usize, Option<Vec<u8>>)> = chunk
            .par_iter()
            .map(|&i| {
                let c = &candidates[i];
                let source = c.thumb_path.as_deref().unwrap_or(&c.file_path);
                (i, hasher::compute_phash(Path::new(source)))
            })
            .collect();

        db_ref
            .with_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                for (i, hash) in &computed {
                    if let Some(h) = hash {
                        tx.execute(
                            "UPDATE media_files SET phash = ?1 WHERE id = ?2",
                            params![h, candidates[*i].id],
                        )?;
                    }
                }
                tx.commit()
            })
            .map_err(|e| format!("DB: {}", e))?;

        for (i, hash) in computed {
            candidates[i].phash = hash;
        }

        app.emit("duplicate-progress", DuplicateProgress {
            phase: "유사 이미지 분석".into(),
            total: total_to_hash,
            current: ((done + 1) * PHASH_CHUNK).min(total_to_hash),
            detail: format!("이미지 지각 해시 계산 중... ({}개)", total_to_hash),
        }).ok();
    }

    let hashed: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.phash.as_ref().map(|h| h.len() == 8).unwrap_or(false))
        .map(|(i, _)| i)
        .collect();

    // LSH banding: similar hashes (≤7 differing bits) share at least one
    // identical byte-band, so only bucket-mates need pairwise comparison
    let mut buckets: HashMap<(usize, u8), Vec<usize>> = HashMap::new();
    for &i in &hashed {
        let hash = candidates[i].phash.as_ref().unwrap();
        for (band, byte) in hash.iter().enumerate() {
            buckets.entry((band, *byte)).or_default().push(i);
        }
    }

    let mut dsu = Dsu::new(candidates.len());
    for bucket in buckets.values() {
        if bucket.len() < 2 || bucket.len() > MAX_BAND_BUCKET {
            continue;
        }
        for a_pos in 0..bucket.len() {
            for b_pos in (a_pos + 1)..bucket.len() {
                let (a, b) = (bucket[a_pos], bucket[b_pos]);
                if dsu.find(a) == dsu.find(b) {
                    continue;
                }
                let dist = hasher::hamming_distance(
                    candidates[a].phash.as_ref().unwrap(),
                    candidates[b].phash.as_ref().unwrap(),
                );
                if dist <= SIMILAR_HAMMING_THRESHOLD {
                    dsu.union(a, b);
                }
            }
        }
    }

    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in &hashed {
        clusters.entry(dsu.find(i)).or_default().push(i);
    }

    let mut similar_groups = 0usize;
    let mut duplicates = 0usize;
    let mut savings = 0i64;

    for members in clusters.values().filter(|m| m.len() > 1) {
        // Keep the highest-resolution (then largest) file by default
        let preferred = *members
            .iter()
            .max_by_key(|&&i| (candidates[i].pixels, candidates[i].file_size))
            .unwrap();
        let max_dist = max_pairwise_distance(&candidates, members);
        let score = 1.0 - (max_dist as f64) / 64.0;

        let group_id = uuid::Uuid::new_v4().to_string();
        db_ref
            .with_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute(
                    "INSERT INTO duplicate_groups (id, match_type, similarity_score, status)
                     VALUES (?1, 'similar', ?2, 'pending')",
                    params![group_id, score],
                )?;
                for &i in members {
                    tx.execute(
                        "INSERT INTO duplicate_members (group_id, media_id, is_preferred)
                         VALUES (?1, ?2, ?3)",
                        params![group_id, candidates[i].id, if i == preferred { 1 } else { 0 }],
                    )?;
                }
                tx.commit()
            })
            .map_err(|e| format!("DB: {}", e))?;

        similar_groups += 1;
        duplicates += members.len() - 1;
        savings += members
            .iter()
            .filter(|&&i| i != preferred)
            .map(|&i| candidates[i].file_size)
            .sum::<i64>();
    }

    Ok((similar_groups, duplicates, savings))
}

fn max_pairwise_distance(candidates: &[SimilarCandidate], members: &[usize]) -> u32 {
    let mut max_dist = 0u32;
    for a_pos in 0..members.len() {
        for b_pos in (a_pos + 1)..members.len() {
            let dist = hasher::hamming_distance(
                candidates[members[a_pos]].phash.as_ref().unwrap(),
                candidates[members[b_pos]].phash.as_ref().unwrap(),
            );
            max_dist = max_dist.max(dist);
        }
    }
    max_dist
}

#[cfg(test)]
mod tests {
    use super::Dsu;

    #[test]
    fn dsu_groups_transitively() {
        let mut dsu = Dsu::new(5);
        dsu.union(0, 1);
        dsu.union(1, 2);
        assert_eq!(dsu.find(0), dsu.find(2));
        assert_ne!(dsu.find(0), dsu.find(3));
        assert_ne!(dsu.find(3), dsu.find(4));
    }

    #[test]
    fn dsu_union_is_idempotent() {
        let mut dsu = Dsu::new(3);
        dsu.union(0, 1);
        dsu.union(0, 1);
        dsu.union(1, 0);
        assert_eq!(dsu.find(0), dsu.find(1));
        let root = dsu.find(0);
        assert_eq!(dsu.size[root], 2);
    }

    #[test]
    fn dsu_handles_long_chains_without_recursion() {
        let n = 100_000;
        let mut dsu = Dsu::new(n);
        for i in 0..n - 1 {
            dsu.union(i, i + 1);
        }
        assert_eq!(dsu.find(0), dsu.find(n - 1));
    }
}

// Fast: no thumbnail generation, just DB query
#[tauri::command]
pub async fn get_duplicate_groups(db: State<'_, Arc<Database>>) -> Result<DupSummary, String> {
    let db_ref = db.inner().clone();

    db_ref.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, match_type, similarity_score FROM duplicate_groups
             WHERE status = 'pending' ORDER BY match_type ASC"
        )?;
        let groups_raw = stmt.query_map([], |row| {
            Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,Option<f64>>(2)?))
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut all_groups = Vec::new();
        let mut total_duplicates = 0usize;
        let mut total_savings = 0i64;
        let mut total_files = 0usize;

        for (group_id, match_type, sim) in &groups_raw {
            let mut mem_stmt = conn.prepare(
                "SELECT dm.media_id, dm.is_preferred, mf.file_path, mf.file_name, mf.file_size,
                        mf.width, mf.height, me.date_taken, mf.thumbnail, mf.media_type
                 FROM duplicate_members dm
                 JOIN media_files mf ON dm.media_id = mf.id
                 LEFT JOIN media_exif me ON mf.id = me.media_id
                 WHERE dm.group_id = ?1"
            )?;
            let members: Vec<DupMember> = mem_stmt.query_map(params![group_id], |row| {
                Ok(DupMember {
                    media_id: row.get(0)?,
                    is_preferred: row.get::<_,i32>(1)? != 0,
                    file_path: row.get(2)?,
                    file_name: row.get(3)?,
                    file_size: row.get(4)?,
                    width: row.get(5)?,
                    height: row.get(6)?,
                    date_taken: row.get(7)?,
                    thumbnail: row.get(8)?,
                    media_type: row.get(9)?,
                })
            })?.collect::<Result<Vec<_>, _>>()?;

            let dup_count = members.len().saturating_sub(1);
            let savings: i64 = members.iter().filter(|m| !m.is_preferred).map(|m| m.file_size).sum();

            total_duplicates += dup_count;
            total_savings += savings;
            total_files += members.len();

            all_groups.push(DupGroup {
                id: group_id.clone(),
                match_type: match_type.clone(),
                similarity_score: *sim,
                members,
            });
        }

        Ok(DupSummary {
            total_groups: all_groups.len(),
            total_duplicates,
            total_savings,
            total_files_in_groups: total_files,
            groups: all_groups,
        })
    }).map_err(|e| format!("DB: {}", e))
}

// Set a specific member as preferred (keep this one) in a duplicate group
#[tauri::command]
pub async fn set_preferred_member(
    db: State<'_, Arc<Database>>,
    group_id: String,
    media_id: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            // Reset all members in this group to not preferred
            conn.execute(
                "UPDATE duplicate_members SET is_preferred = 0 WHERE group_id = ?1",
                params![group_id],
            )?;
            // Set the chosen member as preferred
            conn.execute(
                "UPDATE duplicate_members SET is_preferred = 1 WHERE group_id = ?1 AND media_id = ?2",
                params![group_id, media_id],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

// Dismiss a duplicate group (mark as resolved, keep all files)
#[tauri::command]
pub async fn dismiss_duplicate_group(
    db: State<'_, Arc<Database>>,
    group_id: String,
) -> Result<(), String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "UPDATE duplicate_groups SET status = 'dismissed' WHERE id = ?1",
                params![group_id],
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))
}

// Dry-run preview: list files that would be trashed without executing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunItem {
    pub media_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: i64,
}

#[tauri::command]
pub async fn preview_trash_duplicates(
    db: State<'_, Arc<Database>>,
) -> Result<Vec<DryRunItem>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT dm.media_id, mf.file_path, mf.file_name, mf.file_size
                 FROM duplicate_members dm
                 JOIN duplicate_groups dg ON dm.group_id = dg.id
                 JOIN media_files mf ON dm.media_id = mf.id
                 WHERE dg.status = 'pending' AND dm.is_preferred = 0",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(DryRunItem {
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

// Trash all non-preferred duplicate files across all pending groups
#[tauri::command]
pub async fn trash_duplicate_files(
    db: State<'_, Arc<Database>>,
) -> Result<crate::core::trash::TrashResult, String> {
    let db_ref = db.inner().clone();

    let files_to_trash: Vec<(String, String)> = db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT dm.media_id, mf.file_path
                 FROM duplicate_members dm
                 JOIN duplicate_groups dg ON dm.group_id = dg.id
                 JOIN media_files mf ON dm.media_id = mf.id
                 WHERE dg.status = 'pending' AND dm.is_preferred = 0"
            )?;
            let rows = stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))?;

    crate::core::trash::trash_and_cleanup_db(&db_ref, &files_to_trash)
}

// Trash non-preferred files in a single group
#[tauri::command]
pub async fn trash_group_duplicates(
    db: State<'_, Arc<Database>>,
    group_id: String,
) -> Result<crate::core::trash::TrashResult, String> {
    let db_ref = db.inner().clone();

    let files_to_trash: Vec<(String, String)> = db_ref
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT dm.media_id, mf.file_path
                 FROM duplicate_members dm
                 JOIN media_files mf ON dm.media_id = mf.id
                 WHERE dm.group_id = ?1 AND dm.is_preferred = 0"
            )?;
            let rows = stmt
                .query_map(params![group_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| format!("DB: {}", e))?;

    crate::core::trash::trash_and_cleanup_db(&db_ref, &files_to_trash)
}

// Reveal file in Finder (select the file in its parent folder)
#[tauri::command]
pub async fn open_file(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to reveal in Finder: {}", e))?;
    Ok(())
}

// Quick Look preview (spacebar-like)
#[tauri::command]
pub async fn preview_file(path: String) -> Result<(), String> {
    std::process::Command::new("qlmanage")
        .arg("-p")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to preview: {}", e))?;
    Ok(())
}
