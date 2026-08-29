//! 폴더 비교 — 내용이 완전히 같은 폴더들.
//!
//! 사용자의 말: «후보1번에도 있고 후보2번에도 있고 공용에도 있으면 셋을 한꺼번에
//! 보여 주고, 둘이면 둘. 폴더가 완전히 같은데 사진은 뭐하러 보여 주나.»
//!
//! 폴더의 «서명» = 바로 아래 파일들의 전체 해시를 정렬해 이어 붙인 것. 서명이 같은
//! 폴더끼리 한 묶음이다. 파일 하나라도 해시가 없으면(크기가 유일해 후보조차 아니었던
//! 파일) 그 폴더는 어디와도 같을 수 없으니 뺀다. 하위 폴더는 저마다 따로 비교한다.

use rusqlite::{params, Connection, Transaction};
use serde::Serialize;
use std::collections::HashMap;

use super::apply::ApplyAll;

#[derive(Debug, Clone, Serialize)]
pub struct FolderIn {
    pub folder_id: i64,
    pub library_id: i64,
    pub library: String,
    /// 라이브러리 기준 경로
    pub folder: String,
    pub area: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderSet {
    /// 같은 내용의 폴더들 — 정착 구역이 앞에
    pub folders: Vec<FolderIn>,
    pub files: i64,
    /// 폴더 하나의 용량 — 하나만 남기면 (n-1)배가 빈다
    pub bytes: i64,
    /// 이 묶음의 파일 중 지우기 표시가 아직 없는 것이 있나
    pub pending: bool,
}

fn in_lib(lib_rel: &str, rel: &str) -> String {
    rel.strip_prefix(lib_rel)
        .map(|s| s.trim_start_matches('/'))
        .unwrap_or(rel)
        .to_string()
}

/// 내용이 완전히 같은 폴더 묶음들 — 비는 용량 큰 순.
pub fn identical_sets(c: &Connection, limit: usize) -> rusqlite::Result<Vec<FolderSet>> {
    // 파일이 전부 해시된 폴더만 서명을 만든다. group_concat 의 순서는 안쪽 ORDER BY 를 따른다.
    let mut st = c.prepare(
        "WITH tot AS (
           SELECT folder_id, COUNT(*) n, SUM(full_hash IS NULL) nohash, SUM(size) bytes,
                  SUM(culling_flag = 0) pend
           FROM files WHERE trashed_at IS NULL GROUP BY folder_id),
         sig AS (
           SELECT folder_id, group_concat(full_hash, ',') s
           FROM (SELECT folder_id, full_hash FROM files
                 WHERE trashed_at IS NULL AND full_hash IS NOT NULL
                 ORDER BY folder_id, full_hash)
           GROUP BY folder_id)
         SELECT fo.id, fo.library_id, l.name, l.rel_path, fo.rel_path, fo.area,
                t.n, t.bytes, t.pend, sig.s
         FROM sig
         JOIN tot t ON t.folder_id = sig.folder_id
         JOIN folders fo ON fo.id = sig.folder_id
         JOIN libraries l ON l.id = fo.library_id
         WHERE t.nohash = 0 AND t.n >= 1",
    )?;
    let rows = st.query_map([], |r| {
        let lib_rel: String = r.get(3)?;
        let rel: String = r.get(4)?;
        Ok((
            FolderIn {
                folder_id: r.get(0)?,
                library_id: r.get(1)?,
                library: r.get(2)?,
                folder: in_lib(&lib_rel, &rel),
                area: r.get(5)?,
            },
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, String>(9)?,
        ))
    })?;
    let mut by_sig: HashMap<String, (Vec<FolderIn>, i64, i64, bool)> = HashMap::new();
    for row in rows {
        let (f, n, bytes, pend, s) = row?;
        let e = by_sig.entry(s).or_insert((Vec::new(), n, bytes, false));
        e.0.push(f);
        e.3 |= pend > 0;
    }
    let mut out: Vec<FolderSet> = by_sig
        .into_values()
        .filter(|(fs, _, _, _)| fs.len() >= 2)
        .map(|(mut fs, files, bytes, pending)| {
            // 정착 구역이 앞에, 그다음은 라이브러리·경로 순 — 남길 것이 맨 앞
            fs.sort_by(|a, b| {
                let sa = !(a.area == 1 || a.area == 2);
                let sb = !(b.area == 1 || b.area == 2);
                sa.cmp(&sb)
                    .then(a.library_id.cmp(&b.library_id))
                    .then(a.folder.cmp(&b.folder))
            });
            FolderSet { folders: fs, files, bytes, pending }
        })
        .collect();
    out.sort_by(|a, b| {
        let ga = a.bytes * (a.folders.len() as i64 - 1);
        let gb = b.bytes * (b.folders.len() as i64 - 1);
        gb.cmp(&ga)
    });
    out.truncate(limit);
    Ok(out)
}

