//! EXIF 읽기 — ImageIO로 메타데이터를 꺼낸다.
//!
//! `kamadak-exif` 같은 순수 Rust 파서 대신 ImageIO를 쓰는 이유:
//!   - CR2·NEF·ARW·HEIC의 메타데이터도 같은 코드로 읽힌다
//!   - 썸네일 생성과 **같은 API**라, 나중에 소스를 한 번만 열고 둘 다 처리할 수 있다
//!   - 애플이 관리하는 파서라 이상한 제조사 변형에 강하다

#![cfg(target_os = "macos")]

use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation::url::CFURL;
use std::os::raw::c_void;
use std::path::Path;

mod ffi {
    use core_foundation::dictionary::CFDictionaryRef;
    use core_foundation::string::CFStringRef;
    use core_foundation::url::CFURLRef;
    use std::os::raw::c_void;

    pub type CGImageSourceRef = *const c_void;

    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        pub fn CGImageSourceCreateWithURL(url: CFURLRef, opts: CFDictionaryRef)
            -> CGImageSourceRef;
        pub fn CGImageSourceCopyPropertiesAtIndex(
            src: CGImageSourceRef,
            index: usize,
            opts: CFDictionaryRef,
        ) -> CFDictionaryRef;

        pub static kCGImagePropertyExifDictionary: CFStringRef;
        pub static kCGImagePropertyTIFFDictionary: CFStringRef;
        pub static kCGImagePropertyGPSDictionary: CFStringRef;
        pub static kCGImagePropertyPixelWidth: CFStringRef;
        pub static kCGImagePropertyPixelHeight: CFStringRef;
        pub static kCGImagePropertyOrientation: CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFRelease(cf: *const c_void);
        pub fn CFGetTypeID(cf: *const c_void) -> usize;
        pub fn CFStringGetTypeID() -> usize;
        pub fn CFNumberGetTypeID() -> usize;
        pub fn CFDictionaryGetTypeID() -> usize;
        pub fn CFArrayGetTypeID() -> usize;
    }
}

/// 파일에서 읽어낸 메타데이터. 없는 값은 None이다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: Option<u32>,
    /// EXIF DateTimeOriginal을 유닉스 시각으로. offset이 없으므로 현재 기기의
    /// 지역 시각으로 해석한다.
    pub taken_at: Option<i64>,
    pub cam_make: Option<String>,
    pub cam_model: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<i64>,
    pub aperture: Option<f64>,
    /// `1/250` 같은 표기 그대로. 계산하지 않고 보여주기 위한 값이다.
    pub shutter: Option<String>,
    pub focal_mm: Option<f64>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    pub gps_alt: Option<f64>,
}

