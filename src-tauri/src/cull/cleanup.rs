//! 정리 화면 — 폴더마다 «NAS에 이미 있음 / 없음».
//!
//! 사용자가 하려는 일은 «T7에 모아 둔 것 중 NAS(내사진·공용)에 이미 있는 건 지우고,
//! 없는 건 NAS로 옮긴다»다. 완전 중복 무리를 그대로 보여 주면 이 말이 안 보인다.
//! 여기서는 무리를 폴더 기준으로 다시 세어, 폴더 안의 사진을 셋으로 나눈다:
//! - have  — 똑같은 파일이 정착 구역(area 1·2)에 있다 → 지워도 된다
//! - inner — 똑같은 파일이 정착 구역 밖(같은 T7 등)에만 있다 → 사본은 지워도 되지만 NAS엔 없다
//! - none  — 어디에도 없다 → 옮겨야 한다
//!
//! 무리는 `groups`(kind 0, state 0 = 미결)에서 읽는다. 찾기를 다시 하면 새로 센다.

use rusqlite::{params, Connection, Transaction};
use serde::Serialize;

use super::apply::ApplyAll;

/// 미결 완전 중복 무리의 구성원 — 아래 질의들이 공통으로 쓴다
const PEND: &str = "pend AS (
    SELECT m.group_id, m.file_id, m.is_best, f.folder_id, f.size
    FROM group_members m
    JOIN groups g ON g.id = m.group_id
    JOIN files f ON f.id = m.file_id
    WHERE g.kind = 0 AND g.state = 0),
  sg AS (
    SELECT DISTINCT p.group_id FROM pend p
    JOIN folders fo ON fo.id = p.folder_id
    WHERE p.is_best = 1 AND fo.area IN (1, 2))";

