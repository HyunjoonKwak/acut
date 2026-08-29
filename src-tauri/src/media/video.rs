//! 영상 — 촬영 시각·길이·해상도와 대표 프레임.
//!
//! ImageIO는 영상을 열지 못한다. 그래서 지금까지 2,828개(486GB, 라이브러리
//! 용량의 46%)가 촬영일도 추정값이고 썸네일도 빈 칸이었다.
//!
//! macOS에는 이걸 **C API로** 할 수 있는 길이 둘 있다:
//!   - `MDItemCopyAttribute` (Spotlight) — 촬영일·길이·해상도
//!   - `QLThumbnailImageCreate` (QuickLook) — 대표 프레임 한 장
//!
//! AVFoundation이 정석이지만 Objective-C 메시징이 필요하다. 이 프로젝트에는
//! objc 바인딩이 없고, 위 둘로 필요한 것이 다 나온다.
//!
//! Spotlight는 색인이 꺼진 볼륨에서 아무것도 주지 않는다. 그때는 값이 없는
//! 채로 돌아오고, 촬영일은 기존 폴백 체인(파일명 → 파일시각)이 맡는다.

use crate::media::thumbnail::{ThumbError, ThumbSize};
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation::url::CFURL;
use std::os::raw::c_void;
use std::path::Path;

mod ffi {
    use super::c_void;
    pub use core_foundation::base::CFTypeRef;
    pub use core_foundation::string::CFStringRef;

    pub type MDItemRef = *const c_void;
    pub type CGImageRef = *const c_void;
    pub type CFURLRef = *const c_void;
    pub type CFDictionaryRef = *const c_void;
    pub type CFAllocatorRef = *const c_void;

    #[repr(C)]
    pub struct CGSize {
        pub width: f64,
        pub height: f64,
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        pub fn MDItemCreate(allocator: CFAllocatorRef, path: CFStringRef) -> MDItemRef;
        pub fn MDItemCopyAttribute(item: MDItemRef, name: CFStringRef) -> CFTypeRef;

        pub static kMDItemContentCreationDate: CFStringRef;
        pub static kMDItemDurationSeconds: CFStringRef;
        pub static kMDItemPixelWidth: CFStringRef;
        pub static kMDItemPixelHeight: CFStringRef;
    }

    // 10.15부터 deprecated지만 여전히 동작한다. 대안(QLThumbnailGenerator)은
    // Objective-C 전용이라 지금 구조에서는 쓸 수 없다.
    #[link(name = "QuickLook", kind = "framework")]
    extern "C" {
        pub fn QLThumbnailImageCreate(
            allocator: CFAllocatorRef,
            url: CFURLRef,
            size: CGSize,
            options: CFDictionaryRef,
        ) -> CGImageRef;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGImageGetWidth(image: CGImageRef) -> usize;
        pub fn CGImageGetHeight(image: CGImageRef) -> usize;
        pub fn CGImageRelease(image: CGImageRef);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFRelease(cf: CFTypeRef);
        pub fn CFGetTypeID(cf: CFTypeRef) -> usize;
        pub fn CFDateGetTypeID() -> usize;
        pub fn CFNumberGetTypeID() -> usize;
        pub fn CFDateGetAbsoluteTime(date: CFTypeRef) -> f64;
        pub fn CFNumberGetValue(n: CFTypeRef, ty: i32, out: *mut c_void) -> bool;
    }
}