/// 폴더 묶음 하나를 처리한다 — `keep` 폴더의 파일은 남김, `drops` 폴더의 파일은 지우기 표시.
/// 이 폴더들 안에서만 얽힌 완전 중복 무리는 확정으로 돌려 개별 비교에 다시 안 나오게.
pub fn apply_set(tx: &Transaction, keep: i64, drops: &[i64]) -> rusqlite::Result<ApplyAll> {
    let kept = tx.execute(
        "UPDATE files SET culling_flag = 1 WHERE folder_id = ?1 AND trashed_at IS NULL",
        [keep],
    )?;
    let mut rejected = 0;
    for d in drops {
        rejected += tx.execute(
            "UPDATE files SET culling_flag = 2 WHERE folder_id = ?1 AND trashed_at IS NULL",
            [d],
        )?;
    }
    let all: Vec<i64> = std::iter::once(keep).chain(drops.iter().copied()).collect();
    let list = all.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let groups = tx.execute(
        &format!(
            "UPDATE groups SET state = 1 WHERE kind = 0 AND state = 0
               AND id IN (SELECT m.group_id FROM group_members m JOIN files f ON f.id = m.file_id
                          WHERE f.folder_id IN ({list}))
               AND NOT EXISTS (SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                               WHERE m.group_id = groups.id AND f.folder_id NOT IN ({list}))"
        ),
        params![],
    )?;
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

    /// a/, b/, c/ 는 내용이 같은 폴더(파일 이름은 달라도 된다). d/ 는 한 장이 다르다.
    fn setup() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let x = b"X ".repeat(500);
        let y = b"Y ".repeat(700);
        for (d, names) in [("a", ["1.jpg", "2.jpg"]), ("b", ["one.jpg", "two.jpg"]), ("c", ["1.jpg", "2.jpg"])] {
            let p = dir.path().join(d);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join(names[0]), &x).unwrap();
            std::fs::write(p.join(names[1]), &y).unwrap();
        }
        let p = dir.path().join("d");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("1.jpg"), &x).unwrap();
        std::fs::write(p.join("3.jpg"), b"Z ".repeat(700)).unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 1, |_| {}).unwrap();
        db.write(|c| {
            c.execute("UPDATE folders SET area = 0", [])?;
            c.execute("UPDATE folders SET area = 2 WHERE rel_path LIKE '%c'", [])
        })
        .unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        (dir, db)
    }

    #[test]
    fn finds_folders_with_identical_contents() {
        let (_d, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        assert_eq!(sets.len(), 1, "{sets:?}");
        let s = &sets[0];
        let names: Vec<&str> = s.folders.iter().map(|f| f.folder.as_str()).collect();
        assert_eq!(names, ["c", "a", "b"], "정착 구역(c)이 맨 앞");
        assert_eq!(s.files, 2);
        assert!(s.pending);
    }

    #[test]
    fn applying_a_set_marks_the_other_folders_and_settles_their_groups() {
        let (_d, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        let s = &sets[0];
        let keep = s.folders[0].folder_id;
        let drops: Vec<i64> = s.folders[1..].iter().map(|f| f.folder_id).collect();
        let r = db.transaction(|tx| apply_set(tx, keep, &drops)).unwrap();
        assert_eq!((r.kept, r.rejected), (2, 4));
        // d/1.jpg 는 a·b·c 밖에도 있어 그 무리는 미결로 남는다; y(2.jpg) 무리는 확정
        assert_eq!(r.groups, 1, "{r:?}");
        let pending = db.read(|c| identical_sets(c, 100)).unwrap();
        assert!(!pending[0].pending, "처리한 묶음은 pending 이 아니다");
    }
}