/// 파일의 메타데이터를 읽는다. 읽을 수 없으면 기본값(전부 None)이 아니라 None.
pub fn read(path: impl AsRef<Path>) -> Option<Meta> {
    let url = CFURL::from_path(path.as_ref(), false)?;
    let src =
        unsafe { ffi::CGImageSourceCreateWithURL(url.as_concrete_TypeRef(), std::ptr::null()) };
    if src.is_null() {
        return None;
    }
    let props = unsafe { ffi::CGImageSourceCopyPropertiesAtIndex(src, 0, std::ptr::null()) };
    unsafe { ffi::CFRelease(src as _) };
    if props.is_null() {
        return None;
    }
    // CopyProperties는 소유권을 넘겨준다 — wrap_under_create_rule이 해제를 맡는다.
    let root: CFDictionary = unsafe { CFDictionary::wrap_under_create_rule(props) };

    let mut m = Meta {
        width: num_const(&root, unsafe { ffi::kCGImagePropertyPixelWidth }).map(|v| v as u32),
        height: num_const(&root, unsafe { ffi::kCGImagePropertyPixelHeight }).map(|v| v as u32),
        orientation: num_const(&root, unsafe { ffi::kCGImagePropertyOrientation })
            .map(|v| v as u32),
        ..Default::default()
    };

    if let Some(exif) = sub(&root, unsafe { ffi::kCGImagePropertyExifDictionary }) {
        m.taken_at = text(&exif, "DateTimeOriginal")
            .or_else(|| text(&exif, "DateTimeDigitized"))
            .and_then(|s| parse_exif_time(&s));
        // ISOSpeedRatings는 배열로 오는 경우가 많다.
        m.iso = num_or_first(&exif, "ISOSpeedRatings")
            .or_else(|| num_or_first(&exif, "PhotographicSensitivity"));
        m.aperture = real(&exif, "FNumber");
        m.focal_mm = real(&exif, "FocalLength");
        m.lens = text(&exif, "LensModel");
        m.shutter = real(&exif, "ExposureTime").map(format_shutter);
    }

    if let Some(tiff) = sub(&root, unsafe { ffi::kCGImagePropertyTIFFDictionary }) {
        m.cam_make = text(&tiff, "Make");
        m.cam_model = text(&tiff, "Model");
        if m.taken_at.is_none() {
            m.taken_at = text(&tiff, "DateTime").and_then(|s| parse_exif_time(&s));
        }
    }

    if let Some(gps) = sub(&root, unsafe { ffi::kCGImagePropertyGPSDictionary }) {
        let lat = real(&gps, "Latitude");
        let lon = real(&gps, "Longitude");
        // 반구 표시가 없으면 부호를 알 수 없다. S/W면 음수로 바꾼다.
        m.gps_lat = lat.map(|v| match text(&gps, "LatitudeRef").as_deref() {
            Some("S") => -v,
            _ => v,
        });
        m.gps_lon = lon.map(|v| match text(&gps, "LongitudeRef").as_deref() {
            Some("W") => -v,
            _ => v,
        });
        m.gps_alt = real(&gps, "Altitude").map(|v| {
            // AltitudeRef 1 = 해수면 아래
            match num(&gps, "AltitudeRef") {
                Some(1) => -v,
                _ => v,
            }
        });
    }

    Some(m)
}

// ── CFDictionary 헬퍼 ──────────────────────────────────────────────────
//
// 두 가지를 조심해야 한다:
//   1) `CFString::new(k).as_concrete_TypeRef()`를 인자로 바로 넘기면 임시 CFString이
//      그 자리에서 해제되어 해제된 메모리를 가리킨다. 반드시 변수에 묶는다.
//   2) 딕셔너리 값의 타입은 보장되지 않는다. 타입 확인 없이 wrap하면
//      Objective-C 예외가 나고 Rust는 그것을 잡지 못해 프로세스가 죽는다.

fn is_kind(v: *const c_void, want: unsafe extern "C" fn() -> usize) -> bool {
    !v.is_null() && unsafe { ffi::CFGetTypeID(v) == want() }
}

/// 문자열 키로 raw 값을 꺼낸다. CFString은 변수로 살아 있는 동안만 유효하다.
fn raw(d: &CFDictionary, key: &str) -> Option<*const c_void> {
    let k = CFString::new(key); // 이 변수가 살아 있어야 한다
    let found = d.find(k.as_concrete_TypeRef() as *const c_void).map(|v| *v);
    found
}

fn raw_const(d: &CFDictionary, key: core_foundation::string::CFStringRef) -> Option<*const c_void> {
    d.find(key as *const c_void).map(|v| *v)
}

fn sub(d: &CFDictionary, key: core_foundation::string::CFStringRef) -> Option<CFDictionary> {
    let v = raw_const(d, key)?;
    is_kind(v, ffi::CFDictionaryGetTypeID)
        .then(|| unsafe { CFDictionary::wrap_under_get_rule(v as _) })
}

fn num_const(d: &CFDictionary, key: core_foundation::string::CFStringRef) -> Option<i64> {
    let v = raw_const(d, key)?;
    is_kind(v, ffi::CFNumberGetTypeID)
        .then(|| unsafe { CFNumber::wrap_under_get_rule(v as _) })?
        .to_i64()
}

fn num(d: &CFDictionary, key: &str) -> Option<i64> {
    let v = raw(d, key)?;
    is_kind(v, ffi::CFNumberGetTypeID)
        .then(|| unsafe { CFNumber::wrap_under_get_rule(v as _) })?
        .to_i64()
}

