//! NAS 명령 — 1차 구역 내려받기, 올라갔나 확인, 확인된 것 비우기, XMP.
//!
//! NAS는 종(從)이다. 동기화 배관(내사진 ↔ Photos, 공용 ↔ photo)은 Drive
//! Client가 맡고, 에이컷은 그 결과를 **확인**만 한다(nas_state). 1차 구역은
//! 처리 대기열 — 내려받아 작업대에서 고르고, 정리가 끝나 NAS에 올라간 것이
//! 확인되면 1차에서 비운다. 순서가 곧 안전장치다: 확인 전엔 아무것도 지우지
//! 않는다.

use super::{err, job, AppState};
use crate::db::{libraries, settings};
use crate::nas::ssh::{self, Config};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

const KEY_CONFIG: &str = "nas.config";
/// 작업대 라이브러리 안에서 1차 구역이 내려앉는 폴더
pub const ZONE1_DIR: &str = "NAS-1차";

fn load(state: &AppState) -> Result<Config, String> {
    Ok(settings::get(&state.db, KEY_CONFIG)
        .map_err(err)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

#[tauri::command]
pub async fn nas_config(state: State<'_, AppState>) -> Result<Config, String> {
    load(&state)
}

#[tauri::command]
pub async fn nas_config_set(state: State<'_, AppState>, config: Config) -> Result<Config, String> {
    let c = Config {
        host: config.host.trim().to_string(),
        zone1: config.zone1.trim().trim_end_matches('/').to_string(),
        photos: config.photos.trim().trim_end_matches('/').to_string(),
        shared: config.shared.trim().trim_end_matches('/').to_string(),
        exclude: config.exclude.trim().to_string(),
        rsync_port: if config.rsync_port == 0 { 22 } else { config.rsync_port },
    };
    if c.host.is_empty() || c.zone1.is_empty() {
        return Err("호스트와 1차 구역 경로는 비울 수 없습니다".into());
    }
    settings::set(&state.db, KEY_CONFIG, &serde_json::to_string(&c).unwrap()).map_err(err)?;
    Ok(c)
}

/// 연결 확인 — ssh 한 번. 남은 공간과 1차 구역 파일 수도.
#[tauri::command]
pub async fn nas_check(app: AppHandle) -> Result<ssh::Status, String> {
    let cfg = load(&app.state::<AppState>())?;
    Ok(tauri::async_runtime::spawn_blocking(move || ssh::check(&cfg))
        .await
        .map_err(|e| e.to_string())?)
}

/// 원장 — 받은 적 있는 것들의 상대경로
fn ledger(db: &crate::db::conn::Db) -> Result<Vec<String>, String> {
    db.read(|c| {
        let mut st = c.prepare("SELECT rel_path FROM nas_pulls")?;
        let it = st.query_map([], |r| r.get(0))?;
        it.collect::<rusqlite::Result<Vec<String>>>()
    })
    .map_err(err)
}

#[derive(Debug, Clone, Serialize)]
pub struct Probe {
    pub online: bool,
    pub hostname: String,
    /// 내려받을 작업대 라이브러리 — 없으면 None (등록 안 됨·오프라인)
    pub library_id: Option<i64>,
    pub new_files: usize,
    pub new_bytes: u64,
    pub error: Option<String>,
}

/// 앱을 열 때·주기적으로 — NAS가 켜져 있나, 1차 구역에 받은 적 없는 사진이 있나.
/// 실패해도 조용하다(error에 담아 돌려줄 뿐). NAS가 꺼져 있어도 로컬은 무영향.
#[tauri::command]
pub async fn nas_probe(app: AppHandle) -> Result<Probe, String> {
    let state = app.state::<AppState>();
    let cfg = load(&state)?;
    let libs = libraries::list(&state.db).map_err(err)?;
    let desk = libs.into_iter().find(|l| l.area == 0 && l.online);
    let already = ledger(&state.db)?;
    let cfg2 = cfg.clone();
    let dest = desk.as_ref().and_then(|l| l.dir.clone()).map(|d| d.join(ZONE1_DIR));
    let r = tauri::async_runtime::spawn_blocking(move || {
        let st = ssh::check(&cfg2);
        if !st.online {
            return Probe { online: false, hostname: String::new(), library_id: None, new_files: 0, new_bytes: 0, error: st.error };
        }
        let Some(dest) = dest else {
            return Probe { online: true, hostname: st.hostname, library_id: None, new_files: 0, new_bytes: 0, error: None };
        };
        match ssh::count_new(&cfg2, &dest, &already) {
            Ok((n, b)) => Probe { online: true, hostname: st.hostname, library_id: None, new_files: n, new_bytes: b, error: None },
            Err(e) => Probe { online: true, hostname: st.hostname, library_id: None, new_files: 0, new_bytes: 0, error: Some(e.to_string()) },
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(Probe { library_id: desk.map(|l| l.id), ..r })
}

#[derive(Debug, Clone, Serialize)]
pub struct PullDone {
    pub library_id: i64,
    pub files: usize,
    pub cancelled: bool,
}

/// 1차 구역을 작업대 라이브러리의 `NAS-1차/`로. 진행은 `nas-pull-progress`, 끝나면 `nas-pull-done`.
#[tauri::command]
pub async fn nas_pull_start(app: AppHandle, library_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cfg = load(&state)?;
    let lib = libraries::get(&state.db, library_id).map_err(err)?.ok_or("등록되지 않은 라이브러리입니다")?;
    if lib.area != 0 {
        return Err("1차 구역은 작업대 라이브러리로만 내려받습니다".into());
    }
    let dir = lib.dir.clone().ok_or("디스크가 연결되어 있지 않습니다")?;
    let Some(guard) = job::try_start(&state.running, "에이컷 NAS 내려받기") else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);
    let db = Arc::clone(&state.db);
    let already = ledger(&state.db)?;
    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let dest = dir.join(ZONE1_DIR);
        let r = ssh::pull(&cfg, &dest, &already, &cancel, |p| {
            let _ = handle.emit("nas-pull-progress", p);
        });
        // 원장 — 무엇을 언제 받았나. 비울 때 이걸로 «우리가 받은 것»만 고른다.
        // 이번에 옮긴 것만이 아니라 지금 폴더에 있는 완성 파일 전부 — 멈췄다 이어받은
        // 것도, 원장이 생기기 전에 받은 것도, rsync 가 중간에 실패했어도 디스크에 있는
        // 것은 «받은 것»이다. 안 적으면 다음에 또 받는다 (리뷰 H10)
        let now = chrono::Utc::now().timestamp();
        let present = ssh::present_files(&dest);
        let _ = db.transaction(|tx| {
            let mut ins = tx.prepare(
                "INSERT INTO nas_pulls(rel_path, size, pulled_at) VALUES(?1, ?2, ?3)
                 ON CONFLICT(rel_path) DO UPDATE SET size = excluded.size",
            )?;
            for (rel, size) in &present {
                ins.execute(rusqlite::params![rel, *size as i64, now])?;
            }
            Ok(())
        });
        match r {
            Ok(pulled) => {
                let _ = app.emit("nas-pull-done", PullDone { library_id, files: pulled.files.len(), cancelled: pulled.cancelled });
            }
            Err(e) => {
                let _ = app.emit("nas-error", e.to_string());
            }
        }
    });
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Verified {
    pub library_id: i64,
    pub present: usize,
    pub missing: usize,
    /// 없는 것 몇 개 — 눈으로 볼 수 있게
    pub sample: Vec<String>,
}

/// 이 라이브러리가 NAS에 다 있나 — 내사진은 Photos에, 공용은 photo에.
/// 있는 것은 nas_state에 적고, 없는 것은 지운다.
#[tauri::command]
pub async fn nas_verify(app: AppHandle, library_id: i64) -> Result<Verified, String> {
    let state = app.state::<AppState>();
    let cfg = load(&state)?;
    let lib = libraries::get(&state.db, library_id).map_err(err)?.ok_or("등록되지 않은 라이브러리입니다")?;
    let remote = match lib.area {
        1 => cfg.photos.clone(),
        2 => cfg.shared.clone(),
        _ => return Err("내사진(개인)과 공용 라이브러리만 확인합니다".into()),
    };
    let dir = lib.dir.clone().ok_or("디스크가 연결되어 있지 않습니다")?;
    // «보낼 게 없다»는 «다 올라갔다»가 아니다 — 로컬 폴더가 비었거나 반만 있으면(마운트
    // 직후, Drive 재대조 중) rsync -n 은 아무것도 안 보내고, 그러면 전 행이 «NAS에 있음»으로
    // 찍혀 1차 비우기의 근거가 된다. 디스크의 파일 수가 DB의 90% 미만이면 거부한다 (리뷰 C7)
    let expected: i64 = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                  WHERE fo.library_id = ?1 AND fi.trashed_at IS NULL",
                [library_id],
                |r| r.get(0),
            )
        })
        .map_err(err)?;
    let on_disk = ssh::present_files(&dir).len() as i64;
    if expected > 0 && on_disk * 10 < expected * 9 {
        return Err(format!(
            "디스크에 {on_disk}개뿐입니다 (DB에는 {expected}개) — 폴더가 다 붙은 뒤 다시 확인하세요"
        ));
    }
    let cfg2 = cfg.clone();
    let missing: std::collections::HashSet<String> =
        tauri::async_runtime::spawn_blocking(move || ssh::missing_on_nas(&cfg2, &dir, &remote))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect();
    // 라이브러리의 파일 전부 — 라이브러리 루트 기준 상대경로로
    let rows: Vec<(i64, String)> = state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT fi.id, fo.rel_path, fi.name FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                  WHERE fo.library_id = ?1 AND fi.trashed_at IS NULL",
            )?;
            let it = st.query_map([library_id], |r| {
                let dir: String = r.get(1)?;
                let name: String = r.get(2)?;
                Ok((r.get(0)?, crate::media::cache::rel_path(&dir, &name)))
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)?;
    let lib_rel = lib.rel_path.clone();
    let under = |vol_rel: &str| -> String {
        if lib_rel.is_empty() {
            vol_rel.to_string()
        } else {
            vol_rel.strip_prefix(&lib_rel).map(|s| s.trim_start_matches('/').to_string()).unwrap_or_else(|| vol_rel.to_string())
        }
    };
    let now = chrono::Utc::now().timestamp();
    let (mut present, mut absent) = (0usize, 0usize);
    let remote_root = if lib.area == 1 { cfg.photos.clone() } else { cfg.shared.clone() };
    state
        .db
        .transaction(|tx| {
            let mut put = tx.prepare(
                "INSERT INTO nas_state(file_id, area, remote_path, uploaded_at) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(file_id) DO UPDATE SET area = excluded.area, remote_path = excluded.remote_path, uploaded_at = excluded.uploaded_at",
            )?;
            let mut del = tx.prepare("DELETE FROM nas_state WHERE file_id = ?1")?;
            for (id, vol_rel) in &rows {
                let rel = under(vol_rel);
                if missing.contains(&rel) {
                    absent += 1;
                    del.execute([id])?;
                } else {
                    present += 1;
                    put.execute(rusqlite::params![id, lib.area, format!("{remote_root}/{rel}"), now])?;
                }
            }
            Ok(())
        })
        .map_err(err)?;
    let mut sample: Vec<String> = missing.iter().take(20).cloned().collect();
    sample.sort();
    Ok(Verified { library_id, present, missing: absent, sample })
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgeItem {
    pub rel: String,
    pub size: i64,
    /// «올라감» 또는 «버림»
    pub why: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgePlan {
    pub items: Vec<PurgeItem>,
    pub bytes: i64,
    /// 아직 처리 안 된 것 (작업대에 그대로)
    pub pending: usize,
    /// 어디로 갔는지 모르는 것 — 건드리지 않는다
    pub unknown: usize,
}

/// 1차 구역에서 비워도 되는 것 — 우리가 받았고, 작업대에서 사라졌고,
/// 올라간 것이 확인됐거나(nas_state) 버린 것(휴지통).
#[tauri::command]
pub async fn nas_purge_plan(state: State<'_, AppState>, library_id: i64) -> Result<PurgePlan, String> {
    let lib = libraries::get(&state.db, library_id).map_err(err)?.ok_or("등록되지 않은 라이브러리입니다")?;
    if lib.area != 0 {
        return Err("작업대 라이브러리를 고르세요".into());
    }
    let base = crate::media::cache::rel_path(&lib.rel_path, ZONE1_DIR);
    let local_root = lib.dir.clone();
    let ledger: Vec<(String, i64)> = state
        .db
        .read(|c| {
            let mut st = c.prepare("SELECT rel_path, size FROM nas_pulls ORDER BY rel_path")?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)?;
    let mut plan = PurgePlan { items: Vec::new(), bytes: 0, pending: 0, unknown: 0 };
    for (rel, size) in ledger {
        let orig = crate::media::cache::rel_path(&base, &rel);
        // 아직 작업대에 그대로 있나 (디스크 또는 DB)
        let on_disk = local_root.as_ref().map(|d| d.join(ZONE1_DIR).join(&rel).exists()).unwrap_or(false);
        let (dir, name) = match orig.rsplit_once('/') {
            Some((d, n)) => (d.to_string(), n.to_string()),
            None => (String::new(), orig.clone()),
        };
        let live: i64 = state
            .db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                      WHERE fo.library_id = ?1 AND fo.rel_path = ?2 AND fi.name = ?3 AND fi.trashed_at IS NULL",
                    rusqlite::params![library_id, dir, name],
                    |r| r.get(0),
                )
            })
            .map_err(err)?;
        if on_disk || live > 0 {
            plan.pending += 1;
            continue;
        }
        let trashed: i64 = state
            .db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                      WHERE fo.library_id = ?1 AND fo.rel_path = ?2 AND fi.name = ?3 AND fi.trashed_at IS NOT NULL",
                    rusqlite::params![library_id, dir, name],
                    |r| r.get(0),
                )
            })
            .map_err(err)?;
        if trashed > 0 {
            plan.bytes += size;
            plan.items.push(PurgeItem { rel, size, why: "버림".into() });
            continue;
        }
        // 옮겼나 — 저널의 마지막 기록
        let moved: Option<(Option<i64>, String)> = state
            .db
            .read(|c| {
                c.query_row(
                    "SELECT file_id, op FROM journal WHERE from_path = ?1 AND ok = 1 ORDER BY id DESC LIMIT 1",
                    [&orig],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map(Some)
                .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
            })
            .map_err(err)?;
        match moved {
            Some((_, op)) if op == "trash" => {
                plan.bytes += size;
                plan.items.push(PurgeItem { rel, size, why: "버림".into() });
            }
            Some((Some(fid), op)) if op == "move" => {
                let up: i64 = state
                    .db
                    .read(|c| c.query_row("SELECT COUNT(*) FROM nas_state WHERE file_id = ?1", [fid], |r| r.get(0)))
                    .map_err(err)?;
                if up > 0 {
                    plan.bytes += size;
                    plan.items.push(PurgeItem { rel, size, why: "올라감".into() });
                } else {
                    plan.unknown += 1;
                }
            }
            _ => plan.unknown += 1,
        }
    }
    Ok(plan)
}

