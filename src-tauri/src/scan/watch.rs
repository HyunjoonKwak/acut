//! 폴더 감시 — 파인더로 넣거나 지운 사진이 앱에 저절로 나타나고 사라진다.
//!
//! 라이브러리마다 FSEvents 감시를 하나 건다. 알림은 파일 단위로 쏟아지는데
//! (복사 한 번에 create·modify가 여러 번) 그때마다 스캔하면 디스크만 긁는다.
//! 그래서 **폴더 단위로 모아 두었다가 잠잠해지면** 그 폴더만 다시 스캔한다.
//! 사용자가 시작한 스캔이 도는 중이면 끝날 때까지 미룬다 — 같은 DB·캐시에
//! 둘이 쓰면 안 된다.

use crate::db::conn::Db;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 마지막 알림 뒤 이만큼 잠잠해야 스캔한다. 큰 파일 복사가 끝나기를 기다린다.
pub const QUIET: Duration = Duration::from_millis(1500);
/// 미룬 것을 얼마나 자주 다시 보나
const TICK: Duration = Duration::from_millis(500);

/// 한 폴더에서 무엇이 달라졌나 — 프론트로 보낸다.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Changed {
    pub library_id: i64,
    pub inserted: usize,
    pub updated: usize,
    pub removed: usize,
}

/// 감시 중인 라이브러리 하나
struct Live {
    _watcher: RecommendedWatcher,
}

/// 모아 둔 «다시 볼 폴더»들. (라이브러리, 폴더) → 마지막 알림 시각
type Pending = Arc<Mutex<HashMap<(i64, PathBuf), Instant>>>;

pub struct Watchers {
    live: Mutex<HashMap<i64, Live>>,
    pending: Pending,
    /// 처리 스레드는 한 번만 띄운다
    started: AtomicBool,
}

impl Default for Watchers {
    fn default() -> Self {
        Self {
            live: Mutex::new(HashMap::new()),
            pending: Arc::new(Mutex::new(HashMap::new())),
            started: AtomicBool::new(false),
        }
    }
}

/// 이 알림이 우리가 볼 것인가. 캐시·휴지통·곁가지·임시 파일은 아니다.
pub fn interesting(path: &Path) -> bool {
    for c in path.components() {
        let s = c.as_os_str().to_string_lossy();
        if s == ".acut" || s == ".Trashes" || s == ".Spotlight-V100" || s == ".fseventsd" {
            return false;
        }
    }
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    if name.starts_with("._") || name == ".DS_Store" {
        return false;
    }
    // 확장자 없는 것(폴더)은 본다 — 폴더가 통째로 들어올 수 있다
    match path.extension() {
        None => true,
        Some(_) => crate::scan::kinds::classify(&name).is_some(),
    }
}

/// 알림을 «다시 볼 폴더»로 바꾼다. 파일이면 그 부모, 폴더면 자기 자신.
pub fn folder_of(path: &Path) -> Option<PathBuf> {
    if path.extension().is_none() {
        Some(path.to_path_buf())
    } else {
        path.parent().map(Path::to_path_buf)
    }
}

/// 잠잠해진 것들을 꺼낸다. 아직 알림이 오고 있는 폴더는 남긴다.
pub fn take_due(
    pending: &mut HashMap<(i64, PathBuf), Instant>,
    now: Instant,
    quiet: Duration,
) -> Vec<(i64, PathBuf)> {
    let due: Vec<_> = pending
        .iter()
        .filter(|(_, t)| now.duration_since(**t) >= quiet)
        .map(|(k, _)| k.clone())
        .collect();
    for k in &due {
        pending.remove(k);
    }
    due
}

impl Watchers {
    /// 이 라이브러리를 감시한다. 이미 하고 있으면 그대로.
    pub fn start(&self, library_id: i64, dir: &Path) -> Result<(), String> {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if live.contains_key(&library_id) {
            return Ok(());
        }
        let pending = Arc::clone(&self.pending);
        let mut w = RecommendedWatcher::new(
            move |r: Result<Event, notify::Error>| {
                let Ok(ev) = r else { return };
                if !matches!(
                    ev.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) {
                    return;
                }
                let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
                for path in ev.paths {
                    if !interesting(&path) {
                        continue;
                    }
                    if let Some(f) = folder_of(&path) {
                        p.insert((library_id, f), Instant::now());
                    }
                }
            },
            notify::Config::default(),
        )
        .map_err(|e| e.to_string())?;
        w.watch(dir, RecursiveMode::Recursive).map_err(|e| e.to_string())?;
        live.insert(library_id, Live { _watcher: w });
        Ok(())
    }

