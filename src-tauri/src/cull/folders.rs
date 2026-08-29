//! 폴더 비교 — 내용이 완전히 같은 폴더들.
//!
//! 사용자의 말: «후보1번에도 있고 후보2번에도 있고 공용에도 있으면 셋을 한꺼번에
//! 보여 주고, 둘이면 둘. 폴더가 완전히 같은데 사진은 뭐하러 보여 주나.»
//!
//! 폴더의 «서명» = 바로 아래 파일들의 전체 해시를 정렬해 이어 붙인 것. 서명이 같은
//! 폴더끼리 한 묶음이다. 파일 하나라도 해시가 없으면(크기가 유일해 후보조차 아니었던
//! 파일) 그 폴더는 어디와도 같을 수 없으니 뺀다. 하위 폴더가 있는 폴더는 묶지 않는다 —
//! 하위는 저마다 따로 비교한다.

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
         WHERE t.nohash = 0 AND t.n >= 1
           -- 하위 폴더가 있으면 «바로 아래 파일만 같다»일 뿐이다 — 사용자는 폴더째 같다고
           -- 읽고 Finder 에서 지운다. 하위는 저마다 따로 견준다 (리뷰 H5)
           AND NOT EXISTS (SELECT 1 FROM folders c WHERE c.parent_id = fo.id)",
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
    if drops.contains(&keep) || drops.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let kept = tx.execute(
        "UPDATE files SET culling_flag = 1 WHERE folder_id = ?1 AND trashed_at IS NULL",
        [keep],
    )?;
    let mut rejected = 0;
    for d in drops {
        rejected += tx.execute(
            "UPDATE files SET culling_flag = 2
             WHERE folder_id = ?1 AND trashed_at IS NULL AND culling_flag <> 1",
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


#[derive(Debug, Clone, Serialize)]
pub struct PairRow {
    /// A 뿌리 아래 폴더 (뿌리 기준 경로). 없으면 B 에만 있는 폴더
    pub a: Option<FolderIn>,
    pub b: Option<FolderIn>,
    pub files_a: i64,
    pub files_b: i64,
    /// 바로 아래 파일이 전부 같다
    pub same: bool,
    /// 이름이 같은 폴더끼리, 양쪽에 똑같이 있는 파일 수
    pub common: i64,
    /// 같은 쪽 하나를 지우면 비는 용량(same 일 때) — 아니면 공통 파일의 용량
    pub bytes: i64,
}

struct Agg {
    info: FolderIn,
    /// 뿌리 기준 상대경로 — 이름이 같은 폴더를 찾는 열쇠
    sub: String,
    files: i64,
    bytes: i64,
    hashes: Vec<String>,
    all_hashed: bool,
    has_children: bool,
}

fn folders_under(c: &Connection, root_id: i64) -> rusqlite::Result<Vec<Agg>> {
    let (vol, rel): (String, String) = c.query_row(
        "SELECT volume_uuid, rel_path FROM folders WHERE id = ?1",
        [root_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let esc = crate::db::query::escape_like(&rel);
    let mut st = c.prepare(
        "SELECT fo.id, fo.rel_path, fo.area, l.id, l.name, l.rel_path, f.full_hash, f.size,
                EXISTS (SELECT 1 FROM folders k WHERE k.parent_id = fo.id)
         FROM folders fo
         JOIN libraries l ON l.id = fo.library_id
         LEFT JOIN files f ON f.folder_id = fo.id AND f.trashed_at IS NULL
         WHERE fo.volume_uuid = ?1 AND (fo.rel_path = ?2 OR fo.rel_path LIKE ?3 || '/%' ESCAPE '\\')
         ORDER BY fo.rel_path, f.full_hash",
    )?;
    let mut out: Vec<Agg> = Vec::new();
    let rows = st.query_map(params![vol, rel, esc], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i32>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, bool>(8)?,
        ))
    })?;
    for row in rows {
        let (id, fo_rel, area, lib_id, lib_name, lib_rel, hash, size, kids) = row?;
        if out.last().map(|a| a.info.folder_id) != Some(id) {
            let sub = fo_rel
                .strip_prefix(&rel)
                .map(|s| s.trim_start_matches('/').to_string())
                .unwrap_or_else(|| fo_rel.clone());
            out.push(Agg {
                info: FolderIn {
                    folder_id: id,
                    library_id: lib_id,
                    library: lib_name,
                    folder: in_lib(&lib_rel, &fo_rel),
                    area,
                },
                sub,
                files: 0,
                bytes: 0,
                hashes: Vec::new(),
                all_hashed: true,
                has_children: kids,
            });
        }
        let cur = out.last_mut().unwrap();
        if let Some(size) = size {
            cur.files += 1;
            cur.bytes += size;
            match hash {
                Some(h) => cur.hashes.push(h),
                None => cur.all_hashed = false,
            }
        }
    }
    Ok(out)
}

/// 두 폴더(와 그 아래)를 견준다 — «후보1번/연도별»과 «후보2번»처럼.
///
/// 내용이 완전히 같은 폴더끼리 짝(same), 이름이 같은데 내용이 다른 폴더끼리는 공통
/// 파일 수와 함께 짝(partial), 한쪽에만 있는 폴더는 홀로. 같은 쪽 큰 것부터.
pub fn compare_two(c: &Connection, a_root: i64, b_root: i64) -> rusqlite::Result<Vec<PairRow>> {
    let a = folders_under(c, a_root)?;
    let b = folders_under(c, b_root)?;
    // B 쪽 서명·이름 색인
    let sig = |g: &Agg| -> Option<String> {
        (g.files > 0 && g.all_hashed && !g.has_children).then(|| g.hashes.join(","))
    };
    let mut b_by_sig: HashMap<String, Vec<usize>> = HashMap::new();
    let mut b_by_sub: HashMap<String, usize> = HashMap::new();
    for (i, g) in b.iter().enumerate() {
        if let Some(s) = sig(g) {
            b_by_sig.entry(s).or_default().push(i);
        }
        b_by_sub.entry(g.sub.clone()).or_insert(i);
    }
    let mut used_b = vec![false; b.len()];
    let mut out = Vec::new();
    for ga in &a {
        if ga.files == 0 && !ga.has_children {
            continue; // 빈 폴더
        }
        // 1) 내용이 같은 B 폴더 — 이름까지 같은 것을 먼저
        if let Some(s) = sig(ga) {
            if let Some(cands) = b_by_sig.get(&s) {
                let pick = cands
                    .iter()
                    .copied()
                    .filter(|&i| !used_b[i])
                    .min_by_key(|&i| (b[i].sub != ga.sub, i));
                if let Some(i) = pick {
                    used_b[i] = true;
                    out.push(PairRow {
                        a: Some(ga.info.clone()),
                        b: Some(b[i].info.clone()),
                        files_a: ga.files,
                        files_b: b[i].files,
                        same: true,
                        common: ga.files,
                        bytes: ga.bytes,
                    });
                    continue;
                }
            }
        }
        // 2) 이름이 같은 B 폴더 — 공통 파일 수
        if let Some(&i) = b_by_sub.get(&ga.sub) {
            if !used_b[i] {
                used_b[i] = true;
                let gb = &b[i];
                let mut counts: HashMap<&str, i64> = HashMap::new();
                for h in &gb.hashes {
                    *counts.entry(h.as_str()).or_default() += 1;
                }
                let mut common = 0i64;
                let mut bytes = 0i64;
                let per = if ga.files > 0 { ga.bytes / ga.files } else { 0 };
                for h in &ga.hashes {
                    if let Some(n) = counts.get_mut(h.as_str()) {
                        if *n > 0 {
                            *n -= 1;
                            common += 1;
                            bytes += per;
                        }
                    }
                }
                out.push(PairRow {
                    a: Some(ga.info.clone()),
                    b: Some(gb.info.clone()),
                    files_a: ga.files,
                    files_b: gb.files,
                    same: false,
                    common,
                    bytes,
                });
                continue;
            }
        }
        // 3) A 에만
        out.push(PairRow {
            a: Some(ga.info.clone()),
            b: None,
            files_a: ga.files,
            files_b: 0,
            same: false,
            common: 0,
            bytes: 0,
        });
    }
    for (i, gb) in b.iter().enumerate() {
        if used_b[i] || (gb.files == 0 && !gb.has_children) {
            continue;
        }
        out.push(PairRow {
            a: None,
            b: Some(gb.info.clone()),
            files_a: 0,
            files_b: gb.files,
            same: false,
            common: 0,
            bytes: 0,
        });
    }
    // 같은 것 → 부분 → 한쪽만, 각각 큰 순
    out.sort_by(|x, y| {
        let rank = |r: &PairRow| if r.same { 0 } else if r.a.is_some() && r.b.is_some() { 1 } else { 2 };
        rank(x).cmp(&rank(y)).then(y.bytes.cmp(&x.bytes)).then(y.files_a.cmp(&x.files_a))
    });
    Ok(out)
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

    #[test]
    fn two_roots_pair_identical_and_partial_folders() {
        let (_d, db) = setup();
        let id_of = |name: &str| -> i64 {
            db.read(|c| c.query_row("SELECT id FROM folders WHERE rel_path LIKE ?1", [format!("%{name}")], |r| r.get(0)))
                .unwrap()
        };
        let (a, b, d) = (id_of("a"), id_of("b"), id_of("d"));
        // a 와 b 는 내용이 같다 — 뿌리끼리도 짝이 된다
        let rows = db.read(|c| compare_two(c, a, b)).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].same && rows[0].common == 2);
        // a 와 d 는 한 장만 겹친다 — 뿌리 이름은 다르지만 뿌리끼리는 sub 가 같다("")
        let rows = db.read(|c| compare_two(c, a, d)).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(!rows[0].same);
        assert_eq!((rows[0].common, rows[0].files_a, rows[0].files_b), (1, 2, 2));
    }
}
