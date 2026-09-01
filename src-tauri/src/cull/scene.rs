//! 비슷한 장면 — AI 벡터로 닮은 사진을 묶는다.
//!
//! «같은 순간»은 시계만 본다 — 10초 안에 연달아 찍은 것. 이건 그림을 본다:
//! 한 시간 안에 찍은 사진 가운데 CLIP 벡터가 가까운 것을 잇는다. 자리를
//! 옮겨 가며 다시 찍은 것, 몇 분 뒤 돌아와 또 찍은 것이 여기 모인다.
//! 완전 중복(바이트)과 같은 순간(시계)이 이미 한 그룹에 넣은 짝은 잇지
//! 않는다 — 같은 그룹을 세 탭에서 세 번 보게 하지 않기 위해서다.
//!
//! 묶음은 **씨앗 기준**이다 — 가장 이른 사진을 씨앗으로, 그와 직접 닮은 것만
//! 넣는다. 사슬로 잇지 않는다: A~B, B~C라고 A와 C를 한 묶음에 넣으면 한
//! 시간짜리 잔치가 통째로 한 그룹이 된다 (실측: 976장짜리 그룹, 사진의 84%).
//!
//! 전수 비교는 안 한다. 14만 장이면 짝이 100억이다. 시간순으로 늘어놓고
//! 한 시간 안, 앞뒤 200장까지만 잰다 — 같은 장면은 같은 시간대에 있다.
//! 다른 날 다시 찍은 닮은 장면은 못 잡는다. 그건 «비슷한 사진 찾기»가 한다.

use crate::ai::clip;
use crate::db::conn::{Db, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const KIND: i32 = 3;
/// 코사인 유사도 문턱. 14만 장 실측(2026-08-28, 같은 순간 뺀 뒤):
/// 0.93 → 29.8천 그룹 9.1만 장, 0.95 → 32.2천 그룹 8.6만 장, 0.97 → 34.5천 그룹 8.2만 장.
/// 문턱은 사진 수를 거의 안 바꾸고 그룹의 굵기만 바꾼다 — 높일수록 큰 묶음이
/// 잘게 쪼개진다. 0.95는 관련된 컷을 한 묶음에 두면서 다른 장면은 안 섞이는 자리.
pub const DEFAULT_THRESHOLD: f32 = 0.95;
/// 이 시간 안에 찍은 것끼리만 잰다
pub const WINDOW_SECS: i64 = 3600;
/// 시간순 이웃 최대 — 한 시간에 천 장을 찍어도 셈이 터지지 않게
pub const MAX_NEIGHBORS: usize = 200;
const MIN_GROUP: usize = 2;
type Pair = (usize, usize, f32);
type PairBatch = (Vec<Pair>, usize);

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SceneProgress {
    /// 벡터가 있는 사진 수
    pub photos: usize,
    /// 잰 짝의 수
    pub compared: usize,
    pub groups: usize,
    pub members: usize,
    pub reclaimable: i64,
}

struct Row {
    id: i64,
    taken_at: i64,
    size: i64,
    sharpness: Option<f64>,
    /// 폴더의 영역 — 내사진·공용에 있는 것이 대표가 된다
    area: i32,
}

/// 임베딩 전부를 한 덩어리에 — 장마다 Vec 을 두면 22.8만 장에 900MB 를 넘겼다 (리뷰 H9).
/// i 번째 사진의 벡터는 `vecs[i*DIM..(i+1)*DIM]`.
struct Loaded {
    rows: Vec<Row>,
    vecs: Vec<f32>,
    /// 벡터 길이 — 첫 행이 정한다. 길이가 다른 것(옛 모델)은 견줄 수 없어 뺀다
    dim: usize,
}

impl Loaded {
    fn v(&self, i: usize) -> &[f32] {
        &self.vecs[i * self.dim..(i + 1) * self.dim]
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// 다른 탭이 이미 한 그룹에 넣은 파일들 — 파일 → 그룹들
fn taken(db: &Db) -> Result<HashMap<i64, Vec<i64>>> {
    let rows: Vec<(i64, i64)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT m.file_id, m.group_id FROM group_members m
             JOIN groups g ON g.id = m.group_id
             WHERE g.kind IN (0, 2, 4) AND g.state = 0",
        )?;
        let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let mut m: HashMap<i64, Vec<i64>> = HashMap::new();
    for (f, g) in rows {
        m.entry(f).or_default().push(g);
    }
    Ok(m)
}

fn share_group(taken: &HashMap<i64, Vec<i64>>, a: i64, b: i64) -> bool {
    match (taken.get(&a), taken.get(&b)) {
        (Some(x), Some(y)) => x.iter().any(|g| y.contains(g)),
        _ => false,
    }
}

fn load(db: &Db) -> Result<Loaded> {
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fi.taken_at, fi.size, fi.sharpness, fi.embedding, fo.area
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.embedding IS NOT NULL AND fi.kind <> 1 AND fi.trashed_at IS NULL
             ORDER BY fi.taken_at, fi.id",
        )?;
        let mut rows = Vec::new();
        let mut vecs: Vec<f32> = Vec::new();
        let mut dim = 0usize;
        let mut q = st.query([])?;
        while let Some(r) = q.next()? {
            let blob: Vec<u8> = r.get(4)?;
            let v = clip::from_blob(&blob);
            if dim == 0 {
                dim = v.len();
            }
            if v.len() != dim || dim == 0 {
                continue; // 길이가 다른 벡터(옛 모델)는 견줄 수 없다
            }
            rows.push(Row {
                id: r.get(0)?,
                taken_at: r.get(1)?,
                size: r.get(2)?,
                sharpness: r.get(3)?,
                area: r.get(5)?,
            });
            vecs.extend_from_slice(&v);
        }
        Ok(Loaded { rows, vecs, dim })
    })
}

