//! 되돌리기 — 작업 묶음 하나를 통째로 물린다.
//!
//! 저널에 남긴 (from, to)를 거꾸로 밟는다. 순서도 거꾸로다 — 같은 배치 안에서
//! 이름이 부딪혀 번호가 붙은 경우, 나중 것부터 물려야 원래 이름으로 돌아간다.
//!
//! 되돌린 배치는 `undone_at`이 찍힌다. 두 번 되돌리지 않기 위해서다.

use crate::db::conn::{Db, Result};
use crate::ops::trash::{move_with_sidecars, Outcome};

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
///
/// 물릴 게 없어진 묶음은 먼저 닫는다 — 휴지통 화면에서 이미 되돌린 «휴지통으로»,
/// 다시 휴지통에 간 «되돌리기». 열어 두면 상태바 단추가 그걸 가리킨 채 남는다 (실측 2026-08-30)
pub fn recent(db: &Db, limit: usize) -> Result<Vec<Batch>> {
    let limit = limit.clamp(1, 200);
    db.write(|c| {
        // 닫히지 못한 묶음(도중에 멈춘 합치기 등) — 저널이 있으면 그 수로 닫아 되돌릴 수 있게
        c.execute(
            "UPDATE batches SET item_count = (SELECT COUNT(*) FROM journal j WHERE j.batch_id = batches.id AND j.ok = 1)
             WHERE item_count = 0 AND undone_at IS NULL
               AND EXISTS (SELECT 1 FROM journal j WHERE j.batch_id = batches.id AND j.ok = 1)
               AND created_at < strftime('%s','now') - 60",
            [],
        )?;
        c.execute(
            "UPDATE batches SET undone_at = strftime('%s','now')
             WHERE undone_at IS NULL AND kind = 'trash' AND item_count > 0
               AND NOT EXISTS (SELECT 1 FROM journal j JOIN files f ON f.id = j.file_id
                               WHERE j.batch_id = batches.id AND j.ok = 1 AND f.trashed_at IS NOT NULL)",
            [],
        )?;
        c.execute(
            "UPDATE batches SET undone_at = strftime('%s','now')
             WHERE undone_at IS NULL AND kind = 'restore' AND item_count > 0
               AND NOT EXISTS (SELECT 1 FROM journal j JOIN files f ON f.id = j.file_id
                               WHERE j.batch_id = batches.id AND j.ok = 1 AND f.trashed_at IS NULL)",
            [],
        )
    })?;
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
    /// 원래 있던 볼륨 — 되돌리면 여기로 간다
    from_vol: String,
    from_path: String,
    /// 지금 있는 볼륨 — 볼륨을 넘어간 이동이면 `from_vol`과 다르다
    to_vol: String,
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
    // 영구히 지운 것은 되돌릴 수 없다 — 파일이 디스크에 없다. 되돌리기 후보에도 안 오르지만
    // (화면이 거른다) 명령으로 와도 거절한다
    if kind == "delete" {
        return Ok(Outcome {
            batch_id,
            first_error: Some("영구히 지운 사진은 되돌릴 수 없습니다".into()),
            ..Default::default()
        });
    }

    if kind == "capture_date" {
        return crate::ops::capture_date::undo(db, batch_id);
    }
    if kind == "copy" || kind == "publish" {
        return crate::ops::transfer::undo_copy(db, batch_id);
    }

    // 가져오기는 되돌릴 곳이 없다. 원본은 카드에 그대로 있고, 되돌린다는 건
    // 「들여온 벌을 무른다」는 뜻이다. 그렇다고 지워 버리면 그것대로 되돌릴
    // 수 없으니 휴지통으로 보낸다.
    if kind == "import" {
        let ids: Vec<i64> = db.read(|c| {
            let mut st = c.prepare(
                "SELECT file_id FROM journal WHERE batch_id = ?1 AND ok = 1 AND file_id IS NOT NULL",
            )?;
            let it = st.query_map([batch_id], |r| r.get(0))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        let out = crate::ops::trash::to_trash(db, &ids, "가져오기 되돌리기")?;
        // 하나도 못 옮겼으면(카드·디스크가 빠짐) 배치를 열어 둔다 — 아래 일반 갈래와 같다
        if out.moved > 0 || ids.is_empty() {
            mark_undone(db, batch_id)?;
        }
        return Ok(Outcome { batch_id, ..out });
    }

    // 휴지통은 전용 경로가 있다 — trashed_at·trash_path를 함께 되돌려야 한다
    if kind == "trash" {
        let ids: Vec<i64> = db.read(|c| {
            let mut st = c.prepare(
                "SELECT j.file_id FROM journal j JOIN files f ON f.id = j.file_id
                 WHERE j.batch_id = ?1 AND j.ok = 1 AND f.trashed_at IS NOT NULL",
            )?;
            let it = st.query_map([batch_id], |r| r.get(0))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        // 휴지통 화면에서 이미 되돌렸으면 할 일이 없다 — 배치를 닫고 그렇게 말한다.
        // (열어 두면 «되돌리기» 단추가 눌러도 아무 일 없이 남는다 — 실측 2026-08-30)
        if ids.is_empty() {
            mark_undone(db, batch_id)?;
            return Ok(Outcome {
                batch_id,
                first_error: Some("이미 휴지통에서 되돌린 사진입니다".into()),
                ..Default::default()
            });
        }
        let out = crate::ops::trash::restore(db, &ids)?;
        if out.moved > 0 {
            mark_undone(db, batch_id)?;
        }
        return Ok(Outcome { batch_id, ..out });
    }

    // 휴지통에서 되돌린 것을 물린다 = 다시 휴지통으로. 그새 다른 길로 휴지통에 갔으면 할 일이 없다
    if kind == "restore" {
        let ids: Vec<i64> = db.read(|c| {
            let mut st = c.prepare(
                "SELECT j.file_id FROM journal j JOIN files f ON f.id = j.file_id
                 WHERE j.batch_id = ?1 AND j.ok = 1 AND f.trashed_at IS NULL",
            )?;
            let it = st.query_map([batch_id], |r| r.get(0))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        if ids.is_empty() {
            mark_undone(db, batch_id)?;
            return Ok(Outcome {
                batch_id,
                first_error: Some("이미 휴지통에 있는 사진입니다".into()),
                ..Default::default()
            });
        }
        let out = crate::ops::trash::to_trash(db, &ids, "되돌리기 취소 — 다시 휴지통으로")?;
        if out.moved > 0 {
            mark_undone(db, batch_id)?;
        }
        return Ok(Outcome { batch_id, ..out });
    }

    let rows: Vec<Row> = db.read(|c| {
        // 나중 것부터 — 같은 배치에서 이름이 밀린 경우를 제자리로 돌린다
        let mut st = c.prepare(
            "SELECT file_id, from_vol, from_path, COALESCE(to_vol, from_vol), to_path FROM journal
             WHERE batch_id = ?1 AND ok = 1 AND file_id IS NOT NULL AND to_path IS NOT NULL
             ORDER BY id DESC",
        )?;
        let it = st.query_map([batch_id], |r| {
            Ok(Row {
                file_id: r.get(0)?,
                from_vol: r.get(1)?,
                from_path: r.get(2)?,
                to_vol: r.get(3)?,
                to_path: r.get(4)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let mut out = Outcome { batch_id, ..Default::default() };
    // 볼륨마다 마운트는 한 번만 찾는다
    let mut mounts: std::collections::HashMap<&str, Option<std::path::PathBuf>> =
        std::collections::HashMap::new();
    for row in &rows {
        let now_mount = mount_cached(&mut mounts, &row.to_vol);
        let back_mount = mount_cached(&mut mounts, &row.from_vol);
        let (Some(now_mount), Some(back_mount)) = (now_mount, back_mount) else {
            out.failed += 1;
            out.first_error
                .get_or_insert("디스크가 연결되어 있지 않습니다".into());
            continue;
        };
        let from = now_mount.join(&row.to_path); // 지금 있는 곳
        // 원래 자리에 그새 다른 파일이 생겼을 수 있다 — 덮어쓰지 않고 옆에 놓는다
        // (리뷰: rename은 있는 파일을 소리 없이 바꿔치기한다)
        let to = crate::ops::trash::free_path(back_mount.join(&row.from_path));
        let to_rel = to
            .strip_prefix(&back_mount)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| row.from_path.clone());
        match move_with_sidecars(&from, &to) {
            Ok(()) => {
                repoint(db, row.file_id, &row.from_vol, &to_rel)?;
                out.moved += 1;
            }
            Err(e) => {
                out.failed += 1;
                out.first_error.get_or_insert(e.to_string());
            }
        }
    }
    // 하나도 못 돌렸으면(디스크가 빠짐) 배치를 열어 둔다 — 꽂고 다시 시도할 수 있게
    if out.moved > 0 || rows.is_empty() {
        mark_undone(db, batch_id)?;
    }
    Ok(out)
}

fn mount_cached<'a>(
    m: &mut std::collections::HashMap<&'a str, Option<std::path::PathBuf>>,
    volume_uuid: &'a str,
) -> Option<std::path::PathBuf> {
    m.entry(volume_uuid)
        .or_insert_with(|| crate::db::volumes::find_mount(volume_uuid))
        .clone()
}

/// 파일 행이 원래 폴더를 가리키게 되돌린다. 폴더 행이 사라졌으면 되살린다.
fn repoint(db: &Db, file_id: i64, volume_uuid: &str, vol_rel: &str) -> Result<()> {
    let (dir, name) = match vol_rel.rsplit_once('/') {
        Some((d, n)) => (d.to_string(), n.to_string()),
        None => (String::new(), vol_rel.to_string()),
    };
    // 이 볼륨에서 그 경로를 품는 라이브러리를 찾는다 — 구역(area)도 그 라이브러리의 것.
    // 상수 1(내사진)로 박으면 작업대로 돌아온 폴더가 정착 구역으로 잡혀 고르기가 건너뛴다
    let lib: Option<(i64, i32)> = db.read(|c| {
        use rusqlite::OptionalExtension;
        c.query_row(
            "SELECT id, area FROM libraries WHERE volume_uuid = ?1
               AND (rel_path = '' OR ?2 = rel_path OR substr(?2, 1, length(rel_path) + 1) = rel_path || '/')
             ORDER BY length(rel_path) DESC LIMIT 1",
            rusqlite::params![volume_uuid, dir],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    })?;
    let library_id = lib.map(|l| l.0);
    let area = lib.map(|l| l.1).unwrap_or(0);
    let folder_name = dir.rsplit('/').next().unwrap_or(&dir).to_string();
    db.write(|c| {
        c.execute(
            "INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at)
             VALUES(?1,?2,?3,?4,?5,strftime('%s','now'))
             ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET library_id=COALESCE(excluded.library_id, library_id)",
            rusqlite::params![volume_uuid, library_id, dir, folder_name, area],
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

pub(crate) fn mark_undone(db: &Db, batch_id: i64) -> Result<()> {
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
    fn journal_keeps_the_destination_volume_apart_from_the_source() {
        let (_d, db, _lib, ids) = setup();
        let batch = crate::ops::open_batch(&db, "move", "볼륨 넘어가기").unwrap();
        crate::ops::record_to(&db, batch, "move", ids[0], "VOL-A", "a/x.jpg", "VOL-B", Some("b/x.jpg"), Ok(()))
            .unwrap();
        let (from, to): (String, String) = db
            .read(|c| {
                c.query_row("SELECT from_vol, to_vol FROM journal WHERE batch_id = ?1", [batch], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .unwrap();
        assert_eq!((from.as_str(), to.as_str()), ("VOL-A", "VOL-B"), "to_vol 이 from_vol 에 묶이지 않는다");
    }

    #[test]
    fn a_trash_undo_that_moved_nothing_stays_undoable() {
        let (dir, db, _lib, ids) = setup();
        let out = trash::to_trash(&db, &ids[..1], "치우기").unwrap();
        // 휴지통의 파일이 그새 사라졌다 — 되돌릴 것이 없다
        let trash_path: String = db
            .read(|c| c.query_row("SELECT trash_path FROM files WHERE id = ?1", [ids[0]], |r| r.get(0)))
            .unwrap();
        let _ = std::fs::remove_file(dir.path().join(&trash_path));
        let _ = std::fs::remove_file(&trash_path);
        let u = undo(&db, out.batch_id).unwrap();
        let undone: Option<i64> = db
            .read(|c| c.query_row("SELECT undone_at FROM batches WHERE id=?1", [out.batch_id], |r| r.get(0)))
            .unwrap();
        if u.moved == 0 {
            assert!(undone.is_none(), "하나도 못 돌렸으면 배치는 열려 있어야 한다");
        } else {
            assert!(undone.is_some());
        }
    }

    #[test]
    fn undoing_a_trash_batch_after_the_trash_view_restored_it_just_closes_it() {
        let (dir, db, _lib, ids) = setup();
        let t = trash::to_trash(&db, &ids[..1], "휴지통으로").unwrap();
        trash::restore(&db, &ids[..1]).unwrap(); // 휴지통 화면에서 되돌렸다
        let u = undo(&db, t.batch_id).unwrap();
        assert_eq!((u.moved, u.failed), (0, 0));
        assert!(u.first_error.as_deref().unwrap_or("").contains("이미"));
        let undone: Option<i64> = db
            .read(|c| c.query_row("SELECT undone_at FROM batches WHERE id=?1", [t.batch_id], |r| r.get(0)))
            .unwrap();
        assert!(undone.is_some(), "할 일이 없는 배치는 닫힌다 — 단추가 영영 남지 않게");
        assert!(dir.path().join("작업대/a.jpg").is_file(), "사진은 제자리");
    }

    #[test]
    fn undoing_a_restore_puts_the_photo_back_into_the_trash() {
        let (dir, db, _lib, ids) = setup();
        trash::to_trash(&db, &ids[..1], "휴지통으로").unwrap();
        let r = trash::restore(&db, &ids[..1]).unwrap();
        assert!(dir.path().join("작업대/a.jpg").is_file());
        let u = undo(&db, r.batch_id).unwrap();
        assert_eq!((u.moved, u.failed), (1, 0), "{u:?}");
        assert!(!dir.path().join("작업대/a.jpg").exists(), "다시 휴지통으로");
        let trashed: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files WHERE trashed_at IS NOT NULL", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(trashed, 1);
    }

    #[test]
    fn empty_operations_do_not_leave_batches_behind() {
        let (_d, db, _lib, ids) = setup();
        let before: i64 = db.read(|c| c.query_row("SELECT COUNT(*) FROM batches", [], |r| r.get(0))).unwrap();
        let r = trash::restore(&db, &ids).unwrap(); // 휴지통이 비었다
        assert_eq!(r.moved, 0);
        assert!(r.first_error.is_some());
        let after: i64 = db.read(|c| c.query_row("SELECT COUNT(*) FROM batches", [], |r| r.get(0))).unwrap();
        assert_eq!(before, after, "빈 배치가 생기지 않는다");
    }

    #[test]
    fn a_permanent_delete_cannot_be_undone() {
        let (_d, db, _lib, ids) = setup();
        trash::to_trash(&db, &ids[..1], "휴지통으로").unwrap();
        let e = trash::empty(&db, &ids[..1]).unwrap();
        let u = undo(&db, e.batch_id).unwrap();
        assert_eq!(u.moved, 0);
        assert!(u.first_error.as_deref().unwrap_or("").contains("되돌릴 수 없"));
        let undone: Option<i64> = db
            .read(|c| c.query_row("SELECT undone_at FROM batches WHERE id=?1", [e.batch_id], |r| r.get(0)))
            .unwrap();
        assert!(undone.is_none(), "«되돌린 작업»으로 꾸미지 않는다 — 지운 건 지운 것");
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
    fn recent_closes_trash_batches_that_have_nothing_left_to_undo() {
        let (_d, db, _lib, ids) = setup();
        let t = trash::to_trash(&db, &ids[..1], "휴지통으로").unwrap();
        assert!(recent(&db, 10).unwrap().iter().any(|b| b.id == t.batch_id && b.undone_at.is_none()));
        trash::restore(&db, &ids[..1]).unwrap(); // 휴지통 화면에서 되돌림
        let list = recent(&db, 10).unwrap();
        let b = list.iter().find(|b| b.id == t.batch_id).unwrap();
        assert!(b.undone_at.is_some(), "물릴 게 없는 묶음은 목록을 읽을 때 닫힌다");
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


    #[test]
    fn undo_does_not_overwrite_a_newer_file_in_the_old_place() {
        let (dir, db, lib, ids) = setup();
        let dest = Dest { library_id: lib, rel_dir: "2024/행사".into() };
        let out = move_to(&db, &ids[..1], &dest, "정리").unwrap();
        // 그새 같은 이름의 새 사진이 원래 자리에 들어왔다
        std::fs::write(dir.path().join("작업대/a.jpg"), b"NEW PHOTO").unwrap();

        let u = undo(&db, out.batch_id).unwrap();
        assert_eq!((u.moved, u.failed), (1, 0));
        assert_eq!(std::fs::read(dir.path().join("작업대/a.jpg")).unwrap(), b"NEW PHOTO", "새 사진은 그대로");
        assert!(dir.path().join("작업대/a (2).jpg").is_file(), "돌아온 것은 옆에 놓인다");
        let name: String = db
            .read(|c| c.query_row("SELECT name FROM files WHERE id=?1", [ids[0]], |r| r.get(0)))
            .unwrap();
        assert_eq!(name, "a (2).jpg", "DB도 새 이름을 안다");
    }

    #[test]
    fn a_fully_failed_undo_stays_undoable() {
        let (dir, db, lib, ids) = setup();
        let dest = Dest { library_id: lib, rel_dir: "2024/행사".into() };
        let out = move_to(&db, &ids[..1], &dest, "정리").unwrap();
        // 옮긴 파일이 사라져 되돌릴 수 없다
        std::fs::remove_file(dir.path().join("2024/행사/a.jpg")).unwrap();
        let u = undo(&db, out.batch_id).unwrap();
        assert_eq!((u.moved, u.failed), (0, 1));
        let undone: Option<i64> = db
            .read(|c| c.query_row("SELECT undone_at FROM batches WHERE id=?1", [out.batch_id], |r| r.get(0)))
            .unwrap();
        assert!(undone.is_none(), "하나도 못 돌렸으면 «되돌린 것»으로 찍지 않는다");
    }
}
