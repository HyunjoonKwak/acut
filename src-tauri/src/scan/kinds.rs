//! 파일 종류 판정 — 무엇을 라이브러리에 넣고 무엇을 건너뛸지.

/// DB `files.kind`에 그대로 들어간다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Kind {
    Image = 0,
    Video = 1,
    /// RAW는 따로 센다. 백업 정책이 다르다 (NAS에 올리지 않고 로컬 한 벌).
    Raw = 2,
}

const IMAGE: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "gif", "bmp", "tif", "tiff", "webp", "avif", "jxl",
];
const RAW: &[&str] = &[
    "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "dng", "orf", "rw2", "raf", "pef", "raw",
    "3fr", "erf", "kdc", "mos", "mrw", "x3f",
];
const VIDEO: &[&str] = &[
    "mp4", "mov", "m4v", "avi", "mkv", "3gp", "mts", "m2ts", "wmv", "flv", "webm", "mpg", "mpeg",
];

/// 스캔에서 통째로 건너뛸 폴더.
///
/// `@eaDir`는 시놀로지가 만드는 썸네일 캐시다. 이것을 넣으면 같은 사진이
/// 몇 배로 늘어난다. `#recycle`은 NAS 휴지통이다.
pub fn is_skipped_dir(name: &str) -> bool {
    matches!(
        name,
        "@eaDir"
            | "#recycle"
            | "#trash"
            | ".Spotlight-V100"
            | ".fseventsd"
            | ".TemporaryItems"
            | ".DocumentRevisions-V100"
            | ".Trashes"
            | ".DS_Store"
            | "$RECYCLE.BIN"
            | "System Volume Information"
            // 우리 자신의 캐시, NAS에서 받다 만 파일(nas/ssh.rs PARTIAL_DIR)
            | ".acut"
            | ".rsync-partial"
    ) || name.starts_with(".git")
}

/// 파일명으로 종류를 판정한다. 미디어가 아니면 None.
pub fn classify(name: &str) -> Option<Kind> {
    // 리소스 포크(._foo.jpg)와 숨김 파일은 제외
    if name.starts_with("._") || name.starts_with('.') {
        return None;
    }
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    if RAW.contains(&ext.as_str()) {
        return Some(Kind::Raw);
    }
    if IMAGE.contains(&ext.as_str()) {
        return Some(Kind::Image);
    }
    if VIDEO.contains(&ext.as_str()) {
        return Some(Kind::Video);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension() {
        assert_eq!(classify("a.jpg"), Some(Kind::Image));
        assert_eq!(classify("a.JPG"), Some(Kind::Image), "대소문자 무시");
        assert_eq!(classify("a.heic"), Some(Kind::Image));
        assert_eq!(classify("IMG_0075.CR2"), Some(Kind::Raw));
        assert_eq!(classify("a.dng"), Some(Kind::Raw));
        assert_eq!(classify("C0086.MP4"), Some(Kind::Video));
        assert_eq!(classify("a.mov"), Some(Kind::Video));
    }

    #[test]
    fn ignores_non_media() {
        assert_eq!(classify("readme.txt"), None);
        assert_eq!(classify("사진 백업방법.txt"), None);
        assert_eq!(classify("noext"), None);
        assert_eq!(classify(""), None);
    }

    #[test]
    fn ignores_hidden_and_resource_forks() {
        // macOS가 외장 디스크에 만드는 리소스 포크 — 실제 라이브러리에 많다
        assert_eq!(classify("._2022.jpg"), None);
        assert_eq!(classify(".DS_Store"), None);
        assert_eq!(classify(".hidden.jpg"), None);
    }

    #[test]
    fn skips_synology_and_system_folders() {
        // @eaDir을 넣으면 같은 사진이 몇 배로 늘어난다
        assert!(is_skipped_dir("@eaDir"));
        assert!(is_skipped_dir("#recycle"));
        assert!(is_skipped_dir("#trash"));
        assert!(is_skipped_dir(".Spotlight-V100"));
        assert!(is_skipped_dir(".acut"));
        assert!(is_skipped_dir(".git"));
        // 평범한 폴더는 건너뛰지 않는다
        assert!(!is_skipped_dir("2018"));
        assert!(!is_skipped_dir("2018-07-25 하와이 7일차"));
        assert!(!is_skipped_dir("현준-핸드폰사진 백업"));
    }

    #[test]
    fn kind_values_match_the_schema() {
        assert_eq!(Kind::Image as i32, 0);
        assert_eq!(Kind::Video as i32, 1);
        assert_eq!(Kind::Raw as i32, 2);
    }
}
