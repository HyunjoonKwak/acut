//! 무리를 한꺼번에 확정하는 규칙과 폴더 단위 요약.
//!
//! 완전 중복 69,360무리(실측)를 Space로 하나씩 넘길 수는 없다. 바이트가 같으니
//! 대표 규칙(정착 구역 우선 → 이른 촬영일)만 믿으면 되고, 위험한 무리 — 정착
//! 구역(내사진·공용)에 제외될 사본이 있는 것 — 만 건너뛰어 사람이 본다.
//! 거기서 지우면 Drive 동기화가 NAS에서도 지운다.

use rusqlite::{params, Transaction};
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ApplyAll {
    /// 확정한(또는 dry_run이면 확정할) 무리 수
    pub groups: usize,
    pub kept: usize,
    pub rejected: usize,
    /// 정착 구역의 사본이 있어 건너뛴 무리 수
    pub skipped: usize,
}

/// 무리 몇 개를 **사람이 보고** 확정한다 — 대표는 남김, 나머지는 제외. (남긴 수, 제외한 수)
///
/// 한꺼번에 확정([`apply_all`])과 달리 정착 구역(내사진·공용)의 사본도, 이미 «남김»인
/// 사본도 제외한다: 그 두 장을 눈으로 보고 누른 것이 결정이다. 폴더 비교가 남긴 쪽에
/// 붙여 둔 «남김»(실측 2026-08-30: 공용 46,784장)이 개별 비교의 확정을 조용히 막아
/// «확정했는데 아무 일도 없다»가 됐었다. 없는 무리(다시 찾기로 사라진 것)는 건너뛴다.
pub fn apply_groups(tx: &Transaction, group_ids: &[i64]) -> rusqlite::Result<(usize, usize)> {
    use rusqlite::OptionalExtension;
    let mut kept = 0;
    let mut rejected = 0;
    for gid in group_ids {
        let Some(kind) = tx
            .query_row("SELECT kind FROM groups WHERE id=?1", [gid], |r| r.get::<_, i32>(0))
            .optional()?
        else {
            continue;
        };
        // 잡동사니(kind 1)는 대표가 없어 전부 제외
        let best = if kind == 1 { "" } else { " AND is_best = 0" };
        rejected += tx.execute(
            &format!(
                "UPDATE files SET culling_flag = 2 WHERE trashed_at IS NULL AND id IN
                 (SELECT file_id FROM group_members WHERE group_id = ?1{best})"
            ),
            [gid],
        )?;
        if kind != 1 {
            kept += tx.execute(
                "UPDATE files SET culling_flag = 1 WHERE id IN
                 (SELECT file_id FROM group_members WHERE group_id = ?1 AND is_best = 1)",
                [gid],
            )?;
        }
        tx.execute("UPDATE groups SET state = 1 WHERE id = ?1", [gid])?;
    }
    Ok((kept, rejected))
}

