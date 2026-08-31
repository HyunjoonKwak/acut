//! 긴 일은 한 번에 하나 — 스캔·썸네일·가져오기·AI 벡터.
//!
//! 스위치(`running`)를 켜고, 일이 끝나면(패닉해도) 끈다. 켜는 김에 macOS에
//! «사용자가 시킨 일»이라고 알린다. 안 그러면 창이 뒤로 가는 순간 App Nap이
//! 앱을 느리게 한다 — 벡터 만들기가 초당 34장에서 20장으로 떨어졌다. 맥이
//! 잠들지도 않게 한다: 20분짜리 일을 시켜 놓고 자리를 비웠는데 맥이 자
//! 버리면 돌아와서 다시 눌러야 한다.

use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::rc::Retained;
use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// 일이 도는 동안 들고 있는다. 떨어뜨리면 스위치가 꺼지고 활동이 끝난다.
pub struct JobGuard {
    running: Arc<AtomicBool>,
    _activity: Activity,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        emit_holder(None);
    }
}

/// 스위치를 켠다. 이미 켜져 있으면 None — 두 벌이 같은 캐시에 쓰지 않게.
pub fn try_start(running: &Arc<AtomicBool>, reason: &str) -> Option<JobGuard> {
    try_start_with(running, reason, false)
}

/// «사용자가 기다리는 중» 깃발 — 감시(폴더 재스캔·썸네일)가 이걸 보면 하던 판을 멈추고 양보한다.
/// 감시가 썸네일을 만드는 동안 스위치를 몇 분씩 쥐면 20초 대기로도 «다른 작업이 도는 중»으로
/// 튕긴다 (2026-08-31 «영구히 비우기 실패»). 썸네일의 cancel 로도 그대로 쓴다.
static WAITING: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// 스위치 상태를 프론트로 — «표시 없이 도는 작업»이 다시 생기지 않게, 잡는 쪽이 아니라
/// 스위치 자체가 알린다 (2026-08-31 «감시가 돈다는데 상단 바엔 아무 표시도 없었어»)
static EMITTER: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn set_emitter(h: tauri::AppHandle) {
    let _ = EMITTER.set(h);
}

fn emit_holder(reason: Option<&str>) {
    if let Some(h) = EMITTER.get() {
        let _ = tauri::Emitter::emit(h, "switch-busy", reason);
    }
}

pub fn waiting() -> Arc<AtomicBool> {
    Arc::clone(WAITING.get_or_init(|| Arc::new(AtomicBool::new(false))))
}

/// 사용자 명령용 — 스위치가 잡혀 있으면 기다렸다가 잡는다. 기다리는 동안 깃발을 올려
/// 감시가 다음 폴더로 넘어가지 않고 양보하게 한다.
/// 바로 «다른 작업이 도는 중»으로 튕기면 큰 이동 뒤 감시가 훑는 몇 분 동안 아무것도 못 한다 (실측 2026-08-30)
pub fn try_start_wait(running: &Arc<AtomicBool>, reason: &str, wait: std::time::Duration) -> Option<JobGuard> {
    let flag = waiting();
    flag.store(true, Ordering::Release);
    let until = std::time::Instant::now() + wait;
    let out = loop {
        if let Some(g) = try_start(running, reason) {
            break Some(g);
        }
        if std::time::Instant::now() >= until {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    flag.store(false, Ordering::Release);
    out
}

/// 뒤에서 도는 일(폴더 감시)용 — 잠자기를 막지 않는다. 사용자가 시킨 일은 `UserInitiated`
/// 로 시스템 잠자기를 미루지만, 감시가 그걸 쓰면 맥이 밤새 못 잔다 (리뷰 H15)
pub fn try_start_with(running: &Arc<AtomicBool>, reason: &str, background: bool) -> Option<JobGuard> {
    if running.swap(true, Ordering::AcqRel) {
        return None;
    }
    emit_holder(Some(reason));
    Some(JobGuard { running: Arc::clone(running), _activity: Activity::begin(reason, background) })
}

struct Activity {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

// NSProcessInfo의 활동 토큰은 어느 스레드에서 끝내도 된다 (Apple 문서).
// 일은 명령을 받은 스레드에서 시작해 작업 스레드에서 끝난다.
unsafe impl Send for Activity {}

impl Activity {
    fn begin(reason: &str, background: bool) -> Self {
        let info = NSProcessInfo::processInfo();
        let opts = if background { NSActivityOptions::Background } else { NSActivityOptions::UserInitiated };
        let token = info.beginActivityWithOptions_reason(opts, &NSString::from_str(reason));
        Activity { token }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        // 끝내기는 unsafe로 표시돼 있다 — 시작한 토큰만 넘기면 된다
        unsafe { NSProcessInfo::processInfo().endActivity(&self.token) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_start_is_refused_until_the_first_is_dropped() {
        let running = Arc::new(AtomicBool::new(false));
        let a = try_start(&running, "시험").expect("첫 번째는 켜진다");
        assert!(try_start(&running, "시험").is_none());
        drop(a);
        assert!(!running.load(Ordering::Acquire));
        assert!(try_start(&running, "시험").is_some());
    }

    #[test]
    fn guard_moves_to_another_thread() {
        let running = Arc::new(AtomicBool::new(false));
        let g = try_start(&running, "시험").unwrap();
        std::thread::spawn(move || drop(g)).join().unwrap();
        assert!(!running.load(Ordering::Acquire));
    }
}
