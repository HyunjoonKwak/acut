//! 썸네일 생성 — macOS ImageIO를 직접 호출한다.
//!
//! 왜 바꾸는가: 이전 구현은 파일 하나마다 `sips` 프로세스를 fork했다.
//! 6만 5천 장이면 프로세스가 6만 5천 개다. rayon으로 병렬화해도 프로세스 생성
//! 비용 자체가 상한을 만든다.
//!
//! `sips`가 하는 일이 바로 ImageIO를 부르는 것이므로, 같은 API를 인프로세스로
//! 호출하면 된다. 덤으로 얻는 것들:
//!   - CR2·NEF·ARW·DNG·HEIC를 **시스템이 이미 지원**한다 (libraw를 링크할 필요 없음)
//!   - `kCGImageSourceThumbnailMaxPixelSize`를 주면 디코더가 **축소본만 만든다**.
//!     6000×4000 원본을 전부 펼치지 않는다.
//!   - RAW 파일에 들어 있는 내장 미리보기를 알아서 활용한다.
//!   - EXIF 방향을 `WithTransform`으로 적용해 회전된 사진이 바로 선다.

#![cfg(target_os = "macos")]

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation::url::CFURL;
use std::path::Path;

pub(crate) mod ffi {
    use core_foundation::base::CFTypeRef;
    use core_foundation::dictionary::CFDictionaryRef;
    use core_foundation::string::CFStringRef;
    use core_foundation::url::CFURLRef;
    use std::os::raw::c_void;

    pub type CGImageSourceRef = *const c_void;
    pub type CGImageDestinationRef = *const c_void;
    pub type CGImageRef = *const c_void;

    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        pub fn CGImageSourceCreateWithURL(
            url: CFURLRef,
            options: CFDictionaryRef,
        ) -> CGImageSourceRef;
        pub fn CGImageSourceCreateThumbnailAtIndex(
            src: CGImageSourceRef,
            index: usize,
            options: CFDictionaryRef,
        ) -> CGImageRef;
        pub fn CGImageSourceCopyPropertiesAtIndex(
            src: CGImageSourceRef,
            index: usize,
            options: CFDictionaryRef,
        ) -> CFDictionaryRef;
        pub fn CGImageDestinationCreateWithURL(
            url: CFURLRef,
            ty: CFStringRef,
            count: usize,
            options: CFDictionaryRef,
        ) -> CGImageDestinationRef;
        pub fn CGImageDestinationAddImage(
            dst: CGImageDestinationRef,
            image: CGImageRef,
            props: CFDictionaryRef,
        );
        pub fn CGImageDestinationFinalize(dst: CGImageDestinationRef) -> bool;
        /// 0 = kCGImageStatusComplete. 음수는 손상·미완성·형식 불일치.
        pub fn CGImageSourceGetStatus(src: CGImageSourceRef) -> i32;
        pub fn CGImageSourceGetCount(src: CGImageSourceRef) -> usize;

