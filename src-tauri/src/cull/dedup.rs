//! 완전 중복 찾기 — 바이트가 같은 파일들.
//!
//! 1차 구역 18,049개를 실측했을 때 완전 중복이 1,599개(15.1GB)였다. 가족 사진을
//! 여러 경로로 받거나, 구글포토 누락본을 다시 내려받거나 하면 이렇게 쌓인다.
//!
//! 판정은 [`super::hash`]의 3단계를 따른다. 크기로 후보를 좁히고, 빠른 해시로
//! 다시 좁힌 다음, 전체 해시가 같은 것만 한 그룹으로 묶는다.

use crate::cull::hash;
use crate::db::conn::{Db, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DedupProgress {
    /// 크기가 겹쳐 확인이 필요한 파일 수
    pub candidates: usize,
    pub hashed: usize,
    pub groups: usize,
    /// 중복을 정리하면 확보되는 용량
    pub reclaimable: i64,
}

struct Cand {
    id: i64,
    /// 볼륨 기준 상대경로. 실제 경로는 그 볼륨의 마운트를 앞에 붙여 만든다.
    path: PathBuf,
    volume_uuid: String,
    size: i64,
}

/// 완전 중복을 찾아 `groups`/`group_members`에 기록한다.
///
/// 남길 한 장은 정하지 않는다 — 사용자가 고른다. 다만 자동 선정을 돕도록
/// 가장 이른 촬영일을 가진 것에 `is_best`를 표시한다.
/// 등록한 라이브러리 **전부**를 가로질러 찾는다.
///
/// 볼륨을 하나로 제한하지 않는 이유: 옛 백업 디스크와 운영 디스크 사이의
/// 중복이야말로 가장 크게 확보된다. 대신 파일마다 자기 볼륨의 마운트를 찾아
/// 경로를 푼다 — 디스크가 빠져 있으면 그 파일은 그냥 건너뛴다.
pub fn scan(
    db: &Db,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&DedupProgress) + Sync + Send,
) -> Result<DedupProgress> {
    // 1단계: 크기가 겹치는 것만 후보로. 유일한 크기는 볼 것도 없다.
    let cands: Vec<Cand> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.rel_path, fi.name, fi.size, fo.volume_uuid
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.size > 0
               AND fi.size IN (SELECT size FROM files GROUP BY size HAVING COUNT(*) > 1)",
        )?;
        let it = st.query_map([], |r| {
            let dir: String = r.get(1)?;
            let name: String = r.get(2)?;
            Ok(Cand {
                id: r.get(0)?,
                path: PathBuf::from(crate::media::cache::rel_path(&dir, &name)),
                volume_uuid: r.get(4)?,
                size: r.get(3)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    // 볼륨마다 마운트를 한 번만 찾는다. 파일마다 찾으면 수만 번 syscall이다.
    let mounts: HashMap<String, PathBuf> = cands
        .iter()
        .map(|c| c.volume_uuid.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .filter_map(|u| crate::db::volumes::find_mount(&u).map(|m| (u, m)))
        .collect();
    let full_path = |c: &Cand| mounts.get(&c.volume_uuid).map(|m| m.join(&c.path));

    let progress = Arc::new(Mutex::new(DedupProgress {
        candidates: cands.len(),
        ..Default::default()
    }));
    on_progress(&progress.lock().unwrap().clone());
    if cands.is_empty() {
        return Ok(progress.lock().unwrap().clone());
    }

    // 2단계: 빠른 해시 (파일당 128KB만 읽는다)
    let counter = AtomicUsize::new(0);
    let quick: Vec<(i64, i64, String)> = cands
        .par_iter()
        .filter_map(|c| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let q = hash::quick(full_path(c)?).ok()?;
            let n = counter.fetch_add(1, Ordering::Relaxed);
            if n % 500 == 0 {
                let mut p = progress.lock().unwrap();
                p.hashed = n;
                on_progress(&p.clone());
            }
            Some((c.id, c.size, q))
        })
        .collect();

    // (크기, 빠른해시)가 같은 것끼리 묶는다. 혼자면 중복이 아니다.
    let mut buckets: HashMap<(i64, String), Vec<i64>> = HashMap::new();
    for (id, size, q) in quick {
        buckets.entry((size, q)).or_default().push(id);
    }
    let need_full: Vec<i64> = buckets
        .values()
        .filter(|v| v.len() > 1)
        .flatten()
        .copied()
        .collect();

    // 3단계: 전체 해시 — 여기까지 오는 건 극소수다
    let by_id: HashMap<i64, &Cand> = cands.iter().map(|c| (c.id, c)).collect();
    let full_hashes: Vec<(i64, String)> = need_full
        .par_iter()
        .filter_map(|id| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let c = by_id.get(id)?;
            let h = hash::full(full_path(c)?).ok()?;
            Some((*id, h))
        })
        .collect();

    // DB에 해시를 남겨 다음 스캔에서 다시 읽지 않게 한다.
    db.transaction(|tx| {
        let mut up = tx.prepare("UPDATE files SET full_hash=?1 WHERE id=?2")?;
        for (id, h) in &full_hashes {
            up.execute(rusqlite::params![h, id])?;
        }
        Ok(())
    })?;

    // 전체 해시가 같은 것끼리 그룹
    let mut final_groups: HashMap<String, Vec<i64>> = HashMap::new();
    for (id, h) in full_hashes {
        final_groups.entry(h).or_default().push(id);
    }
    let dupes: Vec<Vec<i64>> = final_groups.into_values().filter(|v| v.len() > 1).collect();

    let mut reclaimable = 0i64;
    db.transaction(|tx| {
        // 이전 결과를 지운다 — 같은 종류를 두 번 쌓지 않게
        tx.execute("DELETE FROM groups WHERE kind = 0", [])?;
        let mut ins_g = tx.prepare(
            "INSERT INTO groups(kind, reason, size_bytes, state, created_at)
             VALUES(0, '완전 중복', ?1, 0, strftime('%s','now'))",
        )?;
        let mut ins_m = tx.prepare(
            "INSERT INTO group_members(group_id, file_id, is_best) VALUES(?1,?2,?3)",
        )?;
        for ids in &dupes {
            let size: i64 = by_id.get(&ids[0]).map(|c| c.size).unwrap_or(0);
            // 한 장만 남기므로 (개수-1)만큼 확보된다
            let gain = size * (ids.len() as i64 - 1);
            reclaimable += gain;
            ins_g.execute([gain])?;
            let gid = tx.last_insert_rowid();

            // 가장 이른 촬영일을 기본 유지본으로 제안한다.
            // 원본이 사본보다 먼저 찍혔을 가능성이 높다.
            let mut best = ids[0];
            // 정리된 자리(내사진·공용)에 있는 사본이 먼저다 — 옛 백업 디스크와
            // 운영 디스크 사이 중복에서 «올라간 쪽을 남기고 옛것을 버린다»가 되게.
            // 같은 자리끼리면 가장 이른 촬영일.
            let mut best_key = (i32::MAX, i64::MAX);
            for id in ids {
                let (area, t): (i32, i64) = tx
                    .query_row(
                        "SELECT fo.area, fi.taken_at FROM files fi JOIN folders fo ON fo.id = fi.folder_id WHERE fi.id = ?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap_or((i32::MAX, i64::MAX));
                let key = (if area == 1 || area == 2 { 0 } else { 1 }, t);
                if key < best_key {
                    best_key = key;
                    best = *id;
                }
            }
            for id in ids {
                ins_m.execute(rusqlite::params![gid, id, (*id == best) as i32])?;
            }
        }
        Ok(())
    })?;

    let mut p = progress.lock().unwrap();
    p.hashed = cands.len();
    p.groups = dupes.len();
    p.reclaimable = reclaimable;
    let out = p.clone();
    drop(p);
    on_progress(&out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_test;

    /// 같은 내용의 파일을 여러 개 만들어 스캔한다.
    fn setup() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        // 같은 내용 3개 (경로·이름은 다르다)
        for (d, n) in [(&a, "20200101_120000.jpg"), (&b, "20200101_120001.jpg"), (&a, "copy.jpg")] {
            std::fs::write(d.join(n), b"SAME CONTENT ".repeat(100)).unwrap();
        }
        // 크기는 같지만 내용이 다른 것 — 그룹에 들어가면 안 된다
        std::fs::write(a.join("other.jpg"), b"DIFF CONTENT ".repeat(100)).unwrap();
        // 혼자인 것
        std::fs::write(b.join("alone.jpg"), b"unique").unwrap();

        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 1, |_| {}).unwrap();
        (dir, db)
    }

    #[test]
    fn groups_only_byte_identical_files() {
        let (_d, db) = setup();
        let p = scan(&db, Arc::new(AtomicBool::new(false)), |_| {})
            .unwrap();
        assert_eq!(p.groups, 1, "같은 내용 3개가 한 그룹");

        let members: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM group_members", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(members, 3);
    }

    #[test]
    fn same_size_different_content_is_not_a_duplicate() {
        let (_d, db) = setup();
        scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        // "other.jpg"는 크기가 같아 후보였지만 그룹에 없어야 한다
        let in_group: i64 = db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM group_members gm
                     JOIN files f ON f.id = gm.file_id WHERE f.name='other.jpg'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(in_group, 0, "크기만 같은 것은 중복이 아니다");
    }

    #[test]
    fn reclaimable_counts_all_but_one() {
        let (_d, db) = setup();
        let p = scan(&db, Arc::new(AtomicBool::new(false)), |_| {})
            .unwrap();
        let one: i64 = db
            .read(|c| {
                c.query_row("SELECT size FROM files WHERE name='copy.jpg'", [], |r| r.get(0))
            })
            .unwrap();
        assert_eq!(p.reclaimable, one * 2, "3개 중 2개분만 확보된다");
    }

    #[test]
    fn exactly_one_best_per_group() {
        let (_d, db) = setup();
        scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let bests: i64 = db
            .read(|c| {
                c.query_row("SELECT SUM(is_best) FROM group_members", [], |r| r.get(0))
            })
            .unwrap();
        assert_eq!(bests, 1, "그룹마다 유지 후보는 하나");
    }

    /// 옛 백업(작업대)과 내사진에 같은 파일이 있으면, 촬영일이 늦어도 내사진 쪽이 유지본
    #[test]
    fn settled_copy_wins_over_an_earlier_shot_in_the_desk() {
        let (_d, db) = setup();
        db.transaction(|tx| {
            tx.execute("UPDATE folders SET area = 0 WHERE name = 'a'", [])?;
            tx.execute("UPDATE folders SET area = 1 WHERE name = 'b'", [])?;
            Ok(())
        })
        .unwrap();
        scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let best_name: String = db
            .read(|c| {
                c.query_row(
                    "SELECT f.name FROM group_members m JOIN files f ON f.id = m.file_id WHERE m.is_best = 1",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(best_name, "20200101_120001.jpg", "내사진(b)의 사본이 남는다");
    }

    #[test]
    fn full_hash_is_saved_for_reuse() {
        let (_d, db) = setup();
        scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let hashed: i64 = db
            .read(|c| {
                c.query_row("SELECT COUNT(*) FROM files WHERE full_hash IS NOT NULL", [], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert!(hashed >= 3, "다음 스캔에서 다시 읽지 않도록 저장한다");
    }

    #[test]
    fn rerunning_does_not_duplicate_groups() {
        let (_d, db) = setup();
        let cancel = Arc::new(AtomicBool::new(false));
        scan(&db, cancel.clone(), |_| {}).unwrap();
        scan(&db, cancel, |_| {}).unwrap();
        let groups: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM groups WHERE kind=0", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(groups, 1, "다시 돌려도 그룹이 쌓이지 않는다");
    }

    #[test]
    fn cancellation_stops_early() {
        let (_d, db) = setup();
        let p = scan(&db, Arc::new(AtomicBool::new(true)), |_| {})
            .unwrap();
        assert_eq!(p.groups, 0);
    }
}

