//! 썸네일 캐시 — 어디에 저장하고 언제 다시 만드는가.
//!
//! **캐시는 라이브러리 볼륨 안(`.acut/thumbs/`)에 둔다.** 앱 데이터 폴더가 아니다.
//! 이 프로젝트에서 실제로 겪은 일 때문이다: 리브랜딩으로 앱 ID가
//! `com.smartcategory.media` → `com.acut.media`로 바뀌자 DB의 썸네일 경로
//! 64,698건이 전부 죽은 링크가 됐다. 볼륨 안에 두면 앱 ID가 바뀌든 디스크를
//! 다른 맥에 꽂든 캐시가 함께 간다.
//!
//! 파일명은 `볼륨 상대경로 + 크기 + 수정시각`의 해시다. DB를 새로 만들어도
//! 같은 이름이 나오므로 재생성하지 않는다.

use std::path::{Path, PathBuf};

/// 그리드 썸네일의 긴 변. 화면에서 최대 320px로 쓰므로 레티나(2배)를 감안했다.
pub const THUMB_PX: u32 = 640;
pub const THUMB_QUALITY: f64 = 0.72;

/// 뷰어용 미리보기. 5K 화면에서 전체화면으로 봐도 견디는 크기.
///
/// 원본을 그대로 주지 않는 이유가 둘이다:
///   - 5760×3840 JPEG는 5MB가 넘어 넘기기 버겁다
///   - **RAW는 웹뷰가 못 읽는다.** ImageIO로 미리보기를 만들어야만 보인다
pub const PREVIEW_PX: u32 = 2560;
pub const PREVIEW_QUALITY: f64 = 0.85;

/// 뷰어 미리보기 캐시 폴더. 썸네일과 나눠 둔다 — 지울 때 따로 지우기 위해서다.
pub fn preview_root(library_root: &Path) -> PathBuf {
    library_root.join(".acut").join("previews")
}

/// 라이브러리 루트 안의 캐시 폴더.
///
/// **볼륨 마운트가 아니라 라이브러리 루트 기준**이다. 부팅 볼륨의 마운트는 `/`라
/// 볼륨 기준으로 하면 `/.acut`에 쓰려다 권한 오류가 난다. 라이브러리가 어디에
/// 있든 그 폴더 안에 캐시가 함께 있는 편이 옮기기도 쉽다.
pub fn cache_root(library_root: &Path) -> PathBuf {
    library_root.join(".acut").join("thumbs")
}

/// 폴더 상대경로와 파일명을 볼륨 기준 상대경로로 합친다.
///
/// 썸네일 일괄 생성과 뷰어 미리보기가 **같은 문자열**을 만들어야 한다. 어긋나면
/// 캐시 키가 달라져 같은 사진을 두 번 만들고, 원본 경로도 어긋난다.
pub fn rel_path(rel_dir: &str, name: &str) -> String {
    if rel_dir.is_empty() {
        name.to_string()
    } else {
        format!("{rel_dir}/{name}")
    }
}

/// 캐시 키 — 원본이 바뀌면 값이 바뀐다.
///
/// 경로·크기·수정시각을 함께 넣는다. 같은 이름의 다른 파일, 또는 편집된 파일이
/// 이전 썸네일을 재사용하지 않게 하기 위해서다.
pub fn key_for(rel_path: &str, size: u64, mtime: i64) -> String {
    // FNV-1a 64bit — 암호학적 강도가 필요 없고 빠르면 된다.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    };
    feed(rel_path.as_bytes());
    feed(&size.to_le_bytes());
    feed(&mtime.to_le_bytes());
    format!("{h:016x}")
}

/// 키에 해당하는 캐시 파일 경로. 앞 2자리로 샤딩해 한 폴더에 수만 개가 쌓이지 않게 한다.
pub fn thumb_path(root: &Path, key: &str) -> PathBuf {
    root.join(&key[0..2]).join(format!("{key}.jpg"))
}

/// 캐시 루트 기준 상대경로 (DB의 `thumbs.rel_path`에 저장할 값).
pub fn thumb_rel(key: &str) -> String {
    format!("{}/{}.jpg", &key[0..2], key)
}