/// 시간순 이웃 가운데 문턱을 넘는 짝. (앞, 뒤, 닮음)
fn pairs(
    loaded: &Loaded,
    threshold: f32,
    taken: &HashMap<i64, Vec<i64>>,
    cancel: &AtomicBool,
) -> (Vec<Pair>, usize) {
    let rows = &loaded.rows;
    let out: Vec<PairBatch> = rows
        .par_iter()
        .enumerate()
        .map(|(i, a)| {
            let mut hits = Vec::new();
            let mut n = 0usize;
            if cancel.load(Ordering::Relaxed) {
                return (hits, n);
            }
            for (j, b) in rows.iter().enumerate().skip(i + 1).take(MAX_NEIGHBORS) {
                if b.taken_at - a.taken_at > WINDOW_SECS {
                    break;
                }
                if share_group(taken, a.id, b.id) {
                    continue;
                }
                n += 1;
                let s = dot(loaded.v(i), loaded.v(j));
                if s >= threshold {
                    hits.push((i, j, s));
                }
            }
            (hits, n)
        })
        .collect();
    let compared = out.iter().map(|(_, n)| n).sum();
    (out.into_iter().flat_map(|(h, _)| h).collect(), compared)
}

pub fn scan(
    db: &Db,
    threshold: f32,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&SceneProgress) + Sync + Send,
) -> Result<SceneProgress> {
    let loaded = load(db)?;
    let rows = &loaded.rows;
    let mut progress = SceneProgress { photos: rows.len(), ..Default::default() };
    on_progress(&progress);
    if cancel.load(Ordering::Relaxed) {
        return Ok(progress);
    }
    if rows.len() < MIN_GROUP {
        db.transaction(|tx| {
            tx.execute(
                "DELETE FROM group_members WHERE group_id IN (SELECT id FROM groups WHERE kind = ?1)",
                [KIND],
            )?;
            tx.execute("DELETE FROM groups WHERE kind = ?1", [KIND])?;
            Ok(())
        })?;
        return Ok(progress);
    }
    let taken = taken(db)?;
    let (links, compared) = pairs(&loaded, threshold, &taken, &cancel);
    progress.compared = compared;
    if cancel.load(Ordering::Relaxed) {
        return Ok(progress);
    }

    // 씨앗 기준 묶기. links는 (앞, 뒤) 순으로 정렬돼 있다 — 앞이 씨앗.
    let mut assigned = vec![false; rows.len()];
    let mut groups: Vec<(Vec<usize>, f32)> = Vec::new();
    let mut k = 0;
    while k < links.len() {
        let seed = links[k].0;
        let mut members = vec![seed];
        let mut weakest = 1.0f32;
        while k < links.len() && links[k].0 == seed {
            let (_, j, s) = links[k];
            k += 1;
            if assigned[seed] || assigned[j] {
                continue;
            }
            members.push(j);
            weakest = weakest.min(s);
        }
        if members.len() >= MIN_GROUP {
            for &i in &members {
                assigned[i] = true;
            }
            groups.push((members, weakest));
        }
    }

    let mut reclaimable = 0i64;
    let mut n_members = 0usize;
    db.transaction(|tx| {
        tx.execute(
            "DELETE FROM group_members WHERE group_id IN (SELECT id FROM groups WHERE kind = ?1)",
            [KIND],
        )?;
        tx.execute("DELETE FROM groups WHERE kind = ?1", [KIND])?;
        let mut ins_g = tx.prepare(
            "INSERT INTO groups(kind, reason, size_bytes, state, created_at)
             VALUES(?1, ?2, ?3, 0, strftime('%s','now'))",
        )?;
        let mut ins_m = tx.prepare(
            "INSERT INTO group_members(group_id, file_id, is_best, score) VALUES(?1,?2,?3,?4)",
        )?;
        for (m, w) in &groups {
            // 정리된 자리(내사진·공용)에 있는 것, 그다음 가장 또렷한 것, 같으면 큰 것
            let settled = |r: &Row| i32::from(r.area == 1 || r.area == 2);
            let best = *m
                .iter()
                .max_by(|&&a, &&b| {
                    let (ra, rb) = (&rows[a], &rows[b]);
                    settled(ra)
                        .cmp(&settled(rb))
                        .then(ra.sharpness.unwrap_or(0.0).total_cmp(&rb.sharpness.unwrap_or(0.0)))
                        .then(ra.size.cmp(&rb.size))
                })
                .unwrap();
            let total: i64 = m.iter().map(|&i| rows[i].size).sum();
            let saved = total - rows[best].size;
            reclaimable += saved;
            n_members += m.len();
            ins_g.execute(rusqlite::params![
                KIND,
                format!("닮음 {:.0}%", w * 100.0),
                saved
            ])?;
            let gid = tx.last_insert_rowid();
            for &i in m {
                ins_m.execute(rusqlite::params![gid, rows[i].id, i == best, rows[i].sharpness])?;
            }
        }
        Ok(())
    })?;

    progress.groups = groups.len();
    progress.members = n_members;
    progress.reclaimable = reclaimable;
    on_progress(&progress);
    Ok(progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::conn::Db;

    fn unit(v: &[f32]) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }
    /// 첫 축과 코사인이 `cos`인 단위 벡터
    fn near(cos: f32) -> Vec<f32> {
        unit(&[cos, (1.0 - cos * cos).sqrt(), 0.0, 0.0])
    }

    /// (id, taken_at, size, sharpness, 벡터). 폴더 하나.
    type SeedItem = (i64, i64, i64, Option<f64>, Vec<f32>);

    fn seed(db: &Db, items: &[SeedItem]) {
        db.transaction(|tx| {
            tx.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])?;
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','f','f',1)",
                [],
            )?;
            for (id, taken, size, sharp, v) in items {
                tx.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,
                        sharpness,scanned_at,embedding)
                     VALUES(?1,1,?2,?3,0,?4,0,?5,0,?6)",
                    rusqlite::params![id, format!("f{id}.jpg"), size, taken, sharp, clip::to_blob(v)],
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

    fn run(db: &Db) -> SceneProgress {
        scan(db, DEFAULT_THRESHOLD, Arc::new(AtomicBool::new(false)), |_| {}).unwrap()
    }

    fn members_of(db: &Db) -> Vec<(i64, bool)> {
        db.read(|c| {
            let mut st = c.prepare(
                "SELECT m.file_id, m.is_best FROM group_members m
                 JOIN groups g ON g.id = m.group_id WHERE g.kind = 3 ORDER BY m.file_id",
            )?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            it.collect()
        })
        .unwrap()
    }

    #[test]
    fn links_alike_photos_within_the_hour_and_keeps_the_sharpest() {
        let (_d, db) = db();
        seed(&db, &[
            (1, 1000, 100, Some(0.2), near(1.0)),
            (2, 1300, 100, Some(0.9), near(0.97)),
            (3, 1600, 100, None, near(0.0)), // 다른 장면
        ]);
        let p = run(&db);
        assert_eq!((p.groups, p.members), (1, 2));
        assert_eq!(members_of(&db), vec![(1, false), (2, true)]);
        assert_eq!(p.reclaimable, 100);
    }

    #[test]
    fn does_not_link_across_hours() {
        let (_d, db) = db();
        seed(&db, &[(1, 1000, 100, None, near(1.0)), (2, 1000 + WINDOW_SECS + 1, 100, None, near(1.0))]);
        assert_eq!(run(&db).groups, 0);
    }

    #[test]
    fn respects_the_threshold() {
        let (_d, db) = db();
        seed(&db, &[(1, 1000, 100, None, near(1.0)), (2, 1100, 100, None, near(0.8))]);
        assert_eq!(run(&db).groups, 0);
        let p = scan(&db, 0.75, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        assert_eq!(p.groups, 1);
    }

    /// «같은 순간» 그룹을 심는다
    fn burst_group(db: &Db, gid: i64, files: &[i64]) {
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO groups(id, kind, reason, size_bytes, state, created_at)
                 VALUES(?1, 2, NULL, 0, 0, 0)",
                [gid],
            )?;
            for (i, f) in files.iter().enumerate() {
                tx.execute(
                    "INSERT INTO group_members(group_id,file_id,is_best,score) VALUES(?1,?2,?3,NULL)",
                    rusqlite::params![gid, f, i == 0],
                )?;
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn skips_pairs_another_tab_already_grouped() {
        let (_d, db) = db();
        seed(&db, &[(1, 1000, 100, None, near(1.0)), (2, 1003, 100, None, near(1.0))]);
        burst_group(&db, 9, &[1, 2]);
        assert_eq!(run(&db).groups, 0);
    }

    /// 1·2는 «같은 순간»이 이미 묶었다. 뒤에 온 3은 씨앗 1과만 묶인다.
    #[test]
    fn a_later_alike_shot_pairs_with_one_burst_member_not_all() {
        let (_d, db) = db();
        seed(&db, &[
            (1, 1000, 100, None, near(1.0)),
            (2, 1003, 100, None, near(1.0)),
            (3, 1100, 100, None, near(0.98)),
        ]);
        burst_group(&db, 9, &[1, 2]);
        let p = run(&db);
        assert_eq!((p.groups, p.members), (1, 2));
        assert_eq!(members_of(&db).iter().map(|m| m.0).collect::<Vec<_>>(), vec![1, 3]);
    }

    /// A~B, B~C라도 A와 C가 안 닮았으면 한 묶음이 아니다 — 씨앗과 직접 닮아야 한다
    #[test]
    fn does_not_chain_through_a_middle_photo() {
        let (_d, db) = db();
        // 1과 2는 닮고(0.93), 2와 3도 닮지만(0.93), 1과 3은 안 닮았다(약 0.73)
        let a = unit(&[1.0, 0.0, 0.0, 0.0]);
        let b = unit(&[0.93, (1.0f32 - 0.93 * 0.93).sqrt(), 0.0, 0.0]);
        let c = unit(&[0.73, 0.68, 0.0, 0.0]);
        seed(&db, &[(1, 1000, 100, None, a), (2, 1100, 100, None, b), (3, 1200, 100, None, c)]);
        let p = scan(&db, 0.90, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        assert_eq!((p.groups, p.members), (1, 2));
        assert_eq!(members_of(&db).iter().map(|m| m.0).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn rerunning_replaces_old_groups() {
        let (_d, db) = db();
        seed(&db, &[(1, 1000, 100, None, near(1.0)), (2, 1100, 100, None, near(0.99))]);
        run(&db);
        let p = run(&db);
        assert_eq!(p.groups, 1);
        assert_eq!(members_of(&db).len(), 2);
    }

    #[test]
    fn rerunning_with_fewer_than_two_photos_clears_old_groups() {
        let (_d, db) = db();
        seed(&db, &[(1, 1000, 100, None, near(1.0)), (2, 1100, 100, None, near(0.99))]);
        assert_eq!(run(&db).groups, 1);
        db.write(|c| c.execute("UPDATE files SET trashed_at=1 WHERE id=2", []))
            .unwrap();
        let p = run(&db);
        assert_eq!((p.groups, p.members), (0, 0));
        assert!(members_of(&db).is_empty(), "대상이 한 장뿐인데 이전 그룹이 남았다");
    }

    #[test]
    fn reason_names_the_weakest_link() {
        let (_d, db) = db();
        seed(&db, &[(1, 1000, 100, None, near(1.0)), (2, 1100, 100, None, near(0.97))]);
        run(&db);
        let reason: String = db
            .read(|c| c.query_row("SELECT reason FROM groups WHERE kind = 3", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(reason, "닮음 97%");
    }

    /// 실제 라이브러리 사본으로 — 시간과 그룹 수를 본다.
    /// `ACUT_DB_COPY=/path/copy.db cargo test --release --lib cull::scene::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 DB 사본 필요"]
    fn real_library_copy() {
        let Ok(path) = std::env::var("ACUT_DB_COPY") else { return };
        let db = Db::open(path).unwrap();
        let thr: f32 = std::env::var("ACUT_SCENE_THRESHOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_THRESHOLD);
        // 실제 흐름대로 — «같은 순간»이 먼저 묶고, 그 짝은 여기서 빠진다
        let b = crate::cull::burst::scan(&db, crate::cull::burst::DEFAULT_GAP_SECS).unwrap();
        eprint!("같은 순간 {}그룹 {}장 · ", b.groups, b.photos);
        let t = std::time::Instant::now();
        let p = scan(&db, thr, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        eprint!("문턱 {thr} ");
        eprintln!(
            "\n[비슷한 장면] {}장 · 짝 {} · {}그룹 · {}장 · 확보 {:.1} GB · {:.1}초",
            p.photos, p.compared, p.groups, p.members,
            p.reclaimable as f64 / 1024f64.powi(3), t.elapsed().as_secs_f64()
        );
        let sizes: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT COUNT(*) FROM group_members m JOIN groups g ON g.id = m.group_id
                     WHERE g.kind = 3 GROUP BY g.id ORDER BY 1 DESC LIMIT 5",
                )?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect()
            })
            .unwrap();
        let pairs: i64 = db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM (SELECT g.id FROM group_members m JOIN groups g ON g.id = m.group_id
                     WHERE g.kind = 3 GROUP BY g.id HAVING COUNT(*) = 2)",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        eprintln!("가장 큰 그룹들: {sizes:?} · 둘짜리 {pairs}");
    }
}