    pub fn stop(&self, library_id: i64) {
        self.live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&library_id);
    }

    pub fn stop_all(&self) {
        self.live.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    pub fn watching(&self) -> Vec<i64> {
        self.live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect()
    }

    /// 처리 스레드 — 잠잠해진 폴더를 다시 스캔한다. 한 번만 뜬다.
    ///
    /// `running`은 사용자 스캔과 같은 스위치다. 그쪽이 돌면 미룬다.
    pub fn run(
        &self,
        db: Arc<Db>,
        cache_base: PathBuf,
        running: Arc<AtomicBool>,
        cancel: Arc<AtomicBool>,
        on_changed: impl Fn(Changed) + Send + 'static,
        // 남은 폴더 수(0 = 끝) — 조용히 돌면 «이미 스캔 중»이 왜 뜨는지 보이지 않고,
        // 수가 줄어드는 게 보여야 «오래 걸린다»가 «밀린 폴더를 하나씩 처리 중»으로 읽힌다 (2026-08-31)
        on_busy: impl Fn(usize) + Send + 'static,
    ) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.pending);
        std::thread::Builder::new()
            .name("acut-watch".into())
            .spawn(move || loop {
                std::thread::sleep(TICK);
                let due = {
                    let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
                    take_due(&mut p, Instant::now(), QUIET)
                };
                if due.is_empty() {
                    continue;
                }
                // 이번 판 + 아직 잠잠해지지 않은 것까지가 «남은 일»이다
                let backlog = pending.lock().unwrap_or_else(|e| e.into_inner()).len();
                let mut remaining = due.len() + backlog;
                on_busy(remaining);
                // 스위치는 JobGuard 로 잡는다 — 직접 켜고 끄면 스캔 중 패닉 한 번에 꺼지지
                // 않아 모든 작업이 «다른 일이 도는 중»으로 영영 막힌다 (리뷰 H12).
                // **폴더 하나마다** 잡았다 놓는다 — 3만 장을 옮긴 뒤 수천 폴더를 훑는 동안 통째로
                // 쥐고 있으면 그동안 휴지통으로·정리 같은 사용자 명령이 전부 튕긴다 (실측 2026-08-30)
                let mut rest = due.into_iter();
                let user_waiting = crate::api::job::waiting();
                while let Some((library_id, dir)) = rest.next() {
                    // 사용자 명령이 스위치를 기다리는 중이면 이번 판을 접고 양보한다 —
                    // 감시는 다음 틱에 이어서 한다 (2026-08-31 «영구히 비우기 실패»)
                    if user_waiting.load(Ordering::Acquire) {
                        let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
                        p.entry((library_id, dir)).or_insert_with(Instant::now);
                        for k in rest {
                            p.entry(k).or_insert_with(Instant::now);
                        }
                        break;
                    }
                    let Some(_guard) = crate::api::job::try_start_with(&running, "폴더 감시", true) else {
                        // 사용자 일이 도는 중 — 이것과 나머지를 되돌려 놓고 다음 틱에
                        let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
                        p.entry((library_id, dir)).or_insert_with(Instant::now);
                        for k in rest {
                            p.entry(k).or_insert_with(Instant::now);
                        }
                        break;
                    };
                    if let Some(c) = rescan_dir(&db, &cache_base, library_id, &dir, &cancel) {
                        on_changed(c);
                    }
                    remaining = remaining.saturating_sub(1);
                    on_busy(remaining);
                }
                on_busy(0);
            })
            .expect("감시 스레드");
    }
}

