//! DB 백업 — 판정·평점·태그 78,857장분이 파일 하나다.
//!
//! `VACUUM INTO`를 쓴다. 파일을 그냥 복사하면 WAL에 아직 안 합쳐진 쓰기가
//! 빠지고, 쓰는 중이면 반쯤 깨진 사본이 나온다. VACUUM INTO는 SQLite가
//! 일관된 시점의 사본을 새 파일로 써 준다 — 켜 둔 채로 해도 된다.
//!
//! 이름에 시각을 넣고 오래된 것부터 지워 몇 벌만 남긴다.

use crate::db::conn::{Db, Result};
use std::path::{Path, PathBuf};

/// 남겨 둘 벌 수. 매일 한 벌이면 사흘치다.
pub const KEEP: usize = 3;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Backup {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
    /// 만든 시각 (유닉스 초)
    pub made_at: i64,
}

fn stamp(now: i64) -> String {
    // 2024-08-27T14:03:22 → 20240827-140322. 파일명에 콜론을 안 넣는다.
    let t = chrono::DateTime::from_timestamp(now, 0).unwrap_or_default();
    t.format("%Y%m%d-%H%M%S").to_string()
}

/// `acut-20240827-140322.db` → 유닉스 초. 모양이 다르면 None.
fn parse_stamp(name: &str) -> Option<i64> {
    let core = name.strip_prefix("acut-")?.strip_suffix(".db")?;
    let t = chrono::NaiveDateTime::parse_from_str(core, "%Y%m%d-%H%M%S").ok()?;
    Some(t.and_utc().timestamp())
}

/// 백업 한 벌을 만든다. 돌아오는 값은 만든 파일.
pub fn make(db: &Db, dir: &Path, now: i64) -> Result<Backup> {
    std::fs::create_dir_all(dir)?;
    let name = format!("acut-{}.db", stamp(now));
    let path = dir.join(&name);
    // 같은 초에 두 번 부르면 이름이 겹친다. VACUUM INTO는 있는 파일에 안 쓴다.
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    db.write(|c| c.execute("VACUUM INTO ?1", [path.to_string_lossy().as_ref()]))?;
    let bytes = std::fs::metadata(&path)?.len();
    prune(dir, KEEP)?;
    Ok(Backup { path, name, bytes, made_at: now })
}

/// 앱을 켤 때 부른다 — 마지막 사본이 이보다 오래됐으면 한 벌 더 뜬다.
pub const AUTO_EVERY_SECS: i64 = 24 * 3600;

/// 오래됐으면 한 벌 뜬다. 안 떴으면 None.
pub fn make_if_stale(db: &Db, dir: &Path, now: i64) -> Result<Option<Backup>> {
    let newest = list(dir)?.into_iter().next();
    if let Some(b) = newest {
        if now - b.made_at < AUTO_EVERY_SECS {
            return Ok(None);
        }
    }
    make(db, dir, now).map(Some)
}

/// 사본으로 되돌린다. **되돌리기 전 지금 상태를 먼저 한 벌 뜬다** — 잘못
/// 고른 사본이었을 때 돌아올 길이다.
pub fn restore(db: &Db, dir: &Path, from: &Path, now: i64) -> Result<Backup> {
    if !from.is_file() {
        return Err(crate::db::conn::DbError::Invalid(format!(
            "사본이 없습니다: {}",
            from.display()
        )));
    }
    let safety = make(db, dir, now)?;
    db.restore_from(from)?;
    Ok(safety)
}

/// 있는 백업들 — 최신 것부터.
pub fn list(dir: &Path) -> Result<Vec<Backup>> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(out) };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("acut-") && name.ends_with(".db")) {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        // 만든 시각은 이름에서 읽는다. 파일 mtime은 복사·동기화로 바뀐다.
        let made_at = parse_stamp(&name).unwrap_or(0);
        out.push(Backup { path: e.path(), name, bytes: md.len(), made_at });
    }
    // 이름에 시각이 있어 이름 내림차순이 곧 최신순이다
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