/// 갈래의 미결 무리를 한꺼번에 확정한다. `folder_id`·`library_id`를 주면 거기에
/// 제외될 사본이 있는 무리만. `dry_run`이면 세기만 하고 바꾸지 않는다.
///
/// 잡동사니(kind 1)는 대표가 없어 전부 제외다.
/// 이미 «남김»(1)인 파일은 어느 갈래에서도 «제외»로 내리지 않는다 — 완전 중복의 대표를
/// 비슷한 장면이 제외해 두 벌 다 지우는 길을 막는다 (리뷰 C11).
pub fn apply_all(
    tx: &Transaction,
    kind: i32,
    skip_settled: bool,
    dry_run: bool,
    folder_id: Option<i64>,
    library_id: Option<i64>,
) -> rusqlite::Result<ApplyAll> {
    tx.execute_batch(
        "DROP TABLE IF EXISTS temp.todo; CREATE TEMP TABLE todo(id INTEGER PRIMARY KEY);",
    )?;
    tx.execute(
        "INSERT INTO temp.todo
         SELECT g.id FROM groups g
         WHERE g.kind = ?1 AND g.state = 0
           AND (?2 IS NULL OR EXISTS (
                 SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                 WHERE m.group_id = g.id AND m.is_best = 0 AND f.folder_id = ?2))
           AND (?3 IS NULL OR EXISTS (
                 SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                 JOIN folders fo ON fo.id = f.folder_id
                 WHERE m.group_id = g.id AND m.is_best = 0 AND fo.library_id = ?3))",
        params![kind, folder_id, library_id],
    )?;
    let total = tx.query_row("SELECT COUNT(*) FROM temp.todo", [], |r| r.get::<_, i64>(0))? as usize;
    // 잡동사니는 무리가 «사유»별이라 수천 장이 한 무리다 — 정착 구역 한 장 때문에 무리째
    // 건너뛰면 버튼이 영영 먹통이다. 구성원 단위로 정착 구역만 빼고 나머지는 표시한다 (리뷰 H7)
    let skipped = if skip_settled && kind == 1 {
        tx.query_row(
            "SELECT COUNT(DISTINCT m.file_id) FROM group_members m
             JOIN files f ON f.id = m.file_id JOIN folders fo ON fo.id = f.folder_id
             WHERE m.group_id IN (SELECT id FROM temp.todo) AND fo.area IN (1, 2)",
            [],
            |r| r.get::<_, i64>(0),
        )? as usize
    } else if skip_settled {
        tx.execute(
            "DELETE FROM temp.todo WHERE id IN (
               SELECT m.group_id FROM group_members m
               JOIN files f ON f.id = m.file_id
               JOIN folders fo ON fo.id = f.folder_id
               WHERE m.is_best = 0 AND fo.area IN (1, 2))",
            [],
        )?
    } else {
        0
    };
    let count = |best: i32| -> rusqlite::Result<usize> {
        tx.query_row(
            "SELECT COUNT(DISTINCT file_id) FROM group_members
             WHERE group_id IN (SELECT id FROM temp.todo) AND is_best = ?1",
            [best],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as usize)
    };
    let (kept, rejected) = if kind == 1 {
        let area = if skip_settled { " AND fo.area NOT IN (1, 2)" } else { "" };
        let all = tx.query_row(
            &format!(
                "SELECT COUNT(DISTINCT m.file_id) FROM group_members m
                 JOIN files f ON f.id = m.file_id JOIN folders fo ON fo.id = f.folder_id
                 WHERE m.group_id IN (SELECT id FROM temp.todo){area}"
            ),
            [],
            |r| r.get::<_, i64>(0),
        )? as usize;
        if !dry_run {
            tx.execute(
                &format!(
                    "UPDATE files SET culling_flag = 2 WHERE culling_flag <> 1 AND id IN (
                       SELECT m.file_id FROM group_members m
                       JOIN files f ON f.id = m.file_id JOIN folders fo ON fo.id = f.folder_id
                       WHERE m.group_id IN (SELECT id FROM temp.todo){area})"
                ),
                [],
            )?;
        }
        (0, all)
    } else {
        let (k, r) = (count(1)?, count(0)?);
        if !dry_run {
            tx.execute(
                "UPDATE files SET culling_flag = 1 WHERE id IN (
                   SELECT file_id FROM group_members
                   WHERE group_id IN (SELECT id FROM temp.todo) AND is_best = 1)",
                [],
            )?;
            tx.execute(
                "UPDATE files SET culling_flag = 2 WHERE culling_flag <> 1 AND id IN (
                   SELECT file_id FROM group_members
                   WHERE group_id IN (SELECT id FROM temp.todo) AND is_best = 0)",
                [],
            )?;
        }
        (k, r)
    };
    if !dry_run {
        // 잡동사니에서 정착 구역 구성원을 건너뛴 무리는 «확정»이 아니라 «보류» — 건너뛴 사진이
        // 조용히 사라지지 않게. 나머지는 확정
        if kind == 1 && skip_settled {
            tx.execute(
                "UPDATE groups SET state = CASE WHEN EXISTS (
                     SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                     JOIN folders fo ON fo.id = f.folder_id
                     WHERE m.group_id = groups.id AND fo.area IN (1, 2)) THEN 2 ELSE 1 END
                 WHERE id IN (SELECT id FROM temp.todo)",
                [],
            )?;
        } else {
            tx.execute("UPDATE groups SET state = 1 WHERE id IN (SELECT id FROM temp.todo)", [])?;
        }
    }
    tx.execute_batch("DROP TABLE temp.todo;")?;
    // 잡동사니의 skipped 는 «건너뛴 사진 수», 나머지 갈래는 «건너뛴 무리 수»
    let groups = if kind == 1 { total } else { total - skipped };
    Ok(ApplyAll { groups, kept, rejected, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cull::dedup;
    use crate::db::conn::Db;
    use crate::scan::scan_test;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// a/ 에 사본 둘, b/ 에 원본 하나(정착 구역). `b_twice`면 b/ 에도 사본 하나 더.
    fn setup(b_twice: bool) -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let same = b"SAME CONTENT ".repeat(100);
        std::fs::write(a.join("20200101_120000.jpg"), &same).unwrap();
        std::fs::write(a.join("copy.jpg"), &same).unwrap();
        std::fs::write(b.join("20200101_120001.jpg"), &same).unwrap();
        if b_twice {
            std::fs::write(b.join("20200101_120002.jpg"), &same).unwrap();
        }
        std::fs::write(a.join("alone.jpg"), b"unique").unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 1, |_| {}).unwrap();
        // a 는 작업대(0), b 는 공용(2, 정착 구역). scan_test 는 전부 같은 구역으로 넣는다.
        db.write(|c| {
            c.execute("UPDATE folders SET area = 0", [])?;
            c.execute("UPDATE folders SET area = 2 WHERE rel_path LIKE '%b'", [])
        })
        .unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        (dir, db)
    }

    #[test]
    fn applies_groups_whose_copies_are_outside_settled_areas() {
        let (_d, db) = setup(false);
        let dry = db.transaction(|tx| apply_all(tx, 0, true, true, None, None)).unwrap();
        assert_eq!(dry, ApplyAll { groups: 1, kept: 1, rejected: 2, skipped: 0 });
        // dry_run 은 아무것도 바꾸지 않는다
        let flagged: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files WHERE culling_flag <> 0", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(flagged, 0);

        let real = db.transaction(|tx| apply_all(tx, 0, true, false, None, None)).unwrap();
        assert_eq!(real, dry);
        let (kept, rejected): (i64, i64) = db
            .read(|c| {
                c.query_row(
                    "SELECT SUM(culling_flag = 1), SUM(culling_flag = 2) FROM files",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!((kept, rejected), (1, 2));
        // 두 번째는 할 것이 없다
        let again = db.transaction(|tx| apply_all(tx, 0, true, true, None, None)).unwrap();
        assert_eq!(again.groups, 0);
    }

    /// 사람이 보고 누른 확정은 정착 구역의 사본도, 폴더 비교가 붙인 «남김»도 넘어선다
    #[test]
    fn explicit_apply_rejects_settled_and_kept_copies_too() {
        let (_d, db) = setup(true);
        // 한꺼번에는 건너뛰는 무리 — b/ 에 제외될 사본이 있다
        let dry = db.transaction(|tx| apply_all(tx, 0, true, true, None, None)).unwrap();
        assert_eq!(dry.skipped, 1);
        // 폴더 비교가 남긴 쪽에 붙이듯 전부 «남김»으로
        db.write(|c| c.execute("UPDATE files SET culling_flag = 1", [])).unwrap();
        let gid: i64 = db.read(|c| c.query_row("SELECT id FROM groups", [], |r| r.get(0))).unwrap();
        let (kept, rejected) = db.transaction(|tx| apply_groups(tx, &[gid, 9_999])).unwrap();
        assert_eq!((kept, rejected), (1, 3), "대표 하나만 남고 셋은 제외 — 없는 무리는 건너뛴다");
        let (b_rejected, state): (i64, i64) = db
            .read(|c| {
                c.query_row(
                    "SELECT (SELECT COUNT(*) FROM files f JOIN folders fo ON fo.id = f.folder_id
                              WHERE fo.area = 2 AND f.culling_flag = 2),
                            (SELECT state FROM groups WHERE id = ?1)",
                    [gid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert!(b_rejected >= 1, "정착 구역 사본도 제외됐다");
        assert_eq!(state, 1);
    }

    #[test]
    fn skips_groups_with_a_settled_copy_to_reject() {
        let (_d, db) = setup(true);
        let r = db.transaction(|tx| apply_all(tx, 0, true, true, None, None)).unwrap();
        assert_eq!(r, ApplyAll { groups: 0, kept: 0, rejected: 0, skipped: 1 });
        // 건너뛰지 않으면 확정된다 — 대표는 b 의 이른 것, 나머지 셋은 제외
        let r = db.transaction(|tx| apply_all(tx, 0, false, true, None, None)).unwrap();
        assert_eq!(r, ApplyAll { groups: 1, kept: 1, rejected: 3, skipped: 0 });
    }


    #[test]
    fn never_demotes_a_kept_file() {
        let (_d, db) = setup(false);
        // a 의 사본 하나가 다른 무리의 대표라 이미 «남김»이다
        db.write(|c| c.execute("UPDATE files SET culling_flag = 1 WHERE name = 'copy.jpg'", []))
            .unwrap();
        let r = db.transaction(|tx| apply_all(tx, 0, true, false, None, None)).unwrap();
        assert_eq!(r.groups, 1);
        let flag: i32 = db
            .read(|c| c.query_row("SELECT culling_flag FROM files WHERE name = 'copy.jpg'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(flag, 1, "남김은 어느 갈래에서도 제외로 내리지 않는다");
    }
}
