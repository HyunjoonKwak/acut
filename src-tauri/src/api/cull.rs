//! 고르기 커맨드.
//!
//! 스캔은 오래 걸릴 수 있으므로(완전 중복은 파일을 읽는다) 별도 스레드에서 돌고
//! 진행 상황을 이벤트로 흘린다. 조회는 즉시 돌아온다.

use crate::api::{err, AppState};
use crate::cull::{apply, burst, dedup, folders, junk, scene};
use super::job;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// 고르기 종류. DB `groups.kind`와 같은 값이다.
pub const KIND_DUP: i32 = 0;
pub const KIND_JUNK: i32 = 1;
pub const KIND_BURST: i32 = 2;
pub const KIND_SCENE: i32 = scene::KIND;

#[derive(Debug, Serialize)]
pub struct GroupRow {
    pub id: i64,
    pub kind: i32,
    pub reason: Option<String>,
    /// 이 그룹을 정리하면 확보되는 용량
    pub size_bytes: i64,
    pub state: i32,
    pub member_count: i64,
    /// 대표 미리보기용 — 대표 파일의 썸네일 경로
    pub cover: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MemberRow {
    pub file_id: i64,
    pub name: String,
    pub size: i64,
    pub taken_at: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub is_best: bool,
    pub score: Option<f64>,
    /// 썸네일 주소를 만들려면 필요하다 — 캐시가 라이브러리마다 따로 있다
    pub library_id: Option<i64>,
    pub thumb: Option<String>,
    pub culling_flag: i32,
    /// 어디 있는 사본인가 — 라이브러리 이름과 라이브러리 기준 폴더. 어느 쪽을
    /// 남길지 고를 때 이게 없으면 판단할 근거가 없다.
    pub library: String,
    pub folder: String,
    pub folder_id: i64,
    pub area: i32,
}

/// 세 갈래를 순서대로 돌린다. 가벼운 것부터 — 결과가 빨리 보이게.
#[tauri::command]
pub async fn cull_scan(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    // 스캔·벡터와 같은 스위치 — 같이 돌면 DB와 디스크를 다툰다. 상태바에 보이고
    // 창이 뒤로 가도 App Nap에 걸리지 않는다 (해시는 한 시간도 걸린다).
    let Some(guard) = job::try_start(&state.running, "에이컷 고르기") else {
        return Err("다른 일이 도는 중입니다 — 끝난 뒤 «다시 찾기»를 눌러 주세요".into());
    };
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let _guard = guard;
        // 1. 잡동사니 — 파일을 열지 않는다. 즉시 끝난다
        match junk::scan(&db) {
            Ok(p) => {
                let _ = app.emit("cull-junk", &p);
            }
            Err(e) => {
                let _ = app.emit("cull-error", e.to_string());
                return;
            }
        }
        // 2. 같은 순간 — 역시 파일을 열지 않는다
        match burst::scan(&db, burst::DEFAULT_GAP_SECS) {
            Ok(p) => {
                let _ = app.emit("cull-burst", &p);
            }
            Err(e) => {
                let _ = app.emit("cull-error", e.to_string());
                return;
            }
        }
        // 3. 완전 중복 — 해시를 읽으므로 가장 오래 걸린다
        let r = dedup::scan(&db, Arc::clone(&cancel), |p| {
            let _ = app.emit("cull-dedup-progress", p);
        });
        match r {
            Ok(p) => {
                let _ = app.emit("cull-dedup", &p);
            }
            Err(e) => {
                let _ = app.emit("cull-error", e.to_string());
                return;
            }
        }
        // 비슷한 장면 — 벡터가 있는 사진만. 없으면 photos 0으로 곧 끝난다.
        match scene::scan(&db, scene::DEFAULT_THRESHOLD, cancel, |_| {}) {
            Ok(p) => {
                let _ = app.emit("cull-scene", &p);
                let _ = app.emit("cull-done", ());
            }
            Err(e) => {
                let _ = app.emit("cull-error", e.to_string());
            }
        }
    });
    Ok(())
}

