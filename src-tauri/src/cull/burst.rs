//! 같은 순간 묶기 — 연달아 찍은 사진에서 한 장을 고른다.
//!
//! 1차 구역 18,049개 실측: 같은 '분'에 3장 이상 찍은 것이 6,497장, 1,418번의 순간.
//! 각 순간에서 한두 장만 남기면 4,000장 가까이 정리된다. 폰 사진에서 가장 큰 몫이다.
//!
//! DSLR 연사만이 아니다. 폰으로도 같은 장면을 여러 번 누른다. 그래서 간격 기준을
//! 초 단위로 두되 넉넉하게 잡는다(기본 10초).
//!
//! **같은 폴더 안에서만 묶는다.** 서로 다른 이벤트의 사진이 시각만 가깝다고 묶이면
//! 사용자가 혼란스럽다.

use crate::db::conn::{Db, Result};

/// 이 간격 안에 찍힌 사진들을 한 순간으로 본다.
pub const DEFAULT_GAP_SECS: i64 = 10;
/// 이 장수 이상일 때만 그룹으로 만든다. 두 장은 굳이 고를 것도 없다.
pub const MIN_GROUP: usize = 3;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BurstProgress {
    pub groups: usize,
    pub photos: usize,
    /// 각 그룹에서 한 장만 남긴다고 할 때 확보되는 용량
    pub reclaimable: i64,
}

struct Row {
    id: i64,
    folder_id: i64,
    taken_at: i64,
    size: i64,
    sharpness: Option<f64>,
}