/// 최신 `keep`벌만 남기고 지운다.
pub fn prune(dir: &Path, keep: usize) -> Result<()> {
    for old in list(dir)?.into_iter().skip(keep) {
        std::fs::remove_file(&old.path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| c.execute("INSERT INTO tags(name) VALUES('여행')", []))
            .unwrap();
        (dir, db)
    }

    /// 사본을 열면 원본에 있던 것이 그대로 있어야 한다.
    #[test]
    fn a_backup_is_a_usable_copy() {
        let (d, db) = seeded();
        let b = make(&db, &d.path().join("backups"), 1_724_716_800).unwrap();
        assert!(b.path.is_file());
        assert!(b.bytes > 0);
        assert_eq!(b.name, "acut-20240827-000000.db");

        let copy = rusqlite::Connection::open(&b.path).unwrap();
        let n: i64 = copy
            .query_row("SELECT COUNT(*) FROM tags WHERE name='여행'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    /// 백업을 만드는 동안에도 원본은 계속 쓸 수 있어야 한다.
    #[test]
    fn the_source_keeps_working_after_backup() {
        let (d, db) = seeded();
        make(&db, &d.path().join("backups"), 1_724_716_800).unwrap();
        db.write(|c| c.execute("INSERT INTO tags(name) VALUES('가족')", []))
            .unwrap();
        let n: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn only_the_newest_few_are_kept() {
        let (d, db) = seeded();
        let dir = d.path().join("backups");
        for i in 0..5 {
            make(&db, &dir, 1_724_716_800 + i * 60).unwrap();
        }
        let l = list(&dir).unwrap();
        assert_eq!(l.len(), KEEP);
        // 최신 것부터
        assert_eq!(l[0].name, "acut-20240827-000400.db");
        assert_eq!(l[KEEP - 1].name, "acut-20240827-000200.db");
    }

    #[test]
    fn a_missing_folder_lists_nothing() {
        let d = tempfile::tempdir().unwrap();
        assert!(list(&d.path().join("없음")).unwrap().is_empty());
    }

    /// 사본으로 되돌리면 그 사본의 내용이 되고, 되돌리기 직전 상태도 한 벌 남는다.
    #[test]
    fn restore_replaces_contents_and_keeps_a_safety_copy() {
        let (d, db) = seeded(); // tags: 여행
        let dir = d.path().join("backups");
        let snap = make(&db, &dir, 1_724_716_800).unwrap();

        // 그 뒤에 바꾼다
        db.write(|c| c.execute("INSERT INTO tags(name) VALUES('가족')", []))
            .unwrap();
        let n = |db: &Db| -> i64 {
            db.read(|c| c.query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0)))
                .unwrap()
        };
        assert_eq!(n(&db), 2);

        let safety = restore(&db, &dir, &snap.path, 1_724_716_900).unwrap();
        assert_eq!(n(&db), 1, "사본 시점(여행만)으로 돌아간다");
        assert!(safety.path.is_file());
        // 안전 사본에는 되돌리기 직전(둘)이 들어 있다
        let c = rusqlite::Connection::open(&safety.path).unwrap();
        let m: i64 = c.query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0)).unwrap();
        assert_eq!(m, 2);
        // 되돌린 뒤에도 계속 쓸 수 있다
        db.write(|c| c.execute("INSERT INTO tags(name) VALUES('생일')", []))
            .unwrap();
        assert_eq!(n(&db), 2);
    }

    #[test]
    fn restore_from_a_missing_file_is_refused() {
        let (d, db) = seeded();
        let r = restore(&db, &d.path().join("b"), &d.path().join("없음.db"), 0);
        assert!(r.is_err());
    }

    /// 하루 안에 뜬 사본이 있으면 또 뜨지 않는다 — 켤 때마다 한 벌씩 쌓이면
    /// 사흘치가 세 시간치가 된다.
    #[test]
    fn auto_backup_only_when_the_last_one_is_old() {
        let (d, db) = seeded();
        let dir = d.path().join("backups");
        let t = 1_724_716_800;
        assert!(make_if_stale(&db, &dir, t).unwrap().is_some(), "처음엔 뜬다");
        assert!(make_if_stale(&db, &dir, t + 3600).unwrap().is_none(), "한 시간 뒤엔 안 뜬다");
        assert!(
            make_if_stale(&db, &dir, t + AUTO_EVERY_SECS + 1).unwrap().is_some(),
            "하루 지나면 뜬다"
        );
        assert_eq!(list(&dir).unwrap().len(), 2);
    }

    /// 백업 폴더에 다른 파일이 있어도 건드리지 않는다.
    #[test]
    fn unrelated_files_are_left_alone() {
        let (d, db) = seeded();
        let dir = d.path().join("backups");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("메모.txt"), "x").unwrap();
        for i in 0..5 {
            make(&db, &dir, 1_724_716_800 + i * 60).unwrap();
        }
        assert!(dir.join("메모.txt").is_file());
    }
}