fn text(d: &CFDictionary, key: &str) -> Option<String> {
    let v = raw(d, key)?;
    is_kind(v, ffi::CFStringGetTypeID)
        .then(|| unsafe { CFString::wrap_under_get_rule(v as _) }.to_string())
        .filter(|s| !s.trim().is_empty())
}

fn real(d: &CFDictionary, key: &str) -> Option<f64> {
    let v = raw(d, key)?;
    is_kind(v, ffi::CFNumberGetTypeID)
        .then(|| unsafe { CFNumber::wrap_under_get_rule(v as _) })?
        .to_f64()
}

/// ISOSpeedRatings처럼 배열로 오는 값의 첫 항목. 숫자로 올 때도 있어 둘 다 받는다.
fn num_or_first(d: &CFDictionary, key: &str) -> Option<i64> {
    use core_foundation::array::CFArray;
    let v = raw(d, key)?;
    if is_kind(v, ffi::CFNumberGetTypeID) {
        return unsafe { CFNumber::wrap_under_get_rule(v as _) }.to_i64();
    }
    if is_kind(v, ffi::CFArrayGetTypeID) {
        let arr: CFArray = unsafe { CFArray::wrap_under_get_rule(v as _) };
        let first = *arr.get(0)?;
        if is_kind(first, ffi::CFNumberGetTypeID) {
            return unsafe { CFNumber::wrap_under_get_rule(first as _) }.to_i64();
        }
    }
    None
}

/// `"2018:07:25 14:31:02"` → 유닉스 시각.
///
/// EXIF의 날짜 구분자는 콜론이다. 시간대 정보가 없으므로 지역 시각으로
/// 그대로 해석한다 (UTC 변환을 하면 자정 근처 사진이 하루 밀린다).
pub fn parse_exif_time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: &[u8]| -> Option<i64> {
        let mut v = 0i64;
        for &c in r {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v * 10 + (c - b'0') as i64;
        }
        Some(v)
    };
    let (y, mo, da) = (n(&b[0..4])?, n(&b[5..7])?, n(&b[8..10])?);
    let (h, mi, se) = (n(&b[11..13])?, n(&b[14..16])?, n(&b[17..19])?);
    if !(1990..=2100).contains(&y) || !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        return None;
    }
    if h >= 24 || mi >= 60 || se >= 60 {
        return None;
    }
    Some(super::taken_at::civil_to_unix(y, mo, da, h, mi, se))
}

/// `0.004` → `"1/250"`. 1초 이상이면 `"2s"`.
fn format_shutter(sec: f64) -> String {
    if sec <= 0.0 {
        return String::new();
    }
    if sec >= 1.0 {
        return format!("{sec:.1}s");
    }
    format!("1/{}", (1.0 / sec).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample(ext: &str) -> Option<PathBuf> {
        let roots = ["/Volumes/MAIN SSD/MERGE/사진통합작업", "/Volumes/PHOTO 1"];
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
                    break;
                }
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    // exFAT의 ._ 사이드카는 사진이 아니다
                    if p.file_name().map(|n| n.to_string_lossy().starts_with("._")).unwrap_or(false) {
                        continue;
                    }
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
    fn parses_exif_timestamp_format() {
        // EXIF는 날짜 구분자가 콜론이다
        let t = parse_exif_time("2018:07:25 14:31:02").expect("파싱");
        assert_eq!(t, super::super::taken_at::civil_to_unix(2018, 7, 25, 14, 31, 2));
    }

    #[test]
    fn rejects_malformed_timestamps() {
        assert_eq!(parse_exif_time(""), None);
        assert_eq!(parse_exif_time("2018:07:25"), None); // 시각이 없다
        assert_eq!(parse_exif_time("0000:00:00 00:00:00"), None); // 미설정 카메라
        assert_eq!(parse_exif_time("2018:13:25 14:31:02"), None); // 13월
        assert_eq!(parse_exif_time("2018:07:25 25:31:02"), None); // 25시
    }

    #[test]
    fn shutter_formatting() {
        assert_eq!(format_shutter(0.004), "1/250");
        assert_eq!(format_shutter(0.5), "1/2");
        assert_eq!(format_shutter(2.0), "2.0s");
        assert_eq!(format_shutter(0.0), "");
    }

    #[test]
    fn reads_dimensions_from_a_real_photo() {
        let Some(p) = sample("jpg") else { return };
        let m = read(&p).expect("메타데이터를 읽어야 한다");
        assert!(m.width.unwrap_or(0) > 0, "너비: {m:?}");
        assert!(m.height.unwrap_or(0) > 0, "높이: {m:?}");
    }

    #[test]
    fn reads_raw_metadata_too() {
        let Some(p) = sample("cr2") else { return };
        let m = read(&p).expect("CR2도 읽어야 한다");
        assert!(m.width.unwrap_or(0) > 0);
        // RAW는 카메라 정보가 거의 항상 있다
        assert!(
            m.cam_make.is_some() || m.cam_model.is_some(),
            "RAW에 카메라 정보가 있어야 한다: {m:?}"
        );
    }

    #[test]
    fn returns_none_for_non_images() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.jpg");
        std::fs::write(&p, b"not an image").unwrap();
        // 소스는 만들어지지만 속성이 없거나 크기가 없다
        let m = read(&p);
        assert!(m.is_none() || m.unwrap().width.is_none());
    }
}