/// 같은 순간끼리 묶어 `groups`(kind=2)에 기록한다.
///
/// 대표는 선명도가 가장 높은 것. 선명도가 아직 없으면 용량이 가장 큰 것을 쓴다
/// (같은 장면이라면 대체로 디테일이 많은 쪽이 크다).
pub fn scan(db: &Db, volume_uuid: &str, gap_secs: i64) -> Result<BurstProgress> {
    let rows: Vec<Row> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fi.folder_id, fi.taken_at, fi.size, fi.sharpness
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fo.volume_uuid = ?1 AND fi.kind <> 1
             ORDER BY fi.folder_id, fi.taken_at, fi.id",
        )?;
        let it = st.query_map([volume_uuid], |r| {
            Ok(Row {
                id: r.get(0)?,
                folder_id: r.get(1)?,
                taken_at: r.get(2)?,
                size: r.get(3)?,
                sharpness: r.get(4)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    // 정렬돼 있으므로 한 번 훑으며 끊는다.
    let mut groups: Vec<Vec<&Row>> = Vec::new();
    let mut cur: Vec<&Row> = Vec::new();
    for r in &rows {
        match cur.last() {
            Some(prev)
                if prev.folder_id == r.folder_id && r.taken_at - prev.taken_at <= gap_secs =>
            {
                cur.push(r)
            }
            _ => {
                if cur.len() >= MIN_GROUP {
                    groups.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                cur.push(r);
            }
        }
    }
    if cur.len() >= MIN_GROUP {
        groups.push(cur);
    }

    let mut reclaimable = 0i64;
    let mut photos = 0usize;

    db.transaction(|tx| {
        tx.execute("DELETE FROM groups WHERE kind = 2", [])?;
        let mut ins_g = tx.prepare(
            "INSERT INTO groups(kind, reason, size_bytes, state, created_at)
             VALUES(2, ?1, ?2, 0, strftime('%s','now'))",
        )?;
        let mut ins_m = tx.prepare(
            "INSERT INTO group_members(group_id, file_id, is_best, score) VALUES(?1,?2,?3,?4)",
        )?;

        for g in &groups {
            photos += g.len();
            // 한 장만 남긴다고 가정 — 나머지 용량이 확보분
            let total: i64 = g.iter().map(|r| r.size).sum();
            let keep = g.iter().map(|r| r.size).max().unwrap_or(0);
            let gain = total - keep;
            reclaimable += gain;

            let span = g.last().unwrap().taken_at - g[0].taken_at;
            let reason = format!("{}장 · {}초 안에", g.len(), span.max(1));
            ins_g.execute(rusqlite::params![reason, gain])?;
            let gid = tx.last_insert_rowid();

            // 대표 고르기 — 선명도가 있으면 그것, 없으면 용량
            let best = g
                .iter()
                .max_by(|a, b| {
                    match (a.sharpness, b.sharpness) {
                        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (None, None) => a.size.cmp(&b.size),
                    }
                })
                .map(|r| r.id)
                .unwrap_or(g[0].id);

            for r in g {
                ins_m.execute(rusqlite::params![gid, r.id, (r.id == best) as i32, r.sharpness])?;
            }
        }
        Ok(())
    })?;

    Ok(BurstProgress { groups: groups.len(), photos, reclaimable })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::conn::Db;

    /// taken_at을 지정해 파일을 심는다 (파일시스템을 거치지 않는다).
    fn seed(db: &Db, items: &[(i64, i64, i64, i64, Option<f64>)]) {
        // (id, folder_id, taken_at, size, sharpness)
        db.transaction(|tx| {
            tx.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])?;
            for fid in [1i64, 2] {
                tx.execute(
                    "INSERT INTO folders(id,volume_uuid,rel_path,name,area)
                     VALUES(?1,'V',?2,?2,1)",
                    rusqlite::params![fid, format!("f{fid}")],
                )?;
            }
            for (id, folder, taken, size, sharp) in items {
                tx.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,
                        sharpness,scanned_at)
                     VALUES(?1,?2,?3,?4,0,?5,0,?6,0)",
                    rusqlite::params![id, folder, format!("f{id}.jpg"), size, taken, sharp],
                )?;
            }
            Ok(())
        })
        .unwrap();
    }

    fn db() -> (tempfile::TempDir, Db) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(d.path().join("t.db")).unwrap();
        (d, db)
    }

    #[test]
    fn groups_photos_taken_close_together() {
        let (_d, db) = db();
        seed(&db, &[
            (1, 1, 1000, 100, None),
            (2, 1, 1003, 100, None),
            (3, 1, 1005, 100, None),
            // 한참 뒤 — 다른 순간
            (4, 1, 5000, 100, None),
        ]);
        let p = scan(&db, "V", DEFAULT_GAP_SECS).unwrap();
        assert_eq!(p.groups, 1);
        assert_eq!(p.photos, 3);
    }

    #[test]
    fn two_photos_are_not_a_group() {
        let (_d, db) = db();
        seed(&db, &[(1, 1, 1000, 100, None), (2, 1, 1002, 100, None)]);
        let p = scan(&db, "V", DEFAULT_GAP_SECS).unwrap();
        assert_eq!(p.groups, 0, "두 장은 고를 것도 없다");
    }

    #[test]
    fn does_not_cross_folders() {
        let (_d, db) = db();
        // 시각은 가깝지만 폴더가 다르다
        seed(&db, &[
            (1, 1, 1000, 100, None),
            (2, 1, 1001, 100, None),
            (3, 2, 1002, 100, None),
            (4, 2, 1003, 100, None),
        ]);
        let p = scan(&db, "V", DEFAULT_GAP_SECS).unwrap();
        assert_eq!(p.groups, 0, "폴더가 다르면 묶지 않는다");
    }

    #[test]
    fn a_long_sequence_stays_one_group() {
        let (_d, db) = db();
        // 3초 간격으로 10장 — 전체 27초지만 연속이므로 한 그룹
        let items: Vec<_> = (1..=10).map(|i| (i, 1i64, 1000 + i * 3, 100i64, None)).collect();
        seed(&db, &items);
        let p = scan(&db, "V", DEFAULT_GAP_SECS).unwrap();
        assert_eq!(p.groups, 1);
        assert_eq!(p.photos, 10);
    }

    #[test]
    fn sharpest_is_picked_as_best() {
        let (_d, db) = db();
        seed(&db, &[
            (1, 1, 1000, 100, Some(10.0)),
            (2, 1, 1002, 100, Some(90.0)), // 가장 선명
            (3, 1, 1004, 100, Some(50.0)),
        ]);
        scan(&db, "V", DEFAULT_GAP_SECS).unwrap();
        let best: i64 = db
            .read(|c| {
                c.query_row("SELECT file_id FROM group_members WHERE is_best=1", [], |r| r.get(0))
            })
            .unwrap();
        assert_eq!(best, 2);
    }

    #[test]
    fn falls_back_to_size_without_sharpness() {
        let (_d, db) = db();
        seed(&db, &[
            (1, 1, 1000, 100, None),
            (2, 1, 1002, 900, None), // 가장 큼
            (3, 1, 1004, 300, None),
        ]);
        scan(&db, "V", DEFAULT_GAP_SECS).unwrap();
        let best: i64 = db
            .read(|c| {
                c.query_row("SELECT file_id FROM group_members WHERE is_best=1", [], |r| r.get(0))
            })
            .unwrap();
        assert_eq!(best, 2, "선명도가 없으면 용량이 큰 쪽");
    }

    #[test]
    fn reclaimable_keeps_the_largest() {
        let (_d, db) = db();
        seed(&db, &[
            (1, 1, 1000, 100, None),
            (2, 1, 1002, 500, None),
            (3, 1, 1004, 300, None),
        ]);
        let p = scan(&db, "V", DEFAULT_GAP_SECS).unwrap();
        assert_eq!(p.reclaimable, 400, "900 - 500(남길 것)");
    }

    #[test]
    fn gap_setting_changes_grouping() {
        let (_d, db) = db();
        seed(&db, &[
            (1, 1, 1000, 100, None),
            (2, 1, 1020, 100, None),
            (3, 1, 1040, 100, None),
        ]);
        // 간격 10초면 따로따로
        assert_eq!(scan(&db, "V", 10).unwrap().groups, 0);
        // 30초면 한 그룹
        assert_eq!(scan(&db, "V", 30).unwrap().groups, 1);
    }
}