/// 폴더 하나를 다시 훑는다 — 새것은 넣고, 사라진 것은 빼고, 썸네일을 만든다.
fn rescan_dir(
    db: &Db,
    cache_base: &Path,
    library_id: i64,
    dir: &Path,
    cancel: &Arc<AtomicBool>,
) -> Option<Changed> {
    let lib = crate::db::libraries::get(db, library_id).ok()??;
    let mount = crate::db::volumes::find_mount(&lib.volume_uuid)?;
    let mut out = Changed { library_id, inserted: 0, updated: 0, removed: 0 };

    // 폴더가 안 보이면(이름을 바꿨거나 디스크가 잠깐 빠짐) 아무것도 지우지 않는다.
    // 옛 경로로 온 알림에 행을 지우면 별점·판정·태그·코멘트가 통째로 사라진다 (리뷰 C2).
    // 진짜 없어진 파일은 다음 전체 스캔이 정리한다.
    // 달라진 것이 없으니 알리지도 않는다 — Some 을 돌려주면 알림마다 «library-changed»가
    // 나가 사라진 폴더 하나에 화면이 새로고침 폭풍을 맞는다 (리뷰 H6)
    if !dir.is_dir() {
        return None;
    }
    match crate::scan::scan_folder(db, library_id, dir, lib.area, |_| {}) {
        Ok(p) => {
            out.inserted = p.inserted;
            out.updated = p.updated;
        }
        Err(e) => log::warn!("감시 스캔 실패 {}: {e}", dir.display()),
    }
    // 마운트 이름이 바뀌어 접두사가 안 맞으면 rel이 ""가 되어 볼륨 전체를 정리해 버린다 — 하지 않는다
    let Ok(rel) = dir.strip_prefix(&mount).map(|r| r.to_string_lossy().into_owned()) else {
        return (out.inserted + out.updated > 0).then_some(out);
    };
    out.removed = crate::scan::prune_missing(db, &mount, library_id, &rel).unwrap_or(0);

    if out.inserted > 0 && !cancel.load(Ordering::Relaxed) {
        // 멈춤 신호로 «사용자가 기다리는 중» 깃발을 준다 — 썸네일이 몇 분씩 스위치를 쥐고
        // 사용자 명령을 굶기지 않게. 못 만든 썸네일은 다음 판에 이어서 만든다 (재개 가능)
        let cache_root = crate::media::cache::cache_root(cache_base, library_id);
        let _ = crate::scan::thumbs::generate(db, library_id, &mount, &cache_root, crate::api::job::waiting(), |_| {});
    }
    if out.inserted + out.updated + out.removed == 0 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_media_and_folders_are_interesting() {
        assert!(interesting(Path::new("/v/lib/2024/IMG_1.jpg")));
        assert!(interesting(Path::new("/v/lib/2024/새 폴더")));
        assert!(!interesting(Path::new("/v/lib/2024/메모.txt")));
        assert!(!interesting(Path::new("/v/lib/2024/._IMG_1.jpg")));
        assert!(!interesting(Path::new("/v/lib/.DS_Store")));
        assert!(!interesting(Path::new("/v/lib/.acut/휴지통/IMG_1.jpg")));
    }

    #[test]
    fn a_file_points_at_its_folder_and_a_folder_at_itself() {
        assert_eq!(folder_of(Path::new("/a/b/c.jpg")), Some(PathBuf::from("/a/b")));
        assert_eq!(folder_of(Path::new("/a/b")), Some(PathBuf::from("/a/b")));
    }

    /// 복사가 끝나기를 기다린다 — 마지막 알림 뒤 QUIET만큼 잠잠해야 나온다.
    #[test]
    fn folders_come_out_only_after_going_quiet() {
        let t0 = Instant::now();
        let mut p = HashMap::new();
        p.insert((1, PathBuf::from("/a")), t0);
        p.insert((1, PathBuf::from("/b")), t0 + Duration::from_millis(1000));

        let due = take_due(&mut p, t0 + Duration::from_millis(1600), QUIET);
        assert_eq!(due, vec![(1, PathBuf::from("/a"))], "/a만 잠잠하다");
        assert_eq!(p.len(), 1, "/b는 남는다");

        let due = take_due(&mut p, t0 + Duration::from_millis(2600), QUIET);
        assert_eq!(due, vec![(1, PathBuf::from("/b"))]);
        assert!(p.is_empty());
    }

    #[test]
    fn same_folder_hit_twice_is_one_entry() {
        let t0 = Instant::now();
        let mut p = HashMap::new();
        p.insert((1, PathBuf::from("/a")), t0);
        p.insert((1, PathBuf::from("/a")), t0 + Duration::from_millis(500));
        assert_eq!(p.len(), 1);
        assert!(take_due(&mut p, t0 + Duration::from_millis(1000), QUIET).is_empty(), "다시 미뤄졌다");
    }
}
