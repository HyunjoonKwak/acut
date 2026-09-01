//! 고르기 — 남길 것과 뺄 것을 가른다.
//!
//! 갈래마다 대상과 판정 방법이 다르다:
//!   - `dedup`  완전 중복 — 바이트가 같다. 해시로 판정
//!   - `junk`   잡동사니 — 스크린샷·다운로드본 등. 규칙으로 판정
//!   - `burst`  같은 순간 — 연달아 찍은 것. 시간 근접 + 품질로 판정
//!   - `scene`  비슷한 장면 — AI 벡터가 가깝다. 다른 컷을 묶는다
//!   - `phash`  크기만 줄인 사본 — 지각 해시가 거의 같다. **같은 그림**을 묶는다
//!
//! 판정 결과는 `files.culling_flag`에 남는다. 그룹은 작업 단위일 뿐이다.

pub mod apply;
pub mod burst;
pub mod dedup;
pub mod folders;
pub mod hash;
pub mod junk;
pub mod phash;
pub mod scene;

#[cfg(test)]
mod real {
    use std::path::Path;
    use std::sync::{atomic::AtomicBool, Arc};

    /// `cargo test --release --lib cull::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 라이브러리 전체"]
    fn cull_the_whole_library() {
        let root = Path::new("/Volumes/MAIN SSD/MERGE/사진통합작업");
        if !root.is_dir() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let db = crate::db::conn::Db::open(tmp.path().join("t.db")).unwrap();
        let t = std::time::Instant::now();
        crate::scan::scan_test(&db, root, 1, |_| {}).unwrap();
        println!("\n스캔 {:.0}초", t.elapsed().as_secs_f64());

        // ── 잡동사니 (파일을 열지 않는다) ─────────────────────────
        let t = std::time::Instant::now();
        let j = super::junk::scan(&db).unwrap();
        println!("\n═══ 잡동사니 ═══  {:.2}초", t.elapsed().as_secs_f64());
        println!("  {}장 · {:.1} GB", j.found, j.bytes as f64 / 1024.0f64.powi(3));
        for (r, n) in &j.by_reason {
            println!("    {r:<16} {n:>6}");
        }

        // ── 같은 순간 ─────────────────────────────────────────────
        let t = std::time::Instant::now();
        let b = super::burst::scan(&db, super::burst::DEFAULT_GAP_SECS).unwrap();
        println!("\n═══ 같은 순간 ═══  {:.2}초", t.elapsed().as_secs_f64());
        println!("  {}그룹 · {}장 · 확보 {:.1} GB",
                 b.groups, b.photos, b.reclaimable as f64 / 1024.0f64.powi(3));

        // ── 완전 중복 (해시를 읽는다) ──────────────────────────────
        let t = std::time::Instant::now();
        let d = super::dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        println!("\n═══ 완전 중복 ═══  {:.1}초", t.elapsed().as_secs_f64());
        println!("  후보 {} · {}그룹 · 확보 {:.1} GB",
                 d.candidates, d.groups, d.reclaimable as f64 / 1024.0f64.powi(3));

        let total = j.bytes + b.reclaimable + d.reclaimable;
        println!("\n  ── 합계 확보 가능 {:.1} GB ──\n", total as f64 / 1024.0f64.powi(3));
    }
}
