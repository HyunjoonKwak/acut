//! 고르기 커맨드.
//!
//! 스캔은 오래 걸릴 수 있으므로(완전 중복은 파일을 읽는다) 별도 스레드에서 돌고
//! 진행 상황을 이벤트로 흘린다. 조회는 즉시 돌아온다.

use crate::api::{err, AppState};
use crate::cull::{apply, burst, dedup, folders, junk, phash, scene};
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
pub const KIND_RESIZED: i32 = phash::KIND;

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
    /// 대표 파일 이름·폴더 — 확정 띠의 풍선에 «무슨 사진인지»를 보여 준다
    pub name: Option<String>,
    pub folder: Option<String>,
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

/// 갈래 **하나만** 다시 찾는다.
///
/// 전부 돌리면 완전 중복이 파일을 끝까지 읽어 한 시간이 걸린다. 「줄인 사본」은
/// 썸네일만 읽어 31초인데(실측 14.3만 장), 그것 하나 보려고 한 시간을 기다릴
/// 이유가 없다. 갈래끼리 앞뒤가 있긴 하지만 없어도 각자 옳게 돈다 — 완전 중복이
/// 아직 안 돌았으면 「줄인 사본」이 그만큼 덜 걸러 낼 뿐이다.
#[tauri::command]
pub async fn cull_scan_kind(app: AppHandle, kind: i32) -> Result<(), String> {
    // 모르는 갈래는 일을 시작하기 전에 바로 돌려준다 — 작업 스위치를 잡지 않게
    if !matches!(kind, KIND_DUP | KIND_JUNK | KIND_BURST | KIND_SCENE | KIND_RESIZED) {
        return Err(format!("모르는 갈래: {kind}"));
    }
    let state = app.state::<AppState>();
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    let cache_base = state.cache_base.clone();
    let Some(guard) = job::try_start_wait(&state.running, "고르기", std::time::Duration::from_secs(20)) else {
        return Err("다른 일이 도는 중입니다 — 끝난 뒤 다시 눌러 주세요".into());
    };
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let _guard = guard;
        let out: Result<(), String> = match kind {
            KIND_JUNK => junk::scan(&db).map(|p| {
                let _ = app.emit("cull-junk", &p);
            }).map_err(|e| e.to_string()),
            KIND_BURST => burst::scan(&db, burst::DEFAULT_GAP_SECS).map(|p| {
                let _ = app.emit("cull-burst", &p);
            }).map_err(|e| e.to_string()),
            KIND_DUP => dedup::scan(&db, Arc::clone(&cancel), |p| {
                let _ = app.emit("cull-dedup-progress", p);
            })
            .map(|p| {
                let _ = app.emit("cull-dedup", &p);
            })
            .map_err(|e| e.to_string()),
            KIND_RESIZED => phash::scan(&db, &cache_base, phash::DEFAULT_THRESHOLD, Arc::clone(&cancel), |p| {
                let _ = app.emit("cull-phash-progress", p);
            })
            .map(|p| {
                let _ = app.emit("cull-phash", &p);
            })
            .map_err(|e| e.to_string()),
            KIND_SCENE => scene::scan(&db, scene::DEFAULT_THRESHOLD, Arc::clone(&cancel), |_| {}).map(|p| {
                let _ = app.emit("cull-scene", &p);
            }).map_err(|e| e.to_string()),
            other => Err(format!("모르는 갈래: {other}")),
        };
        match out {
            Ok(()) => {
                let _ = app.emit("cull-done", ());
            }
            Err(e) => {
                let _ = app.emit("cull-error", e);
            }
        }
    });
    Ok(())
}