#[derive(Debug, Clone, Serialize)]
pub struct Purged {
    pub moved: usize,
    pub bytes: i64,
}

/// 계획의 파일들을 1차 구역의 `#trash/`로 옮긴다. 원장에서도 지운다.
#[tauri::command]
pub async fn nas_purge_run(app: AppHandle, rels: Vec<String>) -> Result<Purged, String> {
    let state = app.state::<AppState>();
    let cfg = load(&state)?;
    // 화면이 준 목록을 그대로 믿지 않는다 — 우리가 받은 것(원장)이고 1차 구역 안의
    // 경로일 때만. 내려받기와 겹쳐 돌지도 않게 잡을 잡는다 (리뷰 C6)
    let allowed: std::collections::HashSet<String> = ledger(&state.db)?.into_iter().collect();
    let rels2: Vec<String> = rels
        .into_iter()
        .filter(|r| ssh::safe_zone1_rel(r) && allowed.contains(r))
        .collect();
    if rels2.is_empty() {
        return Ok(Purged { moved: 0, bytes: 0 });
    }
    let Some(guard) = job::try_start(&state.running, "에이컷 NAS 비우기") else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let moved = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        ssh::trash_in_zone1(&cfg, &rels2)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let mut bytes = 0i64;
    state
        .db
        .transaction(|tx| {
            let mut del = tx.prepare("DELETE FROM nas_pulls WHERE rel_path = ?1 RETURNING size")?;
            for r in &moved {
                let mut rows = del.query([r])?;
                if let Some(row) = rows.next()? {
                    bytes += row.get::<_, i64>(0)?;
                }
            }
            Ok(())
        })
        .map_err(err)?;
    let _ = app.emit("nas-purge-done", Purged { moved: moved.len(), bytes });
    Ok(Purged { moved: moved.len(), bytes })
}

/// 사이드카 내보내기. 진행은 `xmp-progress`, 끝나면 `xmp-done`.
#[tauri::command]
pub async fn xmp_export(app: AppHandle, library_id: Option<i64>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start(&state.running, "에이컷 XMP") else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let r = crate::ops::xmp::export(&db, library_id, |done, total| {
            let _ = handle.emit("xmp-progress", serde_json::json!({ "done": done, "total": total }));
        });
        match r {
            Ok(x) => {
                let _ = app.emit("xmp-done", x);
            }
            Err(e) => {
                let _ = app.emit("nas-error", e.to_string());
            }
        }
    });
    Ok(())
}

#[allow(dead_code)]
fn _assert_path(_: &Path) {}
