//! 되돌리기 — 작업 묶음 하나를 통째로 물린다.
//!
//! 저널에 남긴 (from, to)를 거꾸로 밟는다. 순서도 거꾸로다 — 같은 배치 안에서
//! 이름이 부딪혀 번호가 붙은 경우, 나중 것부터 물려야 원래 이름으로 돌아간다.
//!
//! 되돌린 배치는 `undone_at`이 찍힌다. 두 번 되돌리지 않기 위해서다.

use crate::db::conn::{Db, Result};
use crate::ops::trash::{move_file, Outcome};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Batch {
    pub id: i64,
    pub kind: String,
    pub label: Option<String>,
    pub item_count: i64,
    pub created_at: i64,
    pub undone_at: Option<i64>,
}

/// 최근 작업 묶음들. 되돌릴 수 있는 것이 위에 온다.
pub fn recent(db: &Db, limit: usize) -> Result<Vec<Batch>> {
    let limit = limit.clamp(1, 200);
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT id, kind, label, item_count, created_at, undone_at
             FROM batches WHERE item_count > 0
             ORDER BY id DESC LIMIT ?1",
        )?;
        let it = st.query_map([limit as i64], |r| {
            Ok(Batch {
                id: r.get(0)?,
                kind: r.get(1)?,
                label: r.get(2)?,
                item_count: r.get(3)?,
                created_at: r.get(4)?,
                undone_at: r.get(5)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

struct Row {
    file_id: i64,
    volume_uuid: String,
    from_path: String,
    to_path: String,
}

/// 배치 하나를 되돌린다.
pub fn undo(db: &Db, batch_id: i64) -> Result<Outcome> {
    let (kind, undone): (String, Option<i64>) = db.read(|c| {
        c.query_row(
            "SELECT kind, undone_at FROM batches WHERE id = ?1",
            [batch_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    })?;
    if undone.is_some() {
        return Ok(Outcome {
            batch_id,
            first_error: Some("이미 되돌린 작업입니다".into()),
            ..Default::default()
        });
    }

    // 휴지통은 전용 경로가 있다 — trashed_at·trash_path를 함께 되돌려야 한다
    if kind == "trash" {
        let ids: Vec<i64> = db.read(|c| {
            let mut st = c.prepare(
                "SELECT file_id FROM journal WHERE batch_id = ?1 AND ok = 1 AND file_id IS NOT NULL",
            )?;
            let it = st.query_map([batch_id], |r| r.get(0))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        let out = crate::ops::trash::restore(db, &ids)?;
        mark_undone(db, batch_id)?;
        return Ok(Outcome { batch_id, ..out });
    }

    let rows: Vec<Row> = db.read(|c| {
        // 나중 것부터 — 같은 배치에서 이름이 밀린 경우를 제자리로 돌린다
        let mut st = c.prepare(
            "SELECT file_id, from_vol, from_path, to_path FROM journal
             WHERE batch_id = ?1 AND ok = 1 AND file_id IS NOT NULL AND to_path IS NOT NULL
             ORDER BY id DESC",
        )?;
        let it = st.query_map([batch_id], |r| {
            Ok(Row {
                file_id: r.get(0)?,
                volume_uuid: r.get(1)?,
                from_path: r.get(2)?,
                to_path: r.get(3)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let mut out = Outcome { batch_id, ..Default::default() };
    for row in &rows {
        let Some(mount) = crate::db::volumes::find_mount(&row.volume_uuid) else {
            out.failed += 1;
            out.first_error
                .get_or_insert("디스크가 연결되어 있지 않습니다".into());
            continue;
        };
        let from = mount.join(&row.to_path); // 지금 있는 곳
        let to = mount.join(&row.from_path); // 원래 있던 곳
        match move_file(&from, &to) {
            Ok(()) => {
                repoint(db, row.file_id, &row.volume_uuid, &row.from_path)?;
                out.moved += 1;
            }
            Err(e) => {
                out.failed += 1;
                out.first_error.get_or_insert(e.to_string());
            }
        }
    }
    mark_undone(db, batch_id)?;
    Ok(out)
}

/// 파일 행이 원래 폴더를 가리키게 되돌린다. 폴더 행이 사라졌으면 되살린다.
fn repoint(db: &Db, file_id: i64, volume_uuid: &str, vol_rel: &str) -> Result<()> {
    let (dir, name) = match vol_rel.rsplit_once('/') {
        Some((d, n)) => (d.to_string(), n.to_string()),
        None => (String::new(), vol_rel.to_string()),
    };
    // 이 볼륨에서 그 경로를 품는 라이브러리를 찾는다
    let library_id: Option<i64> = db.read(|c| {
        use rusqlite::OptionalExtension;
        c.query_row(
            "SELECT id FROM libraries WHERE volume_uuid = ?1
               AND (rel_path = '' OR ?2 = rel_path OR ?2 LIKE rel_path || '/%')
             ORDER BY length(rel_path) DESC LIMIT 1",
            rusqlite::params![volume_uuid, dir],
            |r| r.get(0),
        )
        .optional()
    })?;
    let folder_name = dir.rsplit('/').next().unwrap_or(&dir).to_string();
    db.write(|c| {
        c.execute(
            "INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at)
             VALUES(?1,?2,?3,?4,1,strftime('%s','now'))
             ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET library_id=COALESCE(excluded.library_id, library_id)",
            rusqlite::params![volume_uuid, library_id, dir, folder_name],
        )?;
        c.execute(
            "UPDATE files SET name = ?2,
                    folder_id = (SELECT id FROM folders WHERE volume_uuid=?3 AND rel_path=?4)
             WHERE id = ?1",
            rusqlite::params![file_id, name, volume_uuid, dir],
        )
    })?;
    Ok(())
}

fn mark_undone(db: &Db, batch_id: i64) -> Result<()> {
    db.write(|c| {
        c.execute(
            "UPDATE batches SET undone_at = strftime('%s','now') WHERE id = ?1",
            [batch_id],
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::organize::{move_to, Dest};
    use crate::ops::trash;
    use crate::scan::scan_test;

    fn setup() -> (tempfile::TempDir, Db, i64, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("작업대");
        std::fs::create_dir_all(&src).unwrap();
        for n in ["a.jpg", "b.jpg"] {
            std::fs::write(src.join(n), b"bytes ".repeat(20)).unwrap();
        }
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        let lib: i64 = db
            .read(|c| c.query_row("SELECT id FROM libraries", [], |r| r.get(0)))
            .unwrap();
        let ids: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare("SELECT id FROM files ORDER BY name")?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        (dir, db, lib, ids)
    }

    #[test]
    fn undo_puts_moved_files_back() {
        let (dir, db, lib, ids) = setup();
        let dest = Dest { library_id: lib, rel_dir: "2024/행사".into() };
        let out = move_to(&db, &ids, &dest, "정리").unwrap();
        assert!(!dir.path().join("작업대/a.jpg").exists());

        let u = undo(&db, out.batch_id).unwrap();
        assert_eq!((u.moved, u.failed), (2, 0));
        assert!(dir.path().join("작업대/a.jpg").is_file(), "원래 자리로 돌아온다");
        assert!(!dir.path().join("2024/행사/a.jpg").exists());

        // rel_path는 볼륨 기준이라 임시 폴더에서는 앞이 길다. 끝만 본다.
        let rel: String = db
            .read(|c| {
                c.query_row(
                    "SELECT fo.rel_path FROM files fi JOIN folders fo ON fo.id=fi.folder_id
                     WHERE fi.id=?1",
                    [ids[0]],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert!(rel.ends_with("작업대"), "DB도 함께 돌아온다: {rel}");
    }

    #[test]
    fn undo_of_a_trash_batch_restores_the_flag_too() {
        let (dir, db, _lib, ids) = setup();
        let out = trash::to_trash(&db, &ids[..1], "치우기").unwrap();
        let trashed: i64 = db
            .read(|c| {
                c.query_row("SELECT COUNT(*) FROM files WHERE trashed_at IS NOT NULL", [], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert_eq!(trashed, 1);

        undo(&db, out.batch_id).unwrap();
        assert!(dir.path().join("작업대/a.jpg").is_file());
        let still: i64 = db
            .read(|c| {
                c.query_row("SELECT COUNT(*) FROM files WHERE trashed_at IS NOT NULL", [], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert_eq!(still, 0, "휴지통 표시도 지워져야 목록에 다시 나온다");
    }

    #[test]
    fn undoing_twice_does_nothing() {
        let (_d, db, lib, ids) = setup();
        let dest = Dest { library_id: lib, rel_dir: "2024/행사".into() };
        let out = move_to(&db, &ids, &dest, "정리").unwrap();
        undo(&db, out.batch_id).unwrap();
        let again = undo(&db, out.batch_id).unwrap();
        assert_eq!(again.moved, 0);
        assert!(again.first_error.is_some());
    }

    #[test]
    fn recent_lists_newest_first_and_shows_undone() {
        let (_d, db, lib, ids) = setup();
        let a = move_to(&db, &ids[..1], &Dest { library_id: lib, rel_dir: "x".into() }, "1").unwrap();
        let b = move_to(&db, &ids[1..], &Dest { library_id: lib, rel_dir: "y".into() }, "2").unwrap();
        undo(&db, a.batch_id).unwrap();

        let list = recent(&db, 10).unwrap();
        assert_eq!(list[0].id, b.batch_id, "최근 것이 위");
        let first = list.iter().find(|x| x.id == a.batch_id).unwrap();
        assert!(first.undone_at.is_some(), "되돌린 표시가 남는다");
    }
}