/// 갈래를 순서대로 돌린다. 가벼운 것부터 — 결과가 빨리 보이게.
#[tauri::command]
pub async fn cull_scan(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    let state_cache_base = state.cache_base.clone();
    // 스캔·벡터와 같은 스위치 — 같이 돌면 DB와 디스크를 다툰다. 상태바에 보이고
    // 창이 뒤로 가도 App Nap에 걸리지 않는다 (해시는 한 시간도 걸린다).
    let Some(guard) = job::try_start_wait(&state.running, "고르기", std::time::Duration::from_secs(20)) else {
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
        // 4. 크기만 줄인 사본 — 썸네일을 읽어 지각 해시를 채운 뒤 묶는다.
        //    «비슷한 장면»보다 먼저다: 같은 그림이라는 더 또렷한 판정이므로,
        //    여기서 묶인 짝은 저기서 다시 보여 주지 않는다.
        let cache_base = state_cache_base.clone();
        match phash::scan(&db, &cache_base, phash::DEFAULT_THRESHOLD, Arc::clone(&cancel), |p| {
            let _ = app.emit("cull-phash-progress", p);
        }) {
            Ok(p) => {
                let _ = app.emit("cull-phash", &p);
            }
            Err(e) => {
                let _ = app.emit("cull-error", e.to_string());
                return;
            }
        }
        // 5. 비슷한 장면 — 벡터가 있는 사진만. 없으면 photos 0으로 곧 끝난다.
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

/// 범위 — «제외될 사본이 이 라이브러리에 있는 무리»만. 목록·집계·한꺼번에 확정이 같은 잣대를
/// 써야 «머리의 숫자와 넘겨 보는 무리가 다르다»가 안 생긴다 (2026-08-30).
/// 잡동사니(kind 1)는 대표가 없어 구성원 아무나면 된다.
const SCOPE: &str = "(?5 IS NULL OR EXISTS (
         SELECT 1 FROM group_members m JOIN files fi ON fi.id = m.file_id
         JOIN folders fo ON fo.id = fi.folder_id
         WHERE m.group_id = g.id AND fo.library_id = ?5 AND (g.kind = 1 OR m.is_best = 0)))";