fn in_lib(lib_rel: &str, rel: &str) -> String {
    rel.strip_prefix(lib_rel)
        .map(|s| s.trim_start_matches('/'))
        .unwrap_or(rel)
        .to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupFolder {
    pub folder_id: i64,
    /// 라이브러리 기준 경로
    pub folder: String,
    pub total: i64,
    pub have: i64,
    pub have_bytes: i64,
    pub inner: i64,
    pub inner_bytes: i64,
    /// 사본들의 원본이 가장 많이 있는 폴더 — «→ 공용/2025 호주여행»
    pub keeper_library: Option<String>,
    pub keeper_folder: Option<String>,
    pub keeper_copies: i64,
}

/// 라이브러리의 폴더들 — 지워도 되는 사본이 있는 것만, 비는 용량 큰 순.
pub fn folders(c: &Connection, library_id: i64) -> rusqlite::Result<Vec<CleanupFolder>> {
    let sql = format!(
        "WITH {PEND},
         cat AS (
           SELECT p.group_id, p.folder_id, p.size,
                  CASE WHEN p.is_best = 0 AND p.group_id IN (SELECT group_id FROM sg) THEN 1
                       WHEN p.is_best = 0 THEN 2 ELSE 0 END c
           FROM pend p),
         agg AS (
           SELECT folder_id,
                  SUM(c = 1) have, SUM(CASE WHEN c = 1 THEN size ELSE 0 END) hb,
                  SUM(c = 2) inr, SUM(CASE WHEN c = 2 THEN size ELSE 0 END) ib
           FROM cat WHERE c > 0 GROUP BY folder_id),
         pairs AS (
           SELECT p.folder_id src, k.folder_id dst, COUNT(*) n
           FROM cat p JOIN pend k ON k.group_id = p.group_id AND k.is_best = 1
           WHERE p.c > 0 GROUP BY p.folder_id, k.folder_id),
         top AS (SELECT src, MAX(n) n, dst FROM pairs GROUP BY src)
         SELECT fo.id, l.rel_path, fo.rel_path,
                (SELECT COUNT(*) FROM files x WHERE x.folder_id = fo.id AND x.trashed_at IS NULL),
                a.have, a.hb, a.inr, a.ib,
                kl.name, kl.rel_path, kfo.rel_path, COALESCE(t.n, 0)
         FROM agg a
         JOIN folders fo ON fo.id = a.folder_id
         JOIN libraries l ON l.id = fo.library_id
         LEFT JOIN top t ON t.src = a.folder_id
         LEFT JOIN folders kfo ON kfo.id = t.dst
         LEFT JOIN libraries kl ON kl.id = kfo.library_id
         WHERE fo.library_id = ?1 AND fo.area NOT IN (1, 2)
         ORDER BY a.hb + a.ib DESC"
    );
    let mut st = c.prepare(&sql)?;
    let rows = st.query_map([library_id], |r| {
        let lib_rel: String = r.get(1)?;
        let rel: String = r.get(2)?;
        let k_lib_rel: Option<String> = r.get(9)?;
        let k_rel: Option<String> = r.get(10)?;
        Ok(CleanupFolder {
            folder_id: r.get(0)?,
            folder: in_lib(&lib_rel, &rel),
            total: r.get(3)?,
            have: r.get(4)?,
            have_bytes: r.get(5)?,
            inner: r.get(6)?,
            inner_bytes: r.get(7)?,
            keeper_library: r.get(8)?,
            keeper_folder: match (k_lib_rel, k_rel) {
                (Some(l), Some(f)) => Some(in_lib(&l, &f)),
                _ => None,
            },
            keeper_copies: r.get(11)?,
        })
    })?;
    rows.collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupFile {
    pub file_id: i64,
    pub name: String,
    pub size: i64,
    pub kind: i32,
    pub library_id: Option<i64>,
    pub thumb: Option<String>,
    /// "have" · "inner" · "none"
    pub cat: &'static str,
    /// 원본이 있는 곳 — «공용 · 2025/2025 호주여행»
    pub keeper: Option<String>,
}

/// 폴더의 사진 전부를 셋으로 나눠 준다. 촬영 순.
pub fn files(c: &Connection, folder_id: i64) -> rusqlite::Result<Vec<CleanupFile>> {
    let sql = format!(
        "WITH {PEND},
         mine AS (SELECT p.group_id, p.file_id, p.is_best FROM pend p WHERE p.folder_id = ?1)
         SELECT f.id, f.name, f.size, f.kind, t.rel_path, fo.library_id,
                CASE WHEN m.file_id IS NULL OR m.is_best = 1 THEN 0
                     WHEN m.group_id IN (SELECT group_id FROM sg) THEN 1 ELSE 2 END cat,
                kl.name, kl.rel_path, kfo.rel_path
         FROM files f
         JOIN folders fo ON fo.id = f.folder_id
         LEFT JOIN thumbs t ON t.file_id = f.id AND t.state = 1
         LEFT JOIN mine m ON m.file_id = f.id
         LEFT JOIN pend k ON k.group_id = m.group_id AND k.is_best = 1 AND m.is_best = 0
         LEFT JOIN folders kfo ON kfo.id = k.folder_id
         LEFT JOIN libraries kl ON kl.id = kfo.library_id
         WHERE f.folder_id = ?1 AND f.trashed_at IS NULL
         ORDER BY cat DESC, f.taken_at, f.id"
    );
    let mut st = c.prepare(&sql)?;
    let rows = st.query_map([folder_id], |r| {
        let cat: i32 = r.get(6)?;
        let k_lib: Option<String> = r.get(7)?;
        let k_lib_rel: Option<String> = r.get(8)?;
        let k_rel: Option<String> = r.get(9)?;
        let keeper = match (k_lib, k_lib_rel, k_rel) {
            (Some(l), Some(lr), Some(f)) if cat > 0 => {
                let p = in_lib(&lr, &f);
                Some(if p.is_empty() { l } else { format!("{l} · {p}") })
            }
            _ => None,
        };
        Ok(CleanupFile {
            file_id: r.get(0)?,
            name: r.get(1)?,
            size: r.get(2)?,
            kind: r.get(3)?,
            thumb: r.get(4)?,
            library_id: r.get(5)?,
            cat: match cat {
                1 => "have",
                2 => "inner",
                _ => "none",
            },
            keeper,
        })
    })?;
    rows.collect()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CleanupSummary {
    pub have: i64,
    pub have_bytes: i64,
    pub inner: i64,
    pub inner_bytes: i64,
    /// 정착 구역 안에서 겹치는 무리 수와 그 사본 장수 — 사람이 하나씩 본다
    pub settled_groups: i64,
    pub settled_files: i64,
}

/// 라이브러리 하나의 «지워도 되는» 합계와, 정착 구역 안 겹침의 크기.
pub fn summary(c: &Connection, library_id: i64) -> rusqlite::Result<CleanupSummary> {
    let sql = format!(
        "WITH {PEND}
         SELECT
           SUM(p.is_best = 0 AND p.group_id IN (SELECT group_id FROM sg)),
           SUM(CASE WHEN p.is_best = 0 AND p.group_id IN (SELECT group_id FROM sg) THEN p.size ELSE 0 END),
           SUM(p.is_best = 0 AND p.group_id NOT IN (SELECT group_id FROM sg)),
           SUM(CASE WHEN p.is_best = 0 AND p.group_id NOT IN (SELECT group_id FROM sg) THEN p.size ELSE 0 END)
         FROM pend p JOIN folders fo ON fo.id = p.folder_id
         WHERE fo.library_id = ?1 AND fo.area NOT IN (1, 2)"
    );
    let (have, have_bytes, inner, inner_bytes): (Option<i64>, Option<i64>, Option<i64>, Option<i64>) =
        c.query_row(&sql, [library_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    let sql2 = format!(
        "WITH {PEND}
         SELECT COUNT(DISTINCT p.group_id), COUNT(*)
         FROM pend p JOIN folders fo ON fo.id = p.folder_id
         WHERE p.is_best = 0 AND fo.area IN (1, 2)"
    );
    let (settled_groups, settled_files): (i64, i64) =
        c.query_row(&sql2, [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(CleanupSummary {
        have: have.unwrap_or(0),
        have_bytes: have_bytes.unwrap_or(0),
        inner: inner.unwrap_or(0),
        inner_bytes: inner_bytes.unwrap_or(0),
        settled_groups,
        settled_files,
    })
}

/// 두 폴더 사이의 미결 무리를 한꺼번에 — `keep` 폴더 것을 남기고 `drop` 폴더 것에
/// 지우기 표시. 두 폴더 밖에도 구성원이 있는 무리는 건드리지 않는다.
pub fn apply_pair(tx: &Transaction, keep: i64, drop: i64) -> rusqlite::Result<ApplyAll> {
    tx.execute_batch(
        "DROP TABLE IF EXISTS temp.todo; CREATE TEMP TABLE todo(id INTEGER PRIMARY KEY);",
    )?;
    tx.execute(
        "INSERT INTO temp.todo
         SELECT g.id FROM groups g
         WHERE g.kind = 0 AND g.state = 0
           AND EXISTS (SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                       WHERE m.group_id = g.id AND f.folder_id = ?1)
           AND EXISTS (SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                       WHERE m.group_id = g.id AND f.folder_id = ?2)
           AND NOT EXISTS (SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                           WHERE m.group_id = g.id AND f.folder_id NOT IN (?1, ?2))",
        params![keep, drop],
    )?;
    let groups = tx.query_row("SELECT COUNT(*) FROM temp.todo", [], |r| r.get::<_, i64>(0))? as usize;
    tx.execute(
        "UPDATE group_members SET is_best = CASE WHEN file_id IN (SELECT id FROM files WHERE folder_id = ?1) THEN 1 ELSE 0 END
         WHERE group_id IN (SELECT id FROM temp.todo)",
        [keep],
    )?;
    let kept = tx.execute(
        "UPDATE files SET culling_flag = 1 WHERE id IN (
           SELECT file_id FROM group_members WHERE group_id IN (SELECT id FROM temp.todo) AND is_best = 1)",
        [],
    )?;
    let rejected = tx.execute(
        "UPDATE files SET culling_flag = 2 WHERE id IN (
           SELECT file_id FROM group_members WHERE group_id IN (SELECT id FROM temp.todo) AND is_best = 0)",
        [],
    )?;
    tx.execute("UPDATE groups SET state = 1 WHERE id IN (SELECT id FROM temp.todo)", [])?;
    tx.execute_batch("DROP TABLE temp.todo;")?;
    Ok(ApplyAll { groups, kept, rejected, skipped: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cull::dedup;
    use crate::db::conn::Db;
    use crate::scan::scan_test;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// a/ (작업대): 사본 둘 + 혼자인 것 하나 + T7끼리만 겹치는 둘.
    /// b/ (공용): 원본 하나. c/ (공용): b 와 겹치는 것 하나 — 정착 구역 안 겹침.
    fn setup() -> (tempfile::TempDir, Db, i64, i64) {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"));
        for d in [&a, &b, &c] {
            std::fs::create_dir_all(d).unwrap();
        }
        let same = b"SAME CONTENT ".repeat(100);
        let inner = b"INNER ONLY ".repeat(100);
        let pair = b"PAIR IN NAS ".repeat(100);
        std::fs::write(a.join("20200101_120000.jpg"), &same).unwrap();
        std::fs::write(a.join("copy.jpg"), &same).unwrap();
        std::fs::write(a.join("alone.jpg"), b"unique").unwrap();
        std::fs::write(a.join("20200102_120000.jpg"), &inner).unwrap();
        std::fs::write(a.join("inner-copy.jpg"), &inner).unwrap();
        std::fs::write(b.join("20200101_120001.jpg"), &same).unwrap();
        std::fs::write(b.join("20200103_120000.jpg"), &pair).unwrap();
        std::fs::write(c.join("20200103_120001.jpg"), &pair).unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 1, |_| {}).unwrap();
        db.write(|cn| {
            cn.execute("UPDATE folders SET area = 0", [])?;
            cn.execute("UPDATE folders SET area = 2 WHERE rel_path LIKE '%b' OR rel_path LIKE '%c'", [])
        })
        .unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let (lib, fa): (i64, i64) = db
            .read(|cn| {
                cn.query_row(
                    "SELECT library_id, id FROM folders WHERE rel_path LIKE '%a'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        (dir, db, lib, fa)
    }

    #[test]
    fn folder_is_split_into_have_inner_none() {
        let (_d, db, lib, fa) = setup();
        let rows = db.read(|c| folders(c, lib)).unwrap();
        let a = rows.iter().find(|r| r.folder_id == fa).expect("a 폴더");
        assert_eq!((a.total, a.have, a.inner), (5, 2, 1), "{a:?}");
        assert_eq!(a.keeper_folder.as_deref(), Some("b"));

        let fs = db.read(|c| files(c, fa)).unwrap();
        let cat = |n: &str| fs.iter().find(|f| f.name == n).map(|f| f.cat).unwrap_or("?");
        assert_eq!(cat("copy.jpg"), "have");
        assert_eq!(cat("20200101_120000.jpg"), "have");
        assert_eq!(cat("alone.jpg"), "none");
        // T7끼리만 겹치는 둘 — 이른 쪽이 원본(none), 나머지가 inner
        assert_eq!(cat("20200102_120000.jpg"), "none");
        assert_eq!(cat("inner-copy.jpg"), "inner");
        assert!(fs.iter().find(|f| f.name == "copy.jpg").unwrap().keeper.as_deref().unwrap().ends_with("b"));
    }

    #[test]
    fn summary_counts_settled_pairs_separately() {
        let (_d, db, lib, _fa) = setup();
        let s = db.read(|c| summary(c, lib)).unwrap();
        assert_eq!((s.have, s.inner), (2, 1));
        assert_eq!((s.settled_groups, s.settled_files), (1, 1), "b·c 사이 한 무리");
    }

    #[test]
    fn pair_apply_keeps_one_folder_and_marks_the_other() {
        let (_d, db, _lib, _fa) = setup();
        let (fb, fc): (i64, i64) = db
            .read(|c| {
                Ok((
                    c.query_row("SELECT id FROM folders WHERE rel_path LIKE '%b'", [], |r| r.get(0))?,
                    c.query_row("SELECT id FROM folders WHERE rel_path LIKE '%c'", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        // c 를 남기고 b 것에 표시 — 대표가 b 였어도 뒤집힌다
        let r = db.transaction(|tx| apply_pair(tx, fc, fb)).unwrap();
        assert_eq!((r.groups, r.kept, r.rejected), (1, 1, 1));
        let flag: i32 = db
            .read(|c| c.query_row("SELECT culling_flag FROM files WHERE name = '20200103_120000.jpg'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(flag, 2, "b 의 것이 지우기 표시");
        assert_eq!(db.read(|c| summary(c, 1)).unwrap().settled_groups, 0);
    }
}