        pub static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
        pub static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
        pub static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
        pub static kCGImageSourceShouldCache: CFStringRef;
        pub static kCGImageDestinationLossyCompressionQuality: CFStringRef;
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
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ThumbError {
    #[error("경로를 URL로 바꿀 수 없습니다")]
    BadPath,
    #[error("이미지를 열 수 없습니다 (지원하지 않는 형식이거나 손상됨)")]
    Unreadable,
    #[error("썸네일을 만들 수 없습니다")]
    NoThumbnail,
    #[error("썸네일을 저장할 수 없습니다")]
    WriteFailed,
    #[error("저장 폴더를 만들 수 없습니다: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ThumbError>;

/// 만들어진 썸네일의 크기.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbSize {
    pub width: u32,
    pub height: u32,
}

/// RAII 래퍼 — 중간에 에러로 빠져나가도 CFRelease가 불린다.
struct Owned<T: Copy>(T, fn(T));
impl<T: Copy> Drop for Owned<T> {
    fn drop(&mut self) {
        (self.1)(self.0)
    }
}

fn release_source(p: ffi::CGImageSourceRef) {
    if !p.is_null() {
        unsafe { ffi::CFRelease(p as _) }
    }
}
fn release_image(p: ffi::CGImageRef) {
    if !p.is_null() {
        unsafe { ffi::CGImageRelease(p) }
    }
}
fn release_dest(p: ffi::CGImageDestinationRef) {
    if !p.is_null() {
        unsafe { ffi::CFRelease(p as _) }
    }
}

/// `src`의 썸네일을 만들어 `out`에 JPEG으로 쓴다.
///
/// `max_px`는 긴 변의 최대 픽셀. 원본이 그보다 작으면 확대하지 않는다.
/// `quality`는 0.0~1.0.
pub fn make(
    src: impl AsRef<Path>,
    out: impl AsRef<Path>,
    max_px: u32,
    quality: f64,
) -> Result<ThumbSize> {
    let src = src.as_ref();
    let out = out.as_ref();
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let src_url = CFURL::from_path(src, false).ok_or(ThumbError::BadPath)?;

    // 원본 디코딩 결과를 캐시하지 않는다. 한 번 쓰고 버릴 것이라
    // 캐시하면 대량 스캔에서 메모리만 먹는다.
    let src_opts = unsafe {
        CFDictionary::from_CFType_pairs(&[(
            CFString::wrap_under_get_rule(ffi::kCGImageSourceShouldCache),
            CFType::wrap_under_get_rule(
                core_foundation::boolean::CFBoolean::false_value().as_CFTypeRef(),
            ),
        )])
    };

    let source = unsafe {
        ffi::CGImageSourceCreateWithURL(
            src_url.as_concrete_TypeRef(),
            src_opts.as_concrete_TypeRef(),
        )
    };
    if source.is_null() {
        return Err(ThumbError::Unreadable);
    }
    let _source = Owned(source, release_source as fn(_));
    // 소스 생성 성공 != 디코딩 가능. 확장자만 보고 만들어지기 때문이다.
    if unsafe { ffi::CGImageSourceGetStatus(source) != 0 || ffi::CGImageSourceGetCount(source) == 0 }
    {
        return Err(ThumbError::Unreadable);
    }

    // 핵심 옵션 세 가지:
    //   FromImageAlways — 내장 미리보기가 없거나 너무 작으면 원본에서 만든다
    //   MaxPixelSize    — 디코더가 이 크기에 맞춰 축소본만 생성한다
    //   WithTransform   — EXIF 방향을 적용해 회전된 사진이 바로 선다
    let thumb_opts = unsafe {
        CFDictionary::from_CFType_pairs(&[
            (
                CFString::wrap_under_get_rule(ffi::kCGImageSourceCreateThumbnailFromImageAlways),
                CFType::wrap_under_get_rule(
                    core_foundation::boolean::CFBoolean::true_value().as_CFTypeRef(),
                ),
            ),
            (
                CFString::wrap_under_get_rule(ffi::kCGImageSourceCreateThumbnailWithTransform),
                CFType::wrap_under_get_rule(
                    core_foundation::boolean::CFBoolean::true_value().as_CFTypeRef(),
                ),
            ),
            (
                CFString::wrap_under_get_rule(ffi::kCGImageSourceThumbnailMaxPixelSize),
                CFNumber::from(max_px as i64).as_CFType(),
            ),
        ])
    };

    let image = unsafe {
        ffi::CGImageSourceCreateThumbnailAtIndex(source, 0, thumb_opts.as_concrete_TypeRef())
    };
    if image.is_null() {
        return Err(ThumbError::NoThumbnail);
    }
    let _image = Owned(image, release_image as fn(_));

    let size = unsafe {
        ThumbSize {
            width: ffi::CGImageGetWidth(image) as u32,
            height: ffi::CGImageGetHeight(image) as u32,
        }
    };

    write_jpeg(image, out, quality)?;
    Ok(size)
}

/// CGImage 하나를 JPEG 파일로 쓴다.
///
/// 사진(ImageIO)과 영상 프레임(QuickLook)이 같은 길을 쓴다. 어느 쪽이든
/// 결국 CGImage 하나이므로 저장 코드를 둘로 둘 이유가 없다.
pub(crate) fn write_jpeg(image: ffi::CGImageRef, out: &Path, quality: f64) -> Result<()> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out_url = CFURL::from_path(out, false).ok_or(ThumbError::BadPath)?;
    let jpeg = CFString::new("public.jpeg");
    let dest = unsafe {
        ffi::CGImageDestinationCreateWithURL(
            out_url.as_concrete_TypeRef(),
            jpeg.as_concrete_TypeRef(),
            1,
            std::ptr::null(),
        )
    };
    if dest.is_null() {
        return Err(ThumbError::WriteFailed);
    }
    let _dest = Owned(dest, release_dest as fn(_));