/// 캐시 전체 용량과 개수. 설정 화면에서 보여준다.
pub fn cache_stats(root: &Path) -> (u64, usize) {
    let mut bytes = 0;
    let mut count = 0;
    let Ok(shards) = std::fs::read_dir(root) else {
        return (0, 0);
    };
    for shard in shards.flatten() {
        let Ok(files) = std::fs::read_dir(shard.path()) else { continue };
        for f in files.flatten() {
            if let Ok(m) = f.metadata() {
                bytes += m.len();
                count += 1;
            }
        }
    }
    (bytes, count)
}

/// 캐시를 통째로 지운다. 원본은 건드리지 않는다.
pub fn clear(root: &Path) -> std::io::Result<()> {
    if root.exists() {
        std::fs::remove_dir_all(root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_lives_inside_the_library_root() {
        let root = cache_root(Path::new("/Volumes/PHOTO 1"));
        assert_eq!(root, Path::new("/Volumes/PHOTO 1/.acut/thumbs"));
        // 하위 폴더를 라이브러리로 잡아도 그 안에 생긴다
        let sub = cache_root(Path::new("/Volumes/PHOTO 1/내사진"));
        assert_eq!(sub, Path::new("/Volumes/PHOTO 1/내사진/.acut/thumbs"));
    }

    #[test]
    fn rel_path_joins_the_same_way_everywhere() {
        assert_eq!(rel_path("2018/여행", "a.jpg"), "2018/여행/a.jpg");
        // 루트 바로 아래 파일은 앞에 슬래시가 붙으면 안 된다
        assert_eq!(rel_path("", "a.jpg"), "a.jpg");
    }

    #[test]
    fn previews_do_not_share_the_thumbnail_folder() {
        // 같은 키를 써도 폴더가 갈려야 한다. 섞이면 그리드가 2560px을 읽어
        // 느려지고, 캐시를 지울 때 둘 다 날아간다.
        let lib = Path::new("/Volumes/PHOTO 1");
        assert_eq!(preview_root(lib), Path::new("/Volumes/PHOTO 1/.acut/previews"));
        assert_ne!(preview_root(lib), cache_root(lib));

        let key = key_for("2018/a.jpg", 100, 1000);
        assert_ne!(
            thumb_path(&preview_root(lib), &key),
            thumb_path(&cache_root(lib), &key),
        );
    }

    #[test]
    fn preview_is_larger_than_the_grid_thumbnail() {
        assert!(PREVIEW_PX > THUMB_PX, "뷰어가 썸네일을 확대해 보여주면 안 된다");
        assert!(PREVIEW_QUALITY > THUMB_QUALITY);
    }

    #[test]
    fn key_changes_when_the_source_changes() {
        let a = key_for("2018/a.jpg", 100, 1000);
        assert_eq!(a, key_for("2018/a.jpg", 100, 1000), "같은 입력이면 같은 키");
        assert_ne!(a, key_for("2018/a.jpg", 101, 1000), "크기가 바뀌면");
        assert_ne!(a, key_for("2018/a.jpg", 100, 1001), "수정시각이 바뀌면");
        assert_ne!(a, key_for("2018/b.jpg", 100, 1000), "경로가 바뀌면");
    }

    #[test]
    fn key_is_stable_across_runs() {
        // DB를 새로 만들어도 같은 값이 나와야 재생성을 피할 수 있다
        assert_eq!(key_for("x/y.jpg", 42, 7), key_for("x/y.jpg", 42, 7));
        assert_eq!(key_for("x/y.jpg", 42, 7).len(), 16);
    }

    #[test]
    fn paths_are_sharded() {
        let key = key_for("a.jpg", 1, 1);
        let p = thumb_path(Path::new("/root"), &key);
        // /root/<앞 2자>/<키>.jpg
        assert!(p.starts_with("/root"));
        assert_eq!(p.extension().unwrap(), "jpg");
        let parent = p.parent().unwrap().file_name().unwrap().to_string_lossy();
        assert_eq!(parent.len(), 2, "2자리 샤딩: {parent}");
        assert_eq!(thumb_rel(&key), format!("{}/{}.jpg", &key[0..2], key));
    }

    #[test]
    fn stats_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("thumbs");
        std::fs::create_dir_all(root.join("ab")).unwrap();
        std::fs::write(root.join("ab").join("abcd.jpg"), b"12345").unwrap();
        let (bytes, count) = cache_stats(&root);
        assert_eq!((bytes, count), (5, 1));
        clear(&root).unwrap();
        assert!(!root.exists());
        assert_eq!(cache_stats(&root), (0, 0), "없는 폴더는 0");
    }
}
