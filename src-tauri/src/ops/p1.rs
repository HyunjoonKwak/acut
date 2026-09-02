//! Gallery→Desk P1 — 폴더 이름 감사와 작업대 이벤트 자동 발견.
//!
//! 감사와 발견은 DB·파일을 바꾸지 않는다. 폴더명 적용은 기존 `folder` 작업을
//! 자식 배치로 실행해 manifest·경로 안전판을 그대로 쓰고, 사용자에게는 하나의
//! 부모 배치로 보여 일괄 undo한다.

use crate::db::conn::{Db, DbError, Result};
use crate::ops::{folder, naming, organize};
use chrono::{Local, NaiveDate};
use std::collections::{BTreeMap, HashSet};
use walkdir::WalkDir;

fn bad(message: impl Into<String>) -> DbError {
    DbError::Invalid(message.into())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FolderAuditItem {
    pub source_dir: String,
    pub parent_dir: String,
    pub current_name: String,
    pub proposed_name: String,
    pub reason: String,
    pub file_count: i64,
    pub conflict: bool,
}

fn valid_date(value: &str) -> bool {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}

fn normalized_folder_name(name: &str) -> Option<(String, String)> {
    let (date, title) = naming::parse_folder(name);
    let date = date?;
    if !valid_date(&date) {
        return None;
    }
    let proposed = organize::event_folder_name(&date, &title);
    if proposed == name {
        return None;
    }
    let reason = if name.as_bytes().get(4) == Some(&b'_') {
        "밑줄 날짜를 YYYY-MM-DD로 통일".to_string()
    } else if name.as_bytes().get(4) == Some(&b'.') {
        "점 날짜를 YYYY-MM-DD로 통일".to_string()
    } else {
        "붙어 있는 날짜를 YYYY-MM-DD로 통일".to_string()
    };
    Some((proposed, reason))
}

fn library_relative(library_root: &str, volume_relative: &str) -> Option<String> {
    if library_root.is_empty() {
        return Some(volume_relative.trim_matches('/').to_string());
    }
    if volume_relative == library_root {
        return Some(String::new());
    }
    volume_relative
        .strip_prefix(library_root)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(ToString::to_string)
}

/// 라이브러리의 날짜 폴더명을 읽기만 하며 제안한다.
pub fn audit_folder_names(db: &Db, library_id: i64) -> Result<Vec<FolderAuditItem>> {
    let library = crate::db::libraries::get(db, library_id)?
        .ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    let root = library.dir.clone().ok_or_else(|| {
        bad(format!(
            "「{}」 디스크가 연결되어 있지 않습니다",
            library.name
        ))
    })?;
    let rows: Vec<(String, i64)> = db.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT fo.rel_path,COUNT(fi.id) FROM folders fo
             LEFT JOIN files fi ON fi.folder_id=fo.id AND fi.trashed_at IS NULL
             WHERE fo.library_id=?1 GROUP BY fo.id",
        )?;
        let found = statement.query_map([library_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        found.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    // DB는 사진이 든 leaf만 저장할 수 있다. 디스크의 부모·빈 폴더도 감사해야 하므로
    // leaf 장수를 조상에게 올린 뒤 실제 디렉터리를 읽기 전용으로 순회한다.
    let mut counts = BTreeMap::<String, i64>::new();
    for (volume_relative, count) in rows {
        let Some(mut rel) = library_relative(&library.rel_path, &volume_relative) else {
            continue;
        };
        loop {
            *counts.entry(rel.clone()).or_default() += count;
            let Some((parent, _)) = rel.rsplit_once('/') else {
                break;
            };
            rel = parent.to_string();
        }
    }

    let mut out = Vec::new();
    let entries = WalkDir::new(&root)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !name.starts_with('.') && name != "@eaDir"
        });
    for entry in entries {
        let entry = entry.map_err(|error| bad(error.to_string()))?;
        if !entry.file_type().is_dir() {
            continue;
        }
        let source_dir = entry
            .path()
            .strip_prefix(&root)
            .map_err(|_| bad("폴더 감사 경로를 계산하지 못했습니다"))?
            .to_string_lossy();
        let source_dir = crate::scan::nfc(&source_dir);
        let current_name = crate::scan::nfc(&entry.file_name().to_string_lossy());
        if source_dir.is_empty() || current_name.is_empty() {
            continue;
        }
        let Some((proposed_name, reason)) = normalized_folder_name(&current_name) else {
            continue;
        };
        let parent_dir = source_dir
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
            .unwrap_or_default();
        let destination = if parent_dir.is_empty() {
            root.join(&proposed_name)
        } else {
            root.join(&parent_dir).join(&proposed_name)
        };
        let file_count = counts.get(&source_dir).copied().unwrap_or_default();
        out.push(FolderAuditItem {
            source_dir,
            parent_dir,
            current_name,
            proposed_name,
            reason,
            file_count,
            conflict: destination.exists(),
        });
    }
    out.sort_by(|a, b| a.source_dir.cmp(&b.source_dir));
    Ok(out)
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct FolderAuditOutcome {
    pub batch_id: i64,
    pub completed: usize,
    pub failed: usize,
    pub conflicts: usize,
    pub first_error: Option<String>,
}

/// 선택한 제안만 적용한다. 부모 폴더보다 깊은 폴더를 먼저 이름 바꿔 경로가
/// 중간에 사라지지 않게 하고, 기존 목적지가 있으면 절대 병합하지 않는다.
pub fn apply_folder_names(
    db: &Db,
    library_id: i64,
    source_dirs: &[String],
) -> Result<FolderAuditOutcome> {
    if source_dirs.is_empty() {
        return Ok(FolderAuditOutcome::default());
    }
    let wanted: HashSet<String> = source_dirs.iter().map(|p| crate::scan::nfc(p)).collect();
    let mut proposals = audit_folder_names(db, library_id)?
        .into_iter()
        .filter(|item| wanted.contains(&item.source_dir))
        .collect::<Vec<_>>();
    proposals.sort_by(|a, b| {
        b.source_dir
            .matches('/')
            .count()
            .cmp(&a.source_dir.matches('/').count())
            .then(a.source_dir.cmp(&b.source_dir))
    });

    let parent = super::open_batch(db, "folder_audit", "폴더 이름 감사 적용")?;
    let mut out = FolderAuditOutcome {
        batch_id: parent,
        ..Default::default()
    };
    for item in proposals {
        if item.conflict {
            out.conflicts += 1;
            out.failed += 1;
            out.first_error
                .get_or_insert(format!("같은 이름이 있습니다: {}", item.proposed_name));
            continue;
        }
        let request = folder::Request {
            action: folder::Action::Rename,
            source_library_id: library_id,
            source_dir: item.source_dir.clone(),
            destination_library_id: Some(library_id),
            destination_parent: Some(item.parent_dir.clone()),
            name: Some(item.proposed_name.clone()),
            conflict_policy: folder::ConflictPolicy::Skip,
        };
        let label = format!("{} → {}", item.current_name, item.proposed_name);
        let result = match folder::execute(db, &request, &label) {
            Ok(result) => result,
            Err(error) => {
                out.failed += 1;
                out.first_error.get_or_insert(error.to_string());
                continue;
            }
        };
        if result.completed == 1 {
            let seq = out.completed as i64;
            if let Err(error) = db.write(|connection| {
                connection.execute(
                    "INSERT INTO folder_audit_children(parent_batch_id,child_batch_id,seq)
                     VALUES(?1,?2,?3)",
                    rusqlite::params![parent, result.batch_id, seq],
                )
            }) {
                // 자식 배치는 이미 완성됐다. 연결 저장만 실패하면 자식 자체는 최근
                // 작업에 남으므로 개별 undo가 가능하다. 부모를 열린 채 숨기지 않는다.
                out.failed += 1;
                out.first_error.get_or_insert(error.to_string());
                continue;
            }
            out.completed += 1;
        } else {
            out.failed += 1;
            out.first_error.get_or_insert_with(|| {
                result
                    .first_error
                    .unwrap_or_else(|| "폴더 이름 변경 실패".into())
            });
        }
    }
    if out.completed == 0 {
        db.write(|connection| connection.execute("DELETE FROM batches WHERE id=?1", [parent]))?;
        out.batch_id = 0;
    } else {
        super::close_batch(db, parent, out.completed)?;
    }
    Ok(out)
}

/// `folder_audit` 부모 배치를 자식 rename의 역순으로 되돌린다. 일부만 실패하면
/// 부모를 열어 둬 디스크를 다시 연결하거나 충돌을 치운 뒤 재시도할 수 있다.
pub fn undo_folder_audit(db: &Db, parent: i64) -> Result<crate::ops::trash::Outcome> {
    let children: Vec<i64> = db.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT p.child_batch_id FROM folder_audit_children p
             JOIN batches b ON b.id=p.child_batch_id
             WHERE p.parent_batch_id=?1 AND b.undone_at IS NULL
             ORDER BY p.seq DESC",
        )?;
        let rows = statement.query_map([parent], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let mut out = crate::ops::trash::Outcome {
        batch_id: parent,
        ..Default::default()
    };
    for child in children {
        let child_out = folder::undo(db, child)?;
        out.moved += child_out.moved;
        out.failed += child_out.failed;
        out.bytes += child_out.bytes;
        if out.first_error.is_none() {
            out.first_error = child_out.first_error;
        }
    }
    let remaining: i64 = db.read(|connection| {
        connection.query_row(
            "SELECT COUNT(*) FROM folder_audit_children p JOIN batches b ON b.id=p.child_batch_id
             WHERE p.parent_batch_id=?1 AND b.undone_at IS NULL",
            [parent],
            |row| row.get(0),
        )
    })?;
    if remaining == 0 {
        crate::ops::undo::mark_undone(db, parent)?;
    }
    Ok(out)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventItem {
    pub id: i64,
    pub name: String,
    pub taken_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventCandidate {
    pub key: String,
    pub date: String,
    pub start_at: i64,
    pub end_at: i64,
    pub count: usize,
    pub items: Vec<EventItem>,
    pub suggestions: Vec<naming::Suggestion>,
}

fn local_day(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|date| date.with_timezone(&Local).format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// 작업대 라이브러리를 시간 간격과 날짜 경계로 묶는다. 읽기 전용이며, 모든
/// 항목을 돌려줘 UI에서 사진별 제외가 가능하다.
pub fn event_candidates(
    db: &Db,
    library_id: i64,
    gap_minutes: u32,
    min_count: usize,
) -> Result<Vec<EventCandidate>> {
    let library = crate::db::libraries::get(db, library_id)?
        .ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    if library.area != 0 {
        return Err(bad("이벤트 자동 발견은 작업대 라이브러리에서만 실행합니다"));
    }
    if library.dir.is_none() {
        return Err(bad(format!(
            "「{}」 디스크가 연결되어 있지 않습니다",
            library.name
        )));
    }
    let items: Vec<EventItem> = db.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT fi.id,fi.name,fi.taken_at FROM files fi
             JOIN folders fo ON fo.id=fi.folder_id
             WHERE fo.library_id=?1 AND fi.trashed_at IS NULL
             ORDER BY fi.taken_at,fi.id",
        )?;
        let rows = statement.query_map([library_id], |row| {
            Ok(EventItem {
                id: row.get(0)?,
                name: row.get(1)?,
                taken_at: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let gap = i64::from(gap_minutes.clamp(30, 48 * 60)) * 60;
    let minimum = min_count.clamp(2, 5_000);
    let mut groups: Vec<Vec<EventItem>> = Vec::new();
    for item in items {
        let split = groups
            .last()
            .and_then(|group| group.last())
            .is_some_and(|previous| {
                item.taken_at - previous.taken_at > gap
                    || local_day(item.taken_at) != local_day(previous.taken_at)
            });
        if split || groups.is_empty() {
            groups.push(Vec::new());
        }
        groups.last_mut().expect("group exists").push(item);
    }

    let groups = groups
        .into_iter()
        .filter(|group| group.len() >= minimum)
        .collect::<Vec<_>>();
    let suggestion_index = naming::SuggestionIndex::load(db)?;
    let mut out = Vec::with_capacity(groups.len());
    for group in groups {
        let start_at = group.first().map(|item| item.taken_at).unwrap_or_default();
        let end_at = group.last().map(|item| item.taken_at).unwrap_or_default();
        let ids = group.iter().map(|item| item.id).collect::<Vec<_>>();
        out.push(EventCandidate {
            key: format!("{library_id}:{start_at}:{end_at}"),
            date: local_day(start_at),
            start_at,
            end_at,
            count: group.len(),
            suggestions: suggestion_index.suggest(db, &ids, 5)?,
            items: group,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_media(root: &std::path::Path, rel: &str, names: &[&str]) {
        let dir = root.join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            std::fs::write(dir.join(name), format!("fixture-{rel}-{name}")).unwrap();
        }
    }

    #[test]
    fn folder_audit_is_dry_run_and_batch_undo_restores_every_name() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("작업대");
        add_media(
            &root,
            "2024_08_27 여행/20240828_둘째날",
            &["20240828_120000.jpg"],
        );
        add_media(&root, "2024_02_30 잘못된날", &["x.jpg"]);
        add_media(&root, "2024.09.01 행사", &["20240901_120000.jpg"]);
        add_media(&root, "2024-09-01 행사", &["기존.jpg"]);
        let db = Db::open(temp.path().join("test.db")).unwrap();
        let library = crate::db::libraries::add(&db, &root, 0).unwrap();
        crate::scan::scan_folder(&db, library.id, &root, 0, |_| {}).unwrap();
        let before_batches: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM batches", [], |r| r.get(0)))
            .unwrap();

        let audit = audit_folder_names(&db, library.id).unwrap();
        assert_eq!(
            db.read(|c| c.query_row("SELECT COUNT(*) FROM batches", [], |r| r.get::<_, i64>(0)))
                .unwrap(),
            before_batches,
            "dry-run은 DB를 바꾸지 않는다"
        );
        assert!(
            audit
                .iter()
                .any(|item| item.proposed_name == "2024-08-27 여행"),
            "{audit:#?}"
        );
        assert!(audit
            .iter()
            .any(|item| item.proposed_name == "2024-08-28 둘째날"));
        assert!(!audit.iter().any(|item| item.current_name.contains("02_30")));
        assert!(audit
            .iter()
            .any(|item| item.current_name == "2024.09.01 행사" && item.conflict));

        let selected = audit
            .iter()
            .map(|item| item.source_dir.clone())
            .collect::<Vec<_>>();
        let applied = apply_folder_names(&db, library.id, &selected).unwrap();
        assert_eq!((applied.completed, applied.conflicts), (2, 1));
        assert!(root.join("2024-08-27 여행/2024-08-28 둘째날").is_dir());
        let undo = undo_folder_audit(&db, applied.batch_id).unwrap();
        assert_eq!((undo.moved, undo.failed), (2, 0));
        assert!(root.join("2024_08_27 여행/20240828_둘째날").is_dir());
    }

    #[test]
    fn event_discovery_splits_on_gap_and_midnight_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("작업대");
        add_media(
            &root,
            "받은사진",
            &[
                "20240827_120000.jpg",
                "20240827_121000.jpg",
                "20240827_122000.jpg",
                "20240827_180000.jpg",
                "20240827_181000.jpg",
                "20240828_001000.jpg",
                "20240828_002000.jpg",
            ],
        );
        let db = Db::open(temp.path().join("test.db")).unwrap();
        let library = crate::db::libraries::add(&db, &root, 0).unwrap();
        crate::scan::scan_folder(&db, library.id, &root, 0, |_| {}).unwrap();
        let before: (i64, i64) = db
            .read(|c| {
                c.query_row(
                    "SELECT (SELECT COUNT(*) FROM batches),(SELECT COUNT(*) FROM journal)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        let found = event_candidates(&db, library.id, 60, 2).unwrap();
        assert_eq!(
            found.iter().map(|event| event.count).collect::<Vec<_>>(),
            vec![3, 2, 2]
        );
        let after: (i64, i64) = db
            .read(|c| {
                c.query_row(
                    "SELECT (SELECT COUNT(*) FROM batches),(SELECT COUNT(*) FROM journal)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(before, after, "후보 발견은 journal을 만들지 않는다");

        db.write(|c| c.execute("UPDATE libraries SET area=1 WHERE id=?1", [library.id]))
            .unwrap();
        assert!(event_candidates(&db, library.id, 60, 2).is_err());
    }

    #[test]
    fn normalization_preserves_titles_and_rejects_bad_calendar_dates() {
        assert_eq!(
            normalized_folder_name("20240827_거제 여행").map(|value| value.0),
            Some("2024-08-27 거제 여행".into())
        );
        assert_eq!(
            normalized_folder_name("2024.08.27-생일").map(|value| value.0),
            Some("2024-08-27 생일".into())
        );
        assert!(normalized_folder_name("2024_02_30 오류").is_none());
        assert!(normalized_folder_name("2024-08-27 이미 정상").is_none());
    }
}