/// 그룹 목록. 확보 용량이 큰 것부터 — 효과가 큰 것을 먼저 보게 한다.
#[tauri::command]
pub async fn cull_groups(
    state: State<'_, AppState>,
    kind: i32,
    limit: usize,
    offset: usize,
    settled: Option<bool>,
    library_id: Option<i64>,
    done: Option<bool>,
) -> Result<Vec<GroupRow>, String> {
    let limit = limit.clamp(1, 200);
    // 정착 구역(내사진·공용) 안에 제외될 사본이 있는 무리만 — 사람이 하나씩 보는 것
    let settled = settled.unwrap_or(false);
    // «처리됨 보기» — 확정한 무리를 최근 순으로. 앱을 껐다 켜도 남는다 (2026-08-31)
    let done = done.unwrap_or(false);
    let (want_state, order) = if done { (1, "g.done_at DESC, g.id DESC") } else { (0, "g.size_bytes DESC") };
    state
        .db
        .read(|c| {
            let mut st = c.prepare(&format!(
                "SELECT g.id, g.kind, g.reason, g.size_bytes, g.state,
                        (SELECT COUNT(*) FROM group_members m WHERE m.group_id = g.id),
                        (SELECT fo.library_id || '/' || t.rel_path FROM group_members m
                         JOIN files fi ON fi.id = m.file_id
                         JOIN folders fo ON fo.id = fi.folder_id
                         JOIN thumbs t ON t.file_id = m.file_id AND t.state = 1
                         WHERE m.group_id = g.id
                         ORDER BY m.is_best DESC LIMIT 1),
                        (SELECT fi.name FROM group_members m JOIN files fi ON fi.id = m.file_id
                         WHERE m.group_id = g.id ORDER BY m.is_best DESC, m.file_id LIMIT 1),
                        (SELECT fo.rel_path FROM group_members m JOIN files fi ON fi.id = m.file_id
                         JOIN folders fo ON fo.id = fi.folder_id
                         WHERE m.group_id = g.id ORDER BY m.is_best DESC, m.file_id LIMIT 1)
                 FROM groups g
                 WHERE g.kind = ?1 AND g.state = ?6
                   AND (?4 = 0 OR EXISTS (
                         SELECT 1 FROM group_members m JOIN files fi ON fi.id = m.file_id
                         JOIN folders fo ON fo.id = fi.folder_id
                         WHERE m.group_id = g.id AND m.is_best = 0 AND fo.area IN (1, 2)))
                   AND {SCOPE}
                 ORDER BY {order}
                 LIMIT ?2 OFFSET ?3"
            ))?;
            let it = st.query_map(
                rusqlite::params![kind, limit as i64, offset as i64, settled as i32, library_id, want_state],
                |r| {
                    Ok(GroupRow {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        reason: r.get(2)?,
                        size_bytes: r.get(3)?,
                        state: r.get(4)?,
                        member_count: r.get(5)?,
                        cover: r.get(6)?,
                        name: r.get(7)?,
                        folder: r.get(8)?,
                    })
                },
            )?;
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
                 WHERE m.group_id = ?1 AND f.trashed_at IS NULL
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
    /// 한꺼번에 확정에서 정착 구역(내사진·공용)이라 건너뛴 수. 사람이 보고 누른 확정은 0
    pub skipped: usize,
}

/// 그룹의 판정을 확정한다 — 대표는 '남김', 나머지는 '제외'로 표시한다.
///
/// **파일을 지우지는 않는다.** `culling_flag`만 바꾼다. 실제 삭제는 사용자가
/// 따로 확인한 뒤에 한다. 잘못 눌러도 되돌릴 수 있어야 한다.
/// 사람이 보고 누른 것이라 정착 구역·«남김» 도 넘어선다 — 규칙은 [`apply::apply_groups`].
#[tauri::command]
pub async fn cull_apply(state: State<'_, AppState>, group_ids: Vec<i64>) -> Result<ApplyResult, String> {
    if group_ids.is_empty() {
        return Ok(ApplyResult { kept: 0, rejected: 0, skipped: 0 });
    }
    let (kept, rejected) = state
        .db
        .transaction(|tx| apply::apply_groups(tx, &group_ids))
        .map_err(err)?;
    Ok(ApplyResult { kept, rejected, skipped: 0 })
}

/// 확정을 무리 단위로 되돌린다 — «↩ 확정 취소» (규칙은 [`apply::unapply_groups`])
#[tauri::command]
pub async fn cull_unapply(state: State<'_, AppState>, group_ids: Vec<i64>) -> Result<usize, String> {
    state
        .db
        .transaction(|tx| apply::unapply_groups(tx, &group_ids))
        .map_err(err)
}

/// 라이브러리별 미결 무리 수 — 범위 선택지에 붙여 «고르면 무엇이 달라지나»가 미리 보이게
#[derive(Debug, Serialize)]
pub struct ScopeCount {
    pub library_id: i64,
    pub groups: i64,
}

#[tauri::command]
pub async fn cull_scope_counts(state: State<'_, AppState>, kind: i32) -> Result<Vec<ScopeCount>, String> {
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT fo.library_id, COUNT(DISTINCT g.id)
                 FROM groups g JOIN group_members m ON m.group_id = g.id
                 JOIN files fi ON fi.id = m.file_id JOIN folders fo ON fo.id = fi.folder_id
                 WHERE g.kind = ?1 AND g.state = 0 AND (g.kind = 1 OR m.is_best = 0)
                 GROUP BY fo.library_id",
            )?;
            let it = st.query_map([kind], |r| {
                Ok(ScopeCount { library_id: r.get(0)?, groups: r.get(1)? })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
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

/// 폴더 비교로 붙인 표시를 되돌린다 — (표시를 되돌린 장수, 다시 연 무리 수).
#[tauri::command]
pub async fn cull_folder_set_unapply(
    state: State<'_, AppState>,
    folder_ids: Vec<i64>,
) -> Result<(usize, usize), String> {
    state.db.transaction(|tx| folders::unapply_folders(tx, &folder_ids)).map_err(err)
}

/// 두 폴더 비교의 «전부» — (남길 폴더, 지울 폴더) 짝 목록을 한 트랜잭션에.
#[tauri::command]
pub async fn cull_folder_pairs_apply(
    state: State<'_, AppState>,
    pairs: Vec<PairIds>,
) -> Result<folders::PairsApplied, String> {
    if pairs.is_empty() {
        return Ok(folders::PairsApplied::default());
    }
    let pairs: Vec<(Vec<i64>, Vec<i64>)> = pairs.into_iter().map(|p| (p.keep, p.drop)).collect();
    state.db.transaction(|tx| folders::apply_pairs(tx, &pairs)).map_err(err)
}

/// 두 나무의 해시 없는 사진에 해시를 붙인다 — 비교가 «똑같음»을 가릴 수 있게
#[tauri::command]
pub async fn cull_hash_folders(app: AppHandle, folder_ids: Vec<i64>) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(&state.running, "해시 계산", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        folders::hash_missing(&db, &folder_ids, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)
}

/// 폴더 짝 «보기» — 두 나무의 사진을 나란히, 같은 내용끼리 이어서.
#[tauri::command]
pub async fn cull_folder_pair_photos(
    state: State<'_, AppState>,
    a_ids: Vec<i64>,
    b_ids: Vec<i64>,
) -> Result<folders::PairPhotos, String> {
    state.db.read(|c| folders::pair_photos(c, &a_ids, &b_ids)).map_err(err)
}

/// 두 폴더 비교의 짝 하나 — 남길 쪽 폴더 행들과 제외할 쪽 폴더 행들(하위 폴더 포함)
#[derive(Debug, serde::Deserialize)]
pub struct PairIds {
    pub keep: Vec<i64>,
    pub drop: Vec<i64>,
}

/// 폴더 비교 — 내용이 완전히 같은 폴더 묶음들.
#[tauri::command]
pub async fn cull_folder_sets(state: State<'_, AppState>) -> Result<Vec<folders::FolderSet>, String> {
    state.db.read(|c| folders::identical_sets(c, 5000)).map_err(err)
}

/// 두 폴더 비교 — A·B 아래 폴더들을 내용으로 짝짓는다.
#[tauri::command]
pub async fn cull_compare_folders(
    state: State<'_, AppState>,
    a_volume: String,
    a_rel: String,
    b_volume: String,
    b_rel: String,
) -> Result<folders::Compared, String> {
    if a_volume == b_volume && a_rel == b_rel {
        return Err("같은 폴더끼리는 비교할 수 없습니다".into());
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
    keep_ids: Vec<i64>,
    drop_ids: Vec<i64>,
) -> Result<apply::ApplyAll, String> {
    state
        .db
        .transaction(|tx| folders::apply_trees(tx, &keep_ids, &drop_ids))
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
    /// 처리된(확정한) 무리 수 — «처리됨 보기» 토글에 붙는다
    pub done: i64,
}

/// 세 갈래의 현재 상태. 화면 상단에 "중복 1,956 · 같은순간 5,700" 식으로 쓴다.
#[tauri::command]
pub async fn cull_summary(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<Vec<CullSummary>, String> {
    state
        .db
        .read(|c| {
            // 범위를 걸면 장수도 그 무리들의 것이어야 한다 — 머리 숫자와 넘겨 보는 무리를 맞춘다
            let scope = SCOPE.replace("?5", "?1");
            let mut st = c.prepare(&format!(
                "SELECT g.kind,
                        COALESCE(SUM(g.state = 0),0),
                        COALESCE(SUM(CASE WHEN g.state = 0 THEN
                          (SELECT COUNT(*) FROM group_members m WHERE m.group_id = g.id) ELSE 0 END),0),
                        COALESCE(SUM(CASE WHEN g.state = 0 THEN g.size_bytes ELSE 0 END),0),
                        COALESCE(SUM(g.state = 1),0)
                 FROM groups g WHERE {scope} GROUP BY g.kind ORDER BY g.kind"
            ))?;
            let it = st.query_map([library_id], |r| {
                Ok(CullSummary {
                    kind: r.get(0)?,
                    groups: r.get(1)?,
                    photos: r.get(2)?,
                    reclaimable: r.get(3)?,
                    done: r.get(4)?,
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

    /// 범위 SQL 이 목록·집계에서 같은 무리를 고르는지 — 어긋나면 «머리 숫자와 넘겨 보는 무리가 다르다»
    #[test]
    fn scope_sql_matches_groups_whose_rejected_copy_is_in_that_library() {
        let (_d, db) = seed(0);
        // 파일 1(대표)은 라이브러리 1, 파일 2·3은 라이브러리 2 로 옮긴다
        db.transaction(|tx| {
            tx.execute("INSERT OR IGNORE INTO volumes(uuid,name,role) VALUES('V','v','library')", [])?;
            tx.execute(
                "INSERT OR IGNORE INTO libraries(id,volume_uuid,rel_path,name) VALUES(1,'V','a','A'),(2,'V','b','B')",
                [],
            )?;
            tx.execute("UPDATE folders SET library_id = 1", [])?;
            tx.execute(
                "INSERT OR REPLACE INTO folders(id,volume_uuid,rel_path,name,area,library_id)
                 VALUES(2,'V','b','b',0,2)",
                [],
            )?;
            tx.execute("UPDATE files SET folder_id = 2 WHERE id IN (2,3)", [])?;
            Ok(())
        })
        .unwrap();
        let count = |lib: Option<i64>| -> i64 {
            db.read(|c| {
                c.query_row(
                    &format!("SELECT COUNT(*) FROM groups g WHERE {}", super::SCOPE.replace("?5", "?1")),
                    [lib],
                    |r| r.get(0),
                )
            })
            .unwrap()
        };
        assert_eq!(count(None), 1, "범위 없으면 전부");
        assert_eq!(count(Some(2)), 1, "제외될 사본이 B 에 있다");
        assert_eq!(count(Some(1)), 0, "A 엔 대표뿐이라 지울 것이 없다");
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