/// CFAbsoluteTime의 기준(2001-01-01)과 유닉스 기준의 차이.
const ABSOLUTE_TIME_EPOCH: f64 = 978_307_200.0;
/// `kCFNumberDoubleType`
const CF_NUMBER_DOUBLE: i32 = 13;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoMeta {
    /// 촬영 시각 (유닉스 초). Spotlight 색인이 없으면 None.
    pub taken_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

/// 값 하나를 꺼내 f64로. 타입이 예상과 다르면 None.
///
/// 타입 확인이 필수다 — 확인 없이 형변환하면 다른 타입에서 프로세스가 죽는다.
/// 사진 EXIF 쪽에서 실제로 겪었다.
fn number(item: ffi::MDItemRef, key: ffi::CFStringRef) -> Option<f64> {
    let v = unsafe { ffi::MDItemCopyAttribute(item, key) };
    if v.is_null() {
        return None;
    }
    let out = unsafe {
        let id = ffi::CFGetTypeID(v);
        if id == ffi::CFDateGetTypeID() {
            Some(ffi::CFDateGetAbsoluteTime(v) + ABSOLUTE_TIME_EPOCH)
        } else if id == ffi::CFNumberGetTypeID() {
            let mut d: f64 = 0.0;
            ffi::CFNumberGetValue(v, CF_NUMBER_DOUBLE, &mut d as *mut f64 as *mut c_void)
                .then_some(d)
        } else {
            None
        }
    };
    unsafe { ffi::CFRelease(v) };
    out
}

/// 영상 메타데이터를 읽는다. Spotlight 색인이 없으면 전부 None이다.
pub fn probe(path: impl AsRef<Path>) -> VideoMeta {
    let Some(s) = path.as_ref().to_str() else {
        return VideoMeta::default();
    };
    let cf = CFString::new(s);
    let item = unsafe { ffi::MDItemCreate(std::ptr::null(), cf.as_concrete_TypeRef() as _) };
    if item.is_null() {
        return VideoMeta::default();
    }
    // 촬영 시각은 컨테이너에 박힌 것이 먼저 — Spotlight는 색인이 없으면 파일
    // 시스템의 생성 시각(복사한 날)을 돌려준다 (실측: 영상 194개가 전부 같은 날).
    let embedded = super::mp4date::creation_time(path.as_ref());
    let meta = unsafe {
        VideoMeta {
            taken_at: embedded.or_else(|| number(item, ffi::kMDItemContentCreationDate).map(|t| t as i64)),
            duration_ms: number(item, ffi::kMDItemDurationSeconds).map(|d| (d * 1000.0) as i64),
            width: number(item, ffi::kMDItemPixelWidth).map(|v| v as i64),
            height: number(item, ffi::kMDItemPixelHeight).map(|v| v as i64),
        }
    };
    unsafe { ffi::CFRelease(item) };
    meta
}

/// 대표 프레임 한 장을 JPEG으로 뽑는다.
///
/// QuickLook이 형식마다 알아서 프레임을 고른다 (보통 앞부분의 의미 있는 장면).
/// 우리가 시각을 지정할 수는 없지만, 그리드에 뭐가 찍혔는지 보이는 것이 목적이라
/// 충분하다.
pub fn thumbnail(
    src: impl AsRef<Path>,
    out: impl AsRef<Path>,
    max_px: u32,
    quality: f64,
) -> crate::media::thumbnail::Result<ThumbSize> {
    let src = src.as_ref();
    let url = CFURL::from_path(src, false).ok_or(ThumbError::BadPath)?;
    let size = ffi::CGSize {
        width: max_px as f64,
        height: max_px as f64,
    };
    let image = unsafe {
        ffi::QLThumbnailImageCreate(
            std::ptr::null(),
            url.as_concrete_TypeRef() as _,
            size,
            std::ptr::null(),
        )
    };
    if image.is_null() {
        return Err(ThumbError::NoThumbnail);
    }
    let dims = unsafe {
        ThumbSize {
            width: ffi::CGImageGetWidth(image) as u32,
            height: ffi::CGImageGetHeight(image) as u32,
        }
    };
    let r = crate::media::thumbnail::write_jpeg(image, out.as_ref(), quality);
    unsafe { ffi::CGImageRelease(image) };
    r?;
    Ok(dims)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 라이브러리에서 영상 하나를 찾는다. 없으면 건너뛴다.
    fn sample(ext: &str) -> Option<std::path::PathBuf> {
        for root in ["/Volumes/PHOTO 1", "/Volumes/MAIN SSD/MERGE/사진통합작업"] {
            let root = Path::new(root);
            if !root.is_dir() {
                continue;
            }
            let mut stack = vec![root.to_path_buf()];
            let mut seen = 0;
            while let Some(d) = stack.pop() {
                seen += 1;
                if seen > 600 {
                    break;
                }
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        if p.file_name().is_some_and(|n| n == ".acut") {
                            continue;
                        }
                        stack.push(p);
                    } else if p
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| x.eq_ignore_ascii_case(ext))
                    {
                        return Some(p);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn probe_reads_creation_time_and_size() {
        let Some(src) = sample("mp4") else { return };
        let m = probe(&src);
        // Spotlight 색인이 꺼져 있을 수 있다 — 그때는 전부 None이 정상이다
        if m.taken_at.is_none() && m.width.is_none() {
            eprintln!("Spotlight 색인 없음 — 건너뜀: {}", src.display());
            return;
        }
        if let Some(t) = m.taken_at {
            assert!(t > 946_684_800, "2000년 이후여야 한다: {t}");
            assert!(t < 4_102_444_800, "너무 먼 미래: {t}");
        }
        if let Some(w) = m.width {
            assert!((16..=16_384).contains(&w), "해상도: {w}");
        }
        if let Some(d) = m.duration_ms {
            assert!(d > 0, "길이는 양수: {d}");
        }
    }

    #[test]
    fn probe_of_a_missing_file_is_empty_not_a_crash() {
        assert_eq!(probe("/no/such/video.mp4"), VideoMeta::default());
    }

    #[test]
    fn makes_a_frame_from_a_video() {
        let Some(src) = sample("mp4") else { return };
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("v.jpg");
        let Ok(size) = thumbnail(&src, &out, 640, 0.72) else {
            eprintln!("QuickLook이 프레임을 못 뽑음 — 건너뜀");
            return;
        };
        assert!(out.is_file(), "파일이 만들어져야 한다");
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
        assert!(size.width > 0 && size.height > 0);
        assert!(size.width <= 640 && size.height <= 640, "{size:?}");
    }

    #[test]
    fn a_broken_video_fails_without_crashing() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("broken.mp4");
        std::fs::write(&bad, b"not a video at all").unwrap();
        // 실패해도 되고 무언가 뽑아도 되지만, 죽으면 안 된다
        let _ = thumbnail(&bad, dir.path().join("o.jpg"), 320, 0.7);
        let _ = probe(&bad);
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// 실제 라이브러리의 영상으로 처리 속도와 성공률을 잰다.
    /// `cargo test --release --lib media::video::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 라이브러리 필요"]
    fn throughput_on_real_videos() {
        let db = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join("Library/Application Support/com.acut.media/acut-v2.db");
        if !db.is_file() {
            return;
        }
        let conn = rusqlite::Connection::open_with_flags(
            &db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let paths: Vec<String> = {
            let mut st = conn
                .prepare(
                    "SELECT v.last_mount_path || '/' || fo.rel_path || '/' || fi.name
                     FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                     JOIN libraries l ON l.id = fo.library_id
                     JOIN volumes v ON v.uuid = l.volume_uuid
                     WHERE fi.kind = 1 ORDER BY RANDOM() LIMIT 40",
                )
                .unwrap();
            let it = st.query_map([], |r| r.get(0)).unwrap();
            it.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        let paths: Vec<&String> = paths.iter().filter(|p| Path::new(p).is_file()).collect();
        if paths.is_empty() {
            eprintln!("영상 없음 — 건너뜀");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let (mut ok, mut dated, mut fail) = (0, 0, 0);
        let t = std::time::Instant::now();
        for (i, p) in paths.iter().enumerate() {
            if probe(p).taken_at.is_some() {
                dated += 1;
            }
            match thumbnail(p, dir.path().join(format!("{i}.jpg")), 640, 0.72) {
                Ok(_) => ok += 1,
                Err(_) => fail += 1,
            }
        }
        let s = t.elapsed().as_secs_f64();
        println!(
            "\n영상 {}개 · {:.1}초 · {:.1}개/초\n  프레임 성공 {ok} 실패 {fail}\n  Spotlight 촬영일 {dated}/{}",
            paths.len(),
            s,
            paths.len() as f64 / s,
            paths.len()
        );
        assert!(ok > 0, "적어도 몇 개는 프레임이 나와야 한다");
    }
}