/// 그룹 목록. 확보 용량이 큰 것부터 — 효과가 큰 것을 먼저 보게 한다.
#[tauri::command]
pub async fn cull_groups(
    state: State<'_, AppState>,
    kind: i32,
    limit: usize,
    offset: usize,
    settled: Option<bool>,
) -> Result<Vec<GroupRow>, String> {
    let limit = limit.clamp(1, 200);
    // 정착 구역(내사진·공용) 안에 제외될 사본이 있는 무리만 — 사람이 하나씩 보는 것
    let settled = settled.unwrap_or(false);
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT g.id, g.kind, g.reason, g.size_bytes, g.state,
                        (SELECT COUNT(*) FROM group_members m WHERE m.group_id = g.id),
                        (SELECT fo.library_id || '/' || t.rel_path FROM group_members m
                         JOIN files fi ON fi.id = m.file_id
                         JOIN folders fo ON fo.id = fi.folder_id
                         JOIN thumbs t ON t.file_id = m.file_id AND t.state = 1
                         WHERE m.group_id = g.id
                         ORDER BY m.is_best DESC LIMIT 1)
                 FROM groups g
                 WHERE g.kind = ?1 AND g.state = 0
                   AND (?4 = 0 OR EXISTS (
                         SELECT 1 FROM group_members m JOIN files fi ON fi.id = m.file_id
                         JOIN folders fo ON fo.id = fi.folder_id
                         WHERE m.group_id = g.id AND m.is_best = 0 AND fo.area IN (1, 2)))
                 ORDER BY g.size_bytes DESC
                 LIMIT ?2 OFFSET ?3",
            )?;
            let it = st.query_map(rusqlite::params![kind, limit as i64, offset as i64, settled as i32], |r| {
                Ok(GroupRow {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    reason: r.get(2)?,
                    size_bytes: r.get(3)?,
                    state: r.get(4)?,
                    member_count: r.get(5)?,
                    cover: r.get(6)?,
                })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

/// 그룹 하나의 구성원. 대표가 맨 앞에 온다.
#[tauri::command]
pub async fn cull_members(state: State<'_, AppState>, group_id: i64) -> Result<Vec<MemberRow>, String> {
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT m.file_id, f.name, f.size, f.taken_at, f.width, f.height,
                        m.is_best, m.score, t.rel_path, f.culling_flag, fo.library_id,
                        l.name, l.rel_path, fo.rel_path, fo.area, fo.id
                 FROM group_members m
                 JOIN files f ON f.id = m.file_id
                 JOIN folders fo ON fo.id = f.folder_id
                 LEFT JOIN libraries l ON l.id = fo.library_id
                 LEFT JOIN thumbs t ON t.file_id = f.id AND t.state = 1
                 WHERE m.group_id = ?1
                 ORDER BY m.is_best DESC, m.score DESC, f.size DESC",
            )?;
            let it = st.query_map([group_id], |r| {
                let lib_rel: Option<String> = r.get(12)?;
                let rel: String = r.get(13)?;
                let folder = match lib_rel.as_deref().and_then(|l| rel.strip_prefix(l)) {
                    Some(rest) => rest.trim_start_matches('/').to_string(),
                    None => rel.clone(),
                };
                Ok(MemberRow {
                    library: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
                    folder,
                    area: r.get(14)?,
                    folder_id: r.get(15)?,
                    file_id: r.get(0)?,
                    name: r.get(1)?,
                    size: r.get(2)?,
                    taken_at: r.get(3)?,
                    width: r.get(4)?,
                    height: r.get(5)?,
                    is_best: r.get::<_, i32>(6)? != 0,
                    score: r.get(7)?,
                    thumb: r.get(8)?,
                    culling_flag: r.get(9)?,
                    library_id: r.get(10)?,
                })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

/// 대표를 바꾼다. 한 그룹에 대표는 하나뿐이다.
#[tauri::command]
pub async fn cull_set_best(
    state: State<'_, AppState>,
    group_id: i64,
    file_id: i64,
) -> Result<(), String> {
    state
        .db
        .transaction(|tx| {
            // 다시 찾기로 무리 id 가 재사용되면 화면의 id 가 다른 무리를 가리킬 수 있다 —
            // 그 무리에 없는 사진이면 대표가 하나도 없는 무리가 되어 다음 확정이 전부를
            // 제외한다. 바꾸기 전에 있는지부터 본다 (리뷰 H6)
            let member: i64 = tx.query_row(
                "SELECT COUNT(*) FROM group_members WHERE group_id=?1 AND file_id=?2",
                rusqlite::params![group_id, file_id],
                |r| r.get(0),
            )?;
            if member == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            tx.execute("UPDATE group_members SET is_best=0 WHERE group_id=?1", [group_id])?;
            tx.execute(
                "UPDATE group_members SET is_best=1 WHERE group_id=?1 AND file_id=?2",
                rusqlite::params![group_id, file_id],
            )?;
            Ok(())
        })
        .map_err(err)?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ApplyResult {
    pub kept: usize,
    pub rejected: usize,
    /// 정착 구역(내사진·공용)에 있어 제외하지 않은 사본 수 — 거기서 지우면 NAS에서도 지워진다
    pub skipped: usize,
}

/// 그룹의 판정을 확정한다 — 대표는 '남김', 나머지는 '제외'로 표시한다.
///
/// **파일을 지우지는 않는다.** `culling_flag`만 바꾼다. 실제 삭제는 사용자가
/// 따로 확인한 뒤에 한다. 잘못 눌러도 되돌릴 수 있어야 한다.
#[tauri::command]
pub async fn cull_apply(state: State<'_, AppState>, group_ids: Vec<i64>) -> Result<ApplyResult, String> {
    if group_ids.is_empty() {
        return Ok(ApplyResult { kept: 0, rejected: 0, skipped: 0 });
    }
    let (kept, rejected, skipped) = state
        .db
        .transaction(|tx| {
            use rusqlite::OptionalExtension;
            let mut kept = 0;
            let mut rejected = 0;
            let mut skipped = 0;
            for gid in &group_ids {
                // 다시 찾기로 사라진 무리일 수 있다 — 조용히 건너뛴다
                let Some(kind) = tx
                    .query_row("SELECT kind FROM groups WHERE id=?1", [gid], |r| r.get::<_, i32>(0))
                    .optional()?
                else {
                    continue;
                };
                // 정착 구역(내사진·공용)의 사본은 제외하지 않는다 — 치우면 Drive가 NAS에서도
                // 지운다. 이미 «남김»인 파일도 내리지 않는다 (리뷰 C13·C11).
                let best = if kind == 1 { "" } else { " AND m.is_best = 0" };
                let rej = tx.execute(
                    &format!(
                        "UPDATE files SET culling_flag=2 WHERE culling_flag <> 1 AND id IN
                         (SELECT m.file_id FROM group_members m
                          JOIN files f ON f.id = m.file_id
                          JOIN folders fo ON fo.id = f.folder_id
                          WHERE m.group_id=?1{best} AND fo.area NOT IN (1, 2))"
                    ),
                    [gid],
                )?;
                let skp = tx.query_row(
                    &format!(
                        "SELECT COUNT(*) FROM group_members m
                          JOIN files f ON f.id = m.file_id
                          JOIN folders fo ON fo.id = f.folder_id
                          WHERE m.group_id=?1{best} AND fo.area IN (1, 2)"
                    ),
                    [gid],
                    |r| r.get::<_, i64>(0),
                )? as usize;
                rejected += rej;
                skipped += skp;
                if kind != 1 {
                    kept += tx.execute(
                        "UPDATE files SET culling_flag=1 WHERE id IN
                         (SELECT file_id FROM group_members WHERE group_id=?1 AND is_best=1)",
                        [gid],
                    )?;
                }
                // 건너뛴 것뿐이면 «확정»이 아니라 «보류» — 정착 구역 사본이 조용히 잊히지 않게
                let state_v = if rej == 0 && skp > 0 { 2 } else { 1 };
                tx.execute("UPDATE groups SET state=?2 WHERE id=?1", rusqlite::params![gid, state_v])?;
            }
            Ok((kept, rejected, skipped))
        })
        .map_err(err)?;
    Ok(ApplyResult { kept, rejected, skipped })
}

/// 갈래의 미결 무리를 한꺼번에 확정한다 (규칙은 cull::apply). `dry_run`이면 세기만.
#[tauri::command]
pub async fn cull_apply_all(
    state: State<'_, AppState>,
    kind: i32,
    skip_settled: bool,
    dry_run: bool,
    folder_id: Option<i64>,
    library_id: Option<i64>,
) -> Result<apply::ApplyAll, String> {
    state
        .db
        .transaction(|tx| apply::apply_all(tx, kind, skip_settled, dry_run, folder_id, library_id))
        .map_err(err)
}

/// 두 폴더 사이의 무리를 한꺼번에 — keep 것을 남기고 drop 것에 지우기 표시.
#[tauri::command]
pub async fn cull_apply_pair(
    state: State<'_, AppState>,
    keep_folder_id: i64,
    drop_folder_id: i64,
    dry_run: Option<bool>,
) -> Result<apply::ApplyAll, String> {
    if keep_folder_id == drop_folder_id {
        return Err("같은 폴더를 남기고 지울 수는 없습니다".into());
    }
    let dry = dry_run.unwrap_or(false);
    state
        .db
        .transaction(|tx| folders::apply_pair(tx, keep_folder_id, drop_folder_id, dry))
        .map_err(err)
}

/// 두 폴더 비교의 «전부» — (남길 폴더, 지울 폴더) 짝 목록을 한 트랜잭션에.
#[tauri::command]
pub async fn cull_folder_pairs_apply(
    state: State<'_, AppState>,
    pairs: Vec<(i64, i64)>,
) -> Result<folders::PairsApplied, String> {
    if pairs.is_empty() {
        return Ok(folders::PairsApplied::default());
    }
    state.db.transaction(|tx| folders::apply_pairs(tx, &pairs)).map_err(err)
}

/// 폴더 비교 — 내용이 완전히 같은 폴더 묶음들.
#[tauri::command]
pub async fn cull_folder_sets(state: State<'_, AppState>) -> Result<Vec<folders::FolderSet>, String> {
    state.db.read(|c| folders::identical_sets(c, 500)).map_err(err)
}

/// 두 폴더 비교 — A·B 아래 폴더들을 내용으로 짝짓는다.
#[tauri::command]
pub async fn cull_compare_folders(
    state: State<'_, AppState>,
    a_volume: String,
    a_rel: String,
    b_volume: String,
    b_rel: String,
) -> Result<Vec<folders::PairRow>, String> {
    if folders::roots_overlap((&a_volume, &a_rel), (&b_volume, &b_rel)) {
        return Err("두 폴더가 서로를 품고 있습니다 — 겹치지 않는 두 폴더를 고르세요".into());
    }
    state
        .db
        .read(|c| folders::compare_two(c, (&a_volume, &a_rel), (&b_volume, &b_rel)))
        .map_err(err)
}

/// 폴더 묶음 하나 — keep 을 남기고 drops 에 지우기 표시.
#[tauri::command]
pub async fn cull_folder_set_apply(
    state: State<'_, AppState>,
    keep_folder_id: i64,
    drop_folder_ids: Vec<i64>,
) -> Result<apply::ApplyAll, String> {
    state
        .db
        .transaction(|tx| folders::apply_set(tx, keep_folder_id, &drop_folder_ids))
        .map_err(err)
}

/// 그룹을 보류한다 — 목록에서 빠지되 판정은 하지 않는다.
#[tauri::command]
pub async fn cull_skip(state: State<'_, AppState>, group_ids: Vec<i64>) -> Result<usize, String> {
    state
        .db
        .transaction(|tx| {
            let mut n = 0;
            for gid in &group_ids {
                n += tx.execute("UPDATE groups SET state=2 WHERE id=?1", [gid])?;
            }
            Ok(n)
        })
        .map_err(err)
}

#[derive(Debug, Serialize)]
pub struct CullSummary {
    pub kind: i32,
    pub groups: i64,
    pub photos: i64,
    pub reclaimable: i64,
}

/// 세 갈래의 현재 상태. 화면 상단에 "중복 1,956 · 같은순간 5,700" 식으로 쓴다.
#[tauri::command]
pub async fn cull_summary(state: State<'_, AppState>) -> Result<Vec<CullSummary>, String> {
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT g.kind, COUNT(DISTINCT g.id),
                        (SELECT COUNT(*) FROM group_members m
                          JOIN groups g2 ON g2.id = m.group_id
                          WHERE g2.kind = g.kind AND g2.state = 0),
                        COALESCE(SUM(g.size_bytes),0)
                 FROM groups g WHERE g.state = 0 GROUP BY g.kind ORDER BY g.kind",
            )?;
            let it = st.query_map([], |r| {
                Ok(CullSummary {
                    kind: r.get(0)?,
                    groups: r.get(1)?,
                    photos: r.get(2)?,
                    reclaimable: r.get(3)?,
                })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use crate::db::conn::Db;

    /// 그룹 하나를 만들고 apply가 플래그를 어떻게 바꾸는지 본다.
    fn seed(kind: i32) -> (tempfile::TempDir, Db) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(d.path().join("t.db")).unwrap();
        db.transaction(|tx| {
            tx.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])?;
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1)",
                [],
            )?;
            for i in 1..=3 {
                tx.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                     VALUES(?1,1,?2,100,0,1000,0,0)",
                    rusqlite::params![i, format!("f{i}.jpg")],
                )?;
            }
            tx.execute(
                "INSERT INTO groups(id,kind,reason,size_bytes,state,created_at)
                 VALUES(1,?1,'t',200,0,0)",
                [kind],
            )?;
            for i in 1..=3 {
                tx.execute(
                    "INSERT INTO group_members(group_id,file_id,is_best) VALUES(1,?1,?2)",
                    rusqlite::params![i, (i == 1) as i32],
                )?;
            }
            Ok(())
        })
        .unwrap();
        (d, db)
    }

    fn flags(db: &Db) -> Vec<i32> {
        db.read(|c| {
            let mut st = c.prepare("SELECT culling_flag FROM files ORDER BY id")?;
            let it = st.query_map([], |r| r.get(0))?;
            it.collect::<rusqlite::Result<Vec<i32>>>()
        })
        .unwrap()
    }

    #[test]
    fn apply_keeps_best_and_rejects_the_rest() {
        let (_d, db) = seed(0); // 중복 그룹
        db.transaction(|tx| {
            tx.execute(
                "UPDATE files SET culling_flag=1 WHERE id IN
                 (SELECT file_id FROM group_members WHERE group_id=1 AND is_best=1)",
                [],
            )?;
            tx.execute(
                "UPDATE files SET culling_flag=2 WHERE id IN
                 (SELECT file_id FROM group_members WHERE group_id=1 AND is_best=0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(flags(&db), vec![1, 2, 2], "대표만 남김, 나머지 제외");
    }

    #[test]
    fn junk_groups_reject_everything() {
        let (_d, db) = seed(1); // 잡동사니 그룹 — 대표가 의미 없다
        db.write(|c| {
            c.execute(
                "UPDATE files SET culling_flag=2 WHERE id IN
                 (SELECT file_id FROM group_members WHERE group_id=1)",
                [],
            )
        })
        .unwrap();
        assert_eq!(flags(&db), vec![2, 2, 2], "잡동사니는 전부 제외");
    }

    #[test]
    fn nothing_is_deleted_by_apply() {
        let (_d, db) = seed(0);
        let before: i64 =
            db.read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))).unwrap();
        db.write(|c| c.execute("UPDATE files SET culling_flag=2", [])).unwrap();
        let after: i64 =
            db.read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))).unwrap();
        assert_eq!(before, after, "판정은 삭제가 아니다");
    }
}