    let dst_opts = unsafe {
        CFDictionary::from_CFType_pairs(&[(
            CFString::wrap_under_get_rule(ffi::kCGImageDestinationLossyCompressionQuality),
            CFNumber::from(quality).as_CFType(),
        )])
    };
    unsafe {
        ffi::CGImageDestinationAddImage(dest, image, dst_opts.as_concrete_TypeRef());
        if !ffi::CGImageDestinationFinalize(dest) {
            return Err(ThumbError::WriteFailed);
        }
    }
    Ok(())
}

/// 이 파일을 ImageIO가 실제로 디코딩할 수 있는지 확인한다.
///
/// 주의: `CGImageSourceCreateWithURL`이 성공했다고 읽을 수 있는 건 아니다.
/// 확장자만 보고 소스를 만들기 때문에, 내용이 JPEG이 아닌 `.jpg` 파일도
/// 널이 아닌 소스를 돌려준다. 상태와 이미지 개수까지 봐야 한다.
pub fn is_readable(src: impl AsRef<Path>) -> bool {
    let Some(url) = CFURL::from_path(src.as_ref(), false) else {
        return false;
    };
    let s = unsafe { ffi::CGImageSourceCreateWithURL(url.as_concrete_TypeRef(), std::ptr::null()) };
    if s.is_null() {
        return false;
    }
    let ok = unsafe { ffi::CGImageSourceGetStatus(s) == 0 && ffi::CGImageSourceGetCount(s) > 0 };
    unsafe { ffi::CFRelease(s as _) };
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 실제 라이브러리에서 표본을 찾는다. 없으면 테스트를 건너뛴다.
    fn sample(ext: &str) -> Option<PathBuf> {
        let roots = ["/Volumes/PHOTO 1", "/Volumes/MAIN SSD/MERGE/사진통합작업"];
        for r in roots {
            let root = Path::new(r);
            if !root.is_dir() {
                continue;
            }
            let mut stack = vec![root.to_path_buf()];
            let mut seen = 0;
            while let Some(d) = stack.pop() {
                seen += 1;
                if seen > 400 {
                    break; // 너무 깊이 뒤지지 않는다
                }
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if p
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.eq_ignore_ascii_case(ext))
                        .unwrap_or(false)
                    {
                        return Some(p);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn makes_a_jpeg_thumbnail() {
        let Some(src) = sample("jpg") else { return };
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("t.jpg");
        let size = make(&src, &out, 512, 0.8).expect("썸네일 생성");
        assert!(out.exists(), "파일이 만들어져야 한다");
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
        assert!(
            size.width <= 512 && size.height <= 512,
            "긴 변이 512 이하여야 한다: {size:?}"
        );
        assert!(size.width > 0 && size.height > 0);
    }

    #[test]
    fn decodes_raw_without_libraw() {
        // CR2는 이전 구현(image 크레이트)이 열지 못하던 형식이다.
        let Some(src) = sample("cr2") else { return };
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("raw.jpg");
        let size = make(&src, &out, 512, 0.8).expect("ImageIO는 CR2를 읽어야 한다");
        assert!(size.width > 0 && size.height > 0);
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
    }

    /// 뷰어가 요청 시 만드는 크기. 그리드용보다 확실히 커야 하고,
    /// RAW에서도 나와야 한다 — RAW는 웹뷰가 직접 못 읽으니 이게 유일한 경로다.
    #[test]
    fn makes_a_viewer_preview() {
        let Some(src) = sample("jpg") else { return };
        let dir = tempfile::tempdir().unwrap();

        let small = dir.path().join("s.jpg");
        let s = make(&src, &small, crate::media::cache::THUMB_PX, 0.72).expect("썸네일");

        let big = dir.path().join("p.jpg");
        let p = make(&src, &big, crate::media::cache::PREVIEW_PX, 0.85).expect("미리보기");

        // 원본이 작으면 둘이 같을 수 있다 — 그때는 확대하지 않았는지만 본다
        assert!(p.width >= s.width && p.height >= s.height, "{p:?} < {s:?}");
        assert!(p.width <= crate::media::cache::PREVIEW_PX);
        assert!(p.height <= crate::media::cache::PREVIEW_PX);
        assert!(std::fs::metadata(&big).unwrap().len() > 0);
    }

    #[test]
    fn small_source_is_not_upscaled() {
        let Some(src) = sample("jpg") else { return };
        let dir = tempfile::tempdir().unwrap();
        // 아주 큰 값을 주면 원본 크기를 넘지 않아야 한다.
        let out = dir.path().join("big.jpg");
        let s = make(&src, &out, 100_000, 0.8).expect("생성");
        assert!(s.width < 100_000 && s.height < 100_000, "확대하지 않는다");
    }

    #[test]
    fn unreadable_file_reports_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let bogus = dir.path().join("not-an-image.jpg");
        std::fs::write(&bogus, b"this is definitely not a jpeg").unwrap();
        let out = dir.path().join("out.jpg");
        assert!(make(&bogus, &out, 256, 0.8).is_err());
        assert!(!is_readable(&bogus));
    }

    #[test]
    fn missing_file_reports_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.jpg");
        assert!(make("/no/such/file.jpg", &out, 256, 0.8).is_err());
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    /// 실제 라이브러리에서 JPEG 표본을 모은다.
    fn collect(limit: usize) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let roots = ["/Volumes/MAIN SSD/MERGE/사진통합작업", "/Volumes/PHOTO 1"];
        for r in roots {
            let root = Path::new(r);
            if !root.is_dir() {
                continue;
            }
            let mut stack = vec![root.to_path_buf()];
            while let Some(d) = stack.pop() {
                if out.len() >= limit {
                    return out;
                }
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if p
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.eq_ignore_ascii_case("jpg"))
                        .unwrap_or(false)
                    {
                        out.push(p);
                        if out.len() >= limit {
                            return out;
                        }
                    }
                }
            }
        }
        out
    }

    /// `cargo test --release --lib bench::sips_vs_imageio -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 사진이 필요하고 시간이 걸린다"]
    fn sips_vs_imageio() {
        const N: usize = 200;
        let files = collect(N);
        if files.len() < 20 {
            eprintln!("표본 부족 ({}장) — 건너뜀", files.len());
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let total: u64 = files
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();
        println!(
            "\n표본 {}장 · 원본 합계 {:.1} MB\n",
            files.len(),
            total as f64 / 1024.0 / 1024.0
        );

        // ── 이전 방식: 파일마다 sips 프로세스 ──────────────────────────
        let t = Instant::now();
        let mut ok_sips = 0;
        for (i, f) in files.iter().enumerate() {
            let out = dir.path().join(format!("s{i}.jpg"));
            let st = std::process::Command::new("sips")
                .args(["--resampleHeightWidthMax", "512", "-s", "format", "jpeg"])
                .arg(f)
                .arg("--out")
                .arg(&out)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if matches!(st, Ok(s) if s.success()) {
                ok_sips += 1;
            }
        }
        let d_sips = t.elapsed();

        // ── 새 방식: 인프로세스 ImageIO (단일 스레드) ───────────────────
        let t = Instant::now();
        let mut ok_io = 0;
        for (i, f) in files.iter().enumerate() {
            if make(f, dir.path().join(format!("i{i}.jpg")), 512, 0.8).is_ok() {
                ok_io += 1;
            }
        }
        let d_io = t.elapsed();

        // ── 새 방식 + rayon 병렬 ───────────────────────────────────────
        use rayon::prelude::*;
        let t = Instant::now();
        let ok_par: usize = files
            .par_iter()
            .enumerate()
            .map(|(i, f)| {
                make(f, dir.path().join(format!("p{i}.jpg")), 512, 0.8)
                    .is_ok() as usize
            })
            .sum();
        let d_par = t.elapsed();

        let per = |d: std::time::Duration, n: usize| d.as_secs_f64() * 1000.0 / n as f64;
        println!("  sips (프로세스 fork)   {:>8.0} ms   {:>6.1} ms/장   성공 {ok_sips}",
                 d_sips.as_secs_f64() * 1000.0, per(d_sips, files.len()));
        println!("  ImageIO (인프로세스)   {:>8.0} ms   {:>6.1} ms/장   성공 {ok_io}",
                 d_io.as_secs_f64() * 1000.0, per(d_io, files.len()));
        println!("  ImageIO + rayon        {:>8.0} ms   {:>6.1} ms/장   성공 {ok_par}",
                 d_par.as_secs_f64() * 1000.0, per(d_par, files.len()));
        println!("\n  단일 스레드 개선  {:.1}배", d_sips.as_secs_f64() / d_io.as_secs_f64());
        println!("  병렬 포함 개선    {:.1}배", d_sips.as_secs_f64() / d_par.as_secs_f64());
        let est = per(d_par, files.len()) * 65_074.0 / 1000.0;
        println!("\n  → 65,074장 환산: {:.0}초 ({:.1}분)\n", est, est / 60.0);

        assert!(ok_io >= ok_sips, "ImageIO가 sips보다 적게 성공하면 안 된다");
        assert!(d_io < d_sips, "인프로세스가 더 빨라야 한다");
    }
}