#[cfg(test)]
mod smoke {
    use super::*;

    /// `cargo test --release --lib media::exif::smoke -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 사진 필요"]
    fn dump_real_photos() {
        let roots = ["/Volumes/MAIN SSD/MERGE/사진통합작업", "/Volumes/PHOTO 1"];
        let mut shown = 0;
        let mut stack: Vec<std::path::PathBuf> =
            roots.iter().map(std::path::PathBuf::from).filter(|p| p.is_dir()).collect();
        let mut by_ext: std::collections::HashMap<String, usize> = Default::default();

        while let Some(d) = stack.pop() {
            if shown >= 6 {
                break;
            }
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                // exFAT의 ._ 사이드카는 사진이 아니다
                if p.file_name().map(|n| n.to_string_lossy().starts_with("._")).unwrap_or(false) {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let Some(ext) = p.extension().and_then(|x| x.to_str()).map(|x| x.to_lowercase())
                else {
                    continue;
                };
                if !matches!(ext.as_str(), "jpg" | "cr2" | "heic" | "png" | "mp4") {
                    continue;
                }
                // 확장자마다 하나씩만
                if *by_ext.get(&ext).unwrap_or(&0) > 0 {
                    continue;
                }
                by_ext.insert(ext.clone(), 1);
                let name = p.file_name().unwrap().to_string_lossy();
                match read(&p) {
                    Some(m) => {
                        println!("\n  [{ext}] {name}");
                        println!("     크기   {:?} x {:?}", m.width, m.height);
                        println!("     촬영   {:?}  방향 {:?}", m.taken_at, m.orientation);
                        println!("     카메라 {:?} {:?}", m.cam_make, m.cam_model);
                        println!("     렌즈   {:?}", m.lens);
                        println!(
                            "     설정   ISO {:?} · f{:?} · {:?} · {:?}mm",
                            m.iso, m.aperture, m.shutter, m.focal_mm
                        );
                        println!("     GPS    {:?}, {:?}", m.gps_lat, m.gps_lon);
                        // 촬영일 체인이 실제로 어떤 출처를 고르는지
                        let md = std::fs::metadata(&p).ok();
                        let mtime = md.as_ref().and_then(|m| {
                            m.modified().ok().and_then(|t| {
                                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
                            })
                        });
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs() as i64;
                        let (ts, src) =
                            super::super::taken_at::resolve(m.taken_at, &name, mtime, None, now);
                        println!("     → 결정 {ts} ({src:?})");
                        shown += 1;
                    }
                    None => println!("\n  [{ext}] {name} — 메타데이터 없음"),
                }
                if shown >= 6 {
                    break;
                }
            }
        }
    }
}
