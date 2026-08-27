//! 이름 바꾸기 — 파일 하나의 이름을 그 자리에서.
//!
//! 같은 이름이 이미 있으면 **바꾸지 않고 묻는다** (비켜 쓰지 않는다). 이름을
//! 바꾸는 사람은 그 이름을 원한 것이라 «IMG_1 (2).jpg»가 되면 뜻이 없다.
//! 저널에 남기므로 ⌘Z로 돌아온다 — 되돌리기의 일반 분기(경로 이동)가 처리한다.

use crate::db::conn::{Db, DbError, Result};
use crate::ops::trash::move_file;

/// 새 이름을 돌려준다 (NFC로 맞춘 것).
pub fn rename(db: &Db, id: i64, new_name: &str) -> Result<String> {
    let new_name = crate::scan::nfc(new_name.trim());
    if new_name.is_empty() {
        return Err(DbError::Invalid("이름이 비어 있습니다".into()));
    }
    if new_name.contains('/') || new_name == "." || new_name == ".." {
        return Err(DbError::Invalid("이름에 쓸 수 없는 글자가 있습니다".into()));
    }

    let (uuid, rel_dir, old_name): (String, String, String) = db.read(|c| {
        c.query_row(
            "SELECT fo.volume_uuid, fo.rel_path, fi.name
               FROM files fi JOIN folders fo ON fo.id = fi.folder_id
              WHERE fi.id = ?1 AND fi.trashed_at IS NULL",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    })?;
    if old_name == new_name {
        return Ok(new_name);
    }
    let mount = crate::db::volumes::find_mount(&uuid)
        .ok_or_else(|| DbError::Invalid("디스크가 연결되어 있지 않습니다".into()))?;
    let from_rel = crate::media::cache::rel_path(&rel_dir, &old_name);
    let to_rel = crate::media::cache::rel_path(&rel_dir, &new_name);
    let from = mount.join(&from_rel);
    let to = mount.join(&to_rel);
    if !from.exists() {
        return Err(DbError::Invalid(format!("파일이 없습니다: {old_name}")));
    }
    // 대소문자만 다른 이름은 같은 파일이다(exFAT·APFS 기본). 그건 허용한다.
    if to.exists() && !old_name.eq_ignore_ascii_case(&new_name) {
        return Err(DbError::Invalid(format!("같은 이름의 파일이 이미 있습니다: {new_name}")));
    }

    let batch = super::open_batch(db, "rename", &format!("{old_name} → {new_name}"))?;
    match move_file(&from, &to) {
        Ok(()) => {
            super::record(db, batch, "rename", id, &uuid, &from_rel, Some(&to_rel), Ok(()))?;
            let ext = std::path::Path::new(&new_name)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase());
            db.write(|c| {
                c.execute(
                    "UPDATE files SET name = ?2, ext = ?3 WHERE id = ?1",
                    rusqlite::params![id, new_name, ext],
                )
            })?;
            super::close_batch(db, batch, 1)?;
            Ok(new_name)
        }
        Err(e) => {
            let msg = e.to_string();
            super::record(db, batch, "rename", id, &uuid, &from_rel, None, Err(&msg))?;
            super::close_batch(db, batch, 0)?;
            Err(DbError::Invalid(msg))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> (tempfile::TempDir, Db, i64) {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("IMG_1.jpg"), b"a").unwrap();
        std::fs::write(lib.join("IMG_2.jpg"), b"b").unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        crate::scan::scan_test(&db, &lib, 1, |_| {}).unwrap();
        let id: i64 = db
            .read(|c| c.query_row("SELECT id FROM files WHERE name='IMG_1.jpg'", [], |r| r.get(0)))
            .unwrap();
        (dir, db, id)
    }

    #[test]
    fn renames_on_disk_and_in_db() {
        let (d, db, id) = seeded();
        let n = rename(&db, id, " 주원 첫돌.jpg ").unwrap();
        assert_eq!(n, "주원 첫돌.jpg", "앞뒤 공백을 뗀다");
        assert!(d.path().join("lib/주원 첫돌.jpg").is_file());
        assert!(!d.path().join("lib/IMG_1.jpg").exists());
        let (name, ext): (String, String) = db
            .read(|c| c.query_row("SELECT name, ext FROM files WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))))
            .unwrap();
        assert_eq!(name, "주원 첫돌.jpg");
        assert_eq!(ext, "jpg");
    }

    /// 이름을 바꾸는 사람은 그 이름을 원한 것 — 비켜 쓰지 않고 거절한다
    #[test]
    fn refuses_when_the_name_is_taken() {
        let (d, db, id) = seeded();
        let r = rename(&db, id, "IMG_2.jpg");
        assert!(r.is_err());
        assert!(d.path().join("lib/IMG_1.jpg").is_file(), "그대로다");
    }

    #[test]
    fn refuses_bad_names() {
        let (_d, db, id) = seeded();
        assert!(rename(&db, id, "").is_err());
        assert!(rename(&db, id, "a/b.jpg").is_err());
        assert!(rename(&db, id, "..").is_err());
    }

    #[test]
    fn same_name_is_a_no_op() {
        let (_d, db, id) = seeded();
        assert_eq!(rename(&db, id, "IMG_1.jpg").unwrap(), "IMG_1.jpg");
        let n: i64 = db.read(|c| c.query_row("SELECT COUNT(*) FROM batches", [], |r| r.get(0))).unwrap();
        assert_eq!(n, 0, "저널도 안 남긴다");
    }

    /// ⌘Z — 되돌리기의 일반 분기가 이름을 되돌린다
    #[test]
    fn undo_puts_the_old_name_back() {
        let (d, db, id) = seeded();
        rename(&db, id, "새.jpg").unwrap();
        let batch: i64 = db.read(|c| c.query_row("SELECT MAX(id) FROM batches", [], |r| r.get(0))).unwrap();
        let out = crate::ops::undo::undo(&db, batch).unwrap();
        assert_eq!(out.failed, 0, "{:?}", out.first_error);
        assert!(d.path().join("lib/IMG_1.jpg").is_file());
        let name: String = db.read(|c| c.query_row("SELECT name FROM files WHERE id=?1", [id], |r| r.get(0))).unwrap();
        assert_eq!(name, "IMG_1.jpg");
    }
}
