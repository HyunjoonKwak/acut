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
use std::sync::Arc;

/// 일이 도는 동안 들고 있는다. 떨어뜨리면 스위치가 꺼지고 활동이 끝난다.
pub struct JobGuard {
    running: Arc<AtomicBool>,
    _activity: Activity,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
    }
}

/// 스위치를 켠다. 이미 켜져 있으면 None — 두 벌이 같은 캐시에 쓰지 않게.
pub fn try_start(running: &Arc<AtomicBool>, reason: &str) -> Option<JobGuard> {
    if running.swap(true, Ordering::AcqRel) {
        return None;
    }
    Some(JobGuard { running: Arc::clone(running), _activity: Activity::begin(reason) })
}

struct Activity {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

// NSProcessInfo의 활동 토큰은 어느 스레드에서 끝내도 된다 (Apple 문서).
// 일은 명령을 받은 스레드에서 시작해 작업 스레드에서 끝난다.
unsafe impl Send for Activity {}

impl Activity {
    fn begin(reason: &str) -> Self {
        let info = NSProcessInfo::processInfo();
        let token = info.beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiated,
            &NSString::from_str(reason),
        );
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
