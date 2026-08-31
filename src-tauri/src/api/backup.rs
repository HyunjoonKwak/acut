//! 추가 백업 — 라이브러리를 다른 디스크에 한 벌 더.
//!
//! RAW는 맥에 한 벌만 두기로 했다(2026-08-27). 이 백업이 그 보험이다.
//! NAS 동기화는 Drive Client가 하고, 로컬 디스크끼리는 여기서 — v1의
//! sync.rs(단방향 미러·체크섬·충돌·제외 패턴)를 그대로 쓴다.
//!
//! 규칙: 원본 → 백업 한 방향. 백업에만 있는 파일은 세어 알릴 뿐 지우지
//! 않는다. 백업 쪽이 더 새것이면(충돌) 건너뛰고 알린다. 복사한 파일은
//! xxHash로 다시 읽어 맞는지 본다 — 반만 써진 사본은 지우고 다음에 다시.
//!
//! 대상 디스크는 볼륨 UUID + 상대경로로 기억한다. 마운트 이름은 바뀐다.

use super::{err, job, AppState};
use crate::core::sync::{self, SyncPlan, SyncTask};
use crate::db::{libraries, settings, volumes};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

const KEY_TARGET: &str = "backup.target";
const KEY_LAST: &str = "backup.last";
/// 백업하지 않는 것 — 캐시·시스템 찌꺼기·NAS 휴지통
const EXCLUDE: [&str; 7] = [
    ".acut/**",
    ".DS_Store",
    "*.part",
    "#recycle/**",
    "@eaDir/**",
    ".Spotlight-V100/**",
    ".Trashes/**",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub uuid: String,
    pub rel: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Last {
    pub at: i64,
    pub copied: usize,
    pub updated: usize,
    pub bytes: u64,
    pub errors: usize,
    pub cancelled: bool,
}

#[derive(Debug, Serialize)]
pub struct TargetInfo {
    pub target: Option<Target>,
    pub online: bool,
    pub dir: Option<String>,
    pub free_bytes: Option<u64>,
    pub last: Option<Last>,
}

#[derive(Debug, Serialize)]
pub struct LibPlan {
    pub library_id: i64,
    pub name: String,
    pub files: usize,
    pub bytes: u64,
    pub conflicts: usize,
    pub orphans: usize,
    pub offline: bool,
}

#[derive(Debug, Serialize)]
pub struct BackupPlan {
    pub libs: Vec<LibPlan>,
    pub files: usize,
    pub bytes: u64,
    pub conflicts: usize,
    pub orphans: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupProgress {
    pub library: String,
    pub done: usize,
    pub total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current: String,
}

/// 한 라이브러리치 계획 — 실행할 때까지 AppState에 둔다
pub struct Planned {
    pub library_id: i64,
    pub name: String,
    pub plan: SyncPlan,
}

fn load_target(state: &AppState) -> Result<Option<Target>, String> {
    Ok(settings::get(&state.db, KEY_TARGET)
        .map_err(err)?
        .and_then(|s| serde_json::from_str(&s).ok()))
}

fn target_dir(t: &Target) -> Option<PathBuf> {
    libraries::dir_of(&t.uuid, &t.rel)
}

fn info(state: &AppState) -> Result<TargetInfo, String> {
    let target = load_target(state)?;
    let dir = target.as_ref().and_then(target_dir);
    let free_bytes = dir
        .as_ref()
        .and_then(|d| volumes::volume_stat(d).ok())
        .map(|(_, _, free)| free);
    let last = settings::get(&state.db, KEY_LAST)
        .map_err(err)?
        .and_then(|s| serde_json::from_str(&s).ok());
    Ok(TargetInfo {
        online: dir.is_some(),
        dir: dir.map(|d| d.to_string_lossy().into_owned()),
        target,
        free_bytes,
        last,
    })
}

#[tauri::command]
pub async fn backup_target(state: State<'_, AppState>) -> Result<TargetInfo, String> {
    info(&state)
}

/// 백업 폴더를 정한다. 라이브러리 안이거나 라이브러리를 품으면 거절한다 —
/// 자기 자신 안으로 복사하면 끝이 없다.
#[tauri::command]
pub async fn backup_set_target(state: State<'_, AppState>, path: String) -> Result<TargetInfo, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("폴더가 아닙니다: {path}"));
    }
    let v = volumes::describe(&dir).map_err(|e| e.to_string())?;
    let rel = libraries::rel_within(&v.mount_path, &dir)
        .ok_or_else(|| format!("볼륨 안의 경로가 아닙니다: {path}"))?;
    for l in libraries::list(&state.db).map_err(err)? {
        if l.volume_uuid == v.uuid && libraries::overlaps(&l.rel_path, &rel) {
            return Err(format!("라이브러리 「{}」와 겹칩니다. 다른 디스크의 빈 폴더를 고르세요.", l.name));
        }
    }
    let t = Target { uuid: v.uuid, rel, name: v.name };
    settings::set(&state.db, KEY_TARGET, &serde_json::to_string(&t).unwrap()).map_err(err)?;
    info(&state)
}

/// 백업 안에서 이 라이브러리가 들어갈 폴더 이름 — 같은 이름이 둘이면 id를 붙인다
fn dest_name(lib: &libraries::Library, all: &[libraries::Library]) -> String {
    let dup = all.iter().filter(|o| o.name == lib.name).count() > 1;
    if dup {
        format!("{} ({})", lib.name, lib.id)
    } else {
        lib.name.clone()
    }
}

fn task_for(lib: &libraries::Library, src: &Path, dest: &Path) -> SyncTask {
    SyncTask {
        id: lib.id.to_string(),
        name: lib.name.clone(),
        source_dir: src.to_string_lossy().into_owned(),
        target_dir: dest.to_string_lossy().into_owned(),
        exclusion_patterns: EXCLUDE.iter().map(|s| s.to_string()).collect(),
        verify_checksum: true,
        detect_orphans: true,
    }
}

/// 무엇을 복사할지 살핀다. 시간이 걸린다(14만 장이면 수십 초) — 비동기 명령.
#[tauri::command]
pub async fn backup_plan(app: AppHandle) -> Result<BackupPlan, String> {
    let state = app.state::<AppState>();
    let target = load_target(&state)?.ok_or("백업 폴더를 먼저 정하세요")?;
    let root = target_dir(&target).ok_or("백업 디스크가 연결되어 있지 않습니다")?;
    let libs = libraries::list(&state.db).map_err(err)?;
    let mut out = BackupPlan { libs: Vec::new(), files: 0, bytes: 0, conflicts: 0, orphans: 0 };
    let mut planned: Vec<Planned> = Vec::new();
    for lib in &libs {
        let Some(src) = lib.dir.clone() else {
            out.libs.push(LibPlan {
                library_id: lib.id,
                name: lib.name.clone(),
                files: 0,
                bytes: 0,
                conflicts: 0,
                orphans: 0,
                offline: true,
            });
            continue;
        };
        let dest = root.join(dest_name(lib, &libs));
        let plan = sync::plan_sync(&task_for(lib, &src, &dest))?;
        let files = plan.files_to_copy.len() + plan.files_to_update.len();
        let bytes: u64 = plan
            .files_to_copy
            .iter()
            .chain(plan.files_to_update.iter())
            .map(|f| f.file_size)
            .sum();
        out.files += files;
        out.bytes += bytes;
        out.conflicts += plan.conflicts.len();
        out.orphans += plan.orphan_files.len();
        out.libs.push(LibPlan {
            library_id: lib.id,
            name: lib.name.clone(),
            files,
            bytes,
            conflicts: plan.conflicts.len(),
            orphans: plan.orphan_files.len(),
            offline: false,
        });
        planned.push(Planned { library_id: lib.id, name: lib.name.clone(), plan });
    }
    *state.backup_plans.lock().unwrap_or_else(|e| e.into_inner()) = Some(planned);
    Ok(out)
}

/// 복사한 파일이 원본과 같은지 — 크기, 그다음 xxHash. 다르면 사본을 지운다.
fn verify(plan: &SyncPlan, errors: &mut Vec<String>) -> usize {
    let mut bad = 0;
    for op in plan.files_to_copy.iter().chain(plan.files_to_update.iter()) {
        let (s, t) = (Path::new(&op.source), Path::new(&op.target));
        let same_size = std::fs::metadata(t).map(|m| m.len() == op.file_size).unwrap_or(false);
        let same_hash = same_size && crate::core::hasher::xxhash_file(s) == crate::core::hasher::xxhash_file(t);
        if !same_hash {
            bad += 1;
            let _ = std::fs::remove_file(t);
            errors.push(format!("사본이 원본과 다릅니다 — 지웠습니다, 다음에 다시: {}", op.target));
        }
    }
    bad
}

/// 살펴본 계획대로 복사한다. 진행은 `backup-progress`, 끝나면 `backup-done`.
#[tauri::command]
pub async fn backup_run(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let planned = state
        .backup_plans
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or("먼저 「살펴보기」를 누르세요")?;
    let Some(guard) = job::try_start_wait(&state.running, "에이컷 백업", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);
    sync::SYNC_CANCELLED.store(false, Ordering::SeqCst);
    let db = Arc::clone(&state.db);

    std::thread::spawn(move || {
        let _guard = guard;
        let total_files: usize = planned
            .iter()
            .map(|p| p.plan.files_to_copy.len() + p.plan.files_to_update.len())
            .sum();
        let total_bytes: u64 = planned.iter().map(|p| p.plan.total_bytes).sum();
        let mut last = Last { at: chrono::Utc::now().timestamp(), ..Default::default() };
        let mut errors: Vec<String> = Vec::new();
        let (mut files_before, mut bytes_before) = (0usize, 0u64);
        for p in &planned {
            if cancel.load(Ordering::Relaxed) {
                last.cancelled = true;
                break;
            }
            let name = p.name.clone();
            let handle = app.clone();
            let (fb, bb) = (files_before, bytes_before);
            let cancel2 = Arc::clone(&cancel);
            let r = sync::execute_sync(&p.plan, |pr| {
                if cancel2.load(Ordering::Relaxed) {
                    sync::SYNC_CANCELLED.store(true, Ordering::SeqCst);
                }
                let _ = handle.emit(
                    "backup-progress",
                    BackupProgress {
                        library: name.clone(),
                        done: fb + pr.current,
                        total: total_files,
                        bytes_done: bb + pr.bytes_done,
                        bytes_total: total_bytes,
                        current: pr.current_file.clone(),
                    },
                );
            });
            files_before += p.plan.files_to_copy.len() + p.plan.files_to_update.len();
            bytes_before += p.plan.total_bytes;
            last.copied += r.files_copied;
            last.updated += r.files_updated;
            last.bytes += r.bytes_transferred;
            errors.extend(r.errors);
            if r.cancelled {
                last.cancelled = true;
                break;
            }
            let _ = app.emit(
                "backup-progress",
                BackupProgress {
                    library: format!("{} — 확인 중", p.name),
                    done: files_before,
                    total: total_files,
                    bytes_done: bytes_before,
                    bytes_total: total_bytes,
                    current: String::new(),
                },
            );
            verify(&p.plan, &mut errors);
        }
        last.errors = errors.len();
        for e in errors.iter().take(20) {
            log::warn!("백업: {e}");
        }
        let _ = settings::set(&db, KEY_LAST, &serde_json::to_string(&last).unwrap());
        let _ = app.emit("backup-done", &last);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(id: i64, name: &str) -> libraries::Library {
        libraries::Library {
            id,
            volume_uuid: "V".into(),
            volume_name: "v".into(),
            rel_path: String::new(),
            name: name.into(),
            area: 1,
            online: true,
            dir: None,
            file_count: 0,
        }
    }

    #[test]
    fn same_named_libraries_get_their_id_in_the_backup_folder() {
        let all = vec![lib(1, "사진"), lib(2, "사진"), lib(3, "영상")];
        assert_eq!(dest_name(&all[0], &all), "사진 (1)");
        assert_eq!(dest_name(&all[2], &all), "영상");
    }

    #[test]
    fn plan_copies_new_files_and_skips_excluded_ones_then_verify_passes() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("src"), d.path().join("dst"));
        std::fs::create_dir_all(src.join("2024/여행")).unwrap();
        std::fs::create_dir_all(src.join(".acut/thumbs")).unwrap();
        std::fs::write(src.join("2024/여행/a.jpg"), b"aaaa").unwrap();
        std::fs::write(src.join("2024/여행/b.CR2"), b"bbbbbbbb").unwrap();
        std::fs::write(src.join(".acut/thumbs/x.jpg"), b"cache").unwrap();
        std::fs::write(src.join(".DS_Store"), b"junk").unwrap();
        let l = lib(1, "사진");
        let plan = sync::plan_sync(&task_for(&l, &src, &dst)).unwrap();
        let mut names: Vec<String> = plan.files_to_copy.iter().map(|f| f.source.rsplit('/').next().unwrap().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["a.jpg", "b.CR2"]);
        let r = sync::execute_sync(&plan, |_| {});
        assert_eq!((r.files_copied, r.errors.len()), (2, 0));
        let mut errs = Vec::new();
        assert_eq!(verify(&plan, &mut errs), 0);
        assert_eq!(std::fs::read(dst.join("2024/여행/b.CR2")).unwrap(), b"bbbbbbbb");
        // 두 번째 계획은 비어 있다 — 증분
        let again = sync::plan_sync(&task_for(&l, &src, &dst)).unwrap();
        assert_eq!(again.files_to_copy.len() + again.files_to_update.len(), 0);
    }

    #[test]
    fn verify_removes_a_corrupt_copy_so_the_next_run_redoes_it() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("src"), d.path().join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a.jpg"), b"good").unwrap();
        std::fs::write(dst.join("a.jpg"), b"go").unwrap(); // 반만 써진 사본
        let plan = SyncPlan {
            files_to_copy: vec![sync::SyncFileOp {
                source: src.join("a.jpg").to_string_lossy().into_owned(),
                target: dst.join("a.jpg").to_string_lossy().into_owned(),
                file_size: 4,
                reason: "new".into(),
            }],
            files_to_update: vec![],
            conflicts: vec![],
            orphan_files: vec![],
            total_bytes: 4,
            total_files: 1,
        };
        let mut errs = Vec::new();
        assert_eq!(verify(&plan, &mut errs), 1);
        assert!(!dst.join("a.jpg").exists());
        assert_eq!(errs.len(), 1);
    }
}
