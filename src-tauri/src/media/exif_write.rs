//! JPEG 촬영일 쓰기 — ImageIO가 기존 메타데이터를 보존한 채 EXIF/TIFF 시각을 갱신한다.

#![cfg(target_os = "macos")]

use chrono::{Local, TimeZone};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFMutableDictionary;
use core_foundation::string::CFString;
use core_foundation::url::CFURL;
use std::os::raw::c_void;
use std::path::{Path, PathBuf};

mod ffi {
    use core_foundation::dictionary::CFDictionaryRef;
    use core_foundation::string::CFStringRef;
    use core_foundation::url::CFURLRef;
    use std::os::raw::c_void;

    pub type CGImageSourceRef = *const c_void;
    pub type CGImageDestinationRef = *const c_void;
    pub type CGImageMetadataRef = *const c_void;
    pub type CGMutableImageMetadataRef = *const c_void;

    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        pub fn CGImageSourceCreateWithURL(url: CFURLRef, opts: CFDictionaryRef)
            -> CGImageSourceRef;
        pub fn CGImageSourceCopyMetadataAtIndex(
            src: CGImageSourceRef,
            index: usize,
            opts: CFDictionaryRef,
        ) -> CGImageMetadataRef;
        pub fn CGImageSourceGetType(src: CGImageSourceRef) -> CFStringRef;
        pub fn CGImageDestinationCreateWithURL(
            url: CFURLRef,
            ty: CFStringRef,
            count: usize,
            opts: CFDictionaryRef,
        ) -> CGImageDestinationRef;
        pub fn CGImageDestinationCopyImageSource(
            dst: CGImageDestinationRef,
            src: CGImageSourceRef,
            opts: CFDictionaryRef,
            error: *mut *const c_void,
        ) -> bool;
        pub fn CGImageMetadataCreateMutable() -> CGMutableImageMetadataRef;
        pub fn CGImageMetadataCreateMutableCopy(
            metadata: CGImageMetadataRef,
        ) -> CGMutableImageMetadataRef;
        pub fn CGImageMetadataSetValueWithPath(
            metadata: CGMutableImageMetadataRef,
            parent: *const c_void,
            path: CFStringRef,
            value: *const c_void,
        ) -> bool;
        pub static kCGImageDestinationMetadata: CFStringRef;
        pub static kCGImageDestinationMergeMetadata: CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFRelease(cf: *const c_void);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("JPEG 파일을 열 수 없습니다")]
    Open,
    #[error("JPEG 메타데이터를 읽을 수 없습니다")]
    Metadata,
    #[error("임시 JPEG를 만들 수 없습니다")]
    Destination,
    #[error("JPEG 메타데이터를 저장할 수 없습니다")]
    Finalize,
    #[error("파일 쓰기 실패: {0}")]
    Io(#[from] std::io::Error),
}

struct Owned(*const c_void);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ffi::CFRelease(self.0) }
        }
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.photo-desk-{}.tmp", std::process::id()))
}

/// 세 필드(DateTimeOriginal, DateTimeDigitized, TIFF DateTime)를 같은 지역 wall-clock으로 쓴다.
pub fn write_capture_time(path: &Path, timestamp: i64) -> Result<(), WriteError> {
    let source_url = CFURL::from_path(path, false).ok_or(WriteError::Open)?;
    let source = unsafe {
        ffi::CGImageSourceCreateWithURL(source_url.as_concrete_TypeRef(), std::ptr::null())
    };
    if source.is_null() {
        return Err(WriteError::Open);
    }
    let _source = Owned(source);
    let old_metadata =
        unsafe { ffi::CGImageSourceCopyMetadataAtIndex(source, 0, std::ptr::null()) };
    let metadata = if old_metadata.is_null() {
        unsafe { ffi::CGImageMetadataCreateMutable() }
    } else {
        let copy = unsafe { ffi::CGImageMetadataCreateMutableCopy(old_metadata) };
        unsafe { ffi::CFRelease(old_metadata) };
        copy
    };
    if metadata.is_null() {
        return Err(WriteError::Metadata);
    }
    let _metadata = Owned(metadata);

    let local = Local
        .timestamp_opt(timestamp, 0)
        .single()
        .ok_or(WriteError::Metadata)?;
    let value = CFString::new(&local.format("%Y:%m:%d %H:%M:%S").to_string());
    for path in [
        "exif:DateTimeOriginal",
        "exif:DateTimeDigitized",
        "tiff:DateTime",
    ] {
        let key = CFString::new(path);
        let ok = unsafe {
            ffi::CGImageMetadataSetValueWithPath(
                metadata,
                std::ptr::null(),
                key.as_concrete_TypeRef(),
                value.as_concrete_TypeRef() as _,
            )
        };
        if !ok {
            return Err(WriteError::Metadata);
        }
    }

    let temp = temp_path(path);
    let _ = std::fs::remove_file(&temp);
    let output_url = CFURL::from_path(&temp, false).ok_or(WriteError::Destination)?;
    let ty = unsafe { ffi::CGImageSourceGetType(source) };
    if ty.is_null() {
        return Err(WriteError::Destination);
    }
    let dest = unsafe {
        ffi::CGImageDestinationCreateWithURL(
            output_url.as_concrete_TypeRef(),
            ty,
            1,
            std::ptr::null(),
        )
    };
    if dest.is_null() {
        return Err(WriteError::Destination);
    }
    let _dest = Owned(dest);
    let mut options = CFMutableDictionary::new();
    let merge = CFBoolean::true_value();
    options.set(
        unsafe { ffi::kCGImageDestinationMetadata } as *const c_void,
        metadata,
    );
    options.set(
        unsafe { ffi::kCGImageDestinationMergeMetadata } as *const c_void,
        merge.as_concrete_TypeRef() as *const c_void,
    );
    let copied = unsafe {
        ffi::CGImageDestinationCopyImageSource(
            dest,
            source,
            options.as_concrete_TypeRef(),
            std::ptr::null_mut(),
        )
    };
    if !copied {
        let _ = std::fs::remove_file(&temp);
        return Err(WriteError::Finalize);
    }
    let permissions = std::fs::metadata(path)?.permissions();
    std::fs::set_permissions(&temp, permissions)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_all_capture_fields_without_changing_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.jpg");
        let image = image::RgbImage::from_fn(8, 8, |x, y| {
            image::Rgb([(x * 11) as u8, (y * 13) as u8, 77])
        });
        image.save(&path).unwrap();
        let before = image::open(&path).unwrap().to_rgb8();
        let wanted = crate::media::taken_at::civil_to_unix(2024, 1, 2, 23, 59, 58);

        write_capture_time(&path, wanted).unwrap();

        let after = image::open(&path).unwrap().to_rgb8();
        assert_eq!(before, after, "메타데이터 쓰기가 화소를 바꾸면 안 된다");
        assert_eq!(
            crate::media::exif::read(&path).and_then(|m| m.taken_at),
            Some(wanted)
        );
    }
}
