//! 썸네일 캐시 — 어디에 저장하고 언제 다시 만드는가.
//!
//! **캐시는 앱 데이터 폴더 안에 라이브러리별로 둔다.**
//!
//! 원래는 라이브러리 볼륨 안(`.acut/thumbs/`)에 뒀다. 앱 ID가 바뀌었을 때
//! 썸네일 경로 64,698건이 죽은 적이 있어서, 디스크를 따라다니게 하려던
//! 것이었다. 그런데 실측해 보니 대가가 컸다:
//!
//!   - 두 라이브러리 볼륨이 모두 exFAT이다. 작은 파일 쓰기가 내장 APFS의
//!     **1/5.9** (59장/초 vs 347장/초).
//!   - exFAT은 확장 속성을 못 담아 macOS가 파일마다 `._이름` 사이드카를
//!     16KB씩 만든다. 실측 PHOTO 1에서만 64,108개 = 1,001MB.
//!
//! 그래서 DB 옆으로 옮겼다. 앱 ID가 바뀌면 DB와 캐시가 **함께** 사라지므로
//! 죽은 참조가 남지 않는다 — 그때는 그냥 다시 만들면 된다.
//! 대신 디스크를 다른 맥에 꽂으면 썸네일은 따라가지 않는다.
//!
//! 파일명은 `볼륨 상대경로 + 크기 + 수정시각`의 해시다. DB를 새로 만들어도
//! 같은 이름이 나오므로 재생성하지 않는다.

use std::path::{Path, PathBuf};

/// 그리드 썸네일의 긴 변. 화면에서 최대 320px로 쓰므로 레티나(2배)를 감안했다.
pub const THUMB_PX: u32 = 640;
pub const THUMB_QUALITY: f64 = 0.72;

/// 1차 스캔에서 **박힌 미리보기를 받아들이는 최소 크기**.
///
/// 이 한 숫자가 첫 스캔의 체감을 정한다. 실측(사진 80장, 목표 640px):
/// 기준 640이면 7.7장/초, 기준 160이면 177.7장/초 — 23배다.
/// 160이면 거의 모든 JPEG이 통과해 **원본을 한 번도 안 읽는다**.
/// 7만 8천 장이 몇 분에 끝난다. Lap이 하는 것과 같다.
///
/// 대신 썸네일이 작게(평균 448px) 나온다. Lap도 같은 성질이다 — 그리드에
/// 쓸 크기만 만들고, 크게 볼 때는 뷰어가 원본에서 따로 뽑는다.
pub const FAST_ACCEPT_PX: u32 = 160;

/// 뷰어용 미리보기. 5K 화면에서 전체화면으로 봐도 견디는 크기.
///
/// 원본을 그대로 주지 않는 이유가 둘이다:
///   - 5760×3840 JPEG는 5MB가 넘어 넘기기 버겁다
///   - **RAW는 웹뷰가 못 읽는다.** ImageIO로 미리보기를 만들어야만 보인다
pub const PREVIEW_PX: u32 = 2560;
pub const PREVIEW_QUALITY: f64 = 0.85;
const _: () = assert!(PREVIEW_PX > THUMB_PX);
const _: () = assert!(PREVIEW_QUALITY > THUMB_QUALITY);

/// 뷰어 미리보기 캐시 폴더. 썸네일과 나눠 둔다 — 지울 때 따로 지우기 위해서다.
pub fn preview_root(base: &Path, library_id: i64) -> PathBuf {
    base.join("previews").join(library_id.to_string())
}

/// 라이브러리의 썸네일 캐시 폴더. `base`는 앱 데이터 폴더다.
///
/// 라이브러리마다 나누는 이유: 등록을 지울 때 그 라이브러리 것만 지우면 된다.
pub fn cache_root(base: &Path, library_id: i64) -> PathBuf {
    base.join("thumbs").join(library_id.to_string())
}

/// 옛 위치 — 라이브러리 볼륨 안. 옮겨 오기 위해서만 쓴다.
pub fn legacy_root(library_dir: &Path) -> PathBuf {
    library_dir.join(".acut").join("thumbs")
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

/// 옛 위치(라이브러리 볼륨 안)의 캐시를 새 위치로 옮긴다.
///
/// 이미 만들어 둔 것을 버리지 않기 위해서다 — 실측 12만 장이 쌓여 있었고,
/// 다시 만들려면 390GB를 또 읽어야 한다. 사이드카(`._…`)는 옮기지 않는다.
///
/// 옮긴 개수를 돌려준다. 옛 폴더가 없으면 0.
pub fn migrate_from_legacy(legacy: &Path, dest: &Path) -> (usize, usize) {
    let Ok(shards) = std::fs::read_dir(legacy) else {
        return (0, 0);
    };
    let (mut moved, mut failed) = (0, 0);
    for shard in shards.flatten() {
        let Ok(files) = std::fs::read_dir(shard.path()) else { continue };
        let Some(shard_name) = shard.file_name().to_str().map(str::to_string) else { continue };
        let out_dir = dest.join(&shard_name);
        if std::fs::create_dir_all(&out_dir).is_err() {
            failed += 1;
            continue;
        }
        for f in files.flatten() {
            let name = f.file_name();
            if name.to_string_lossy().starts_with("._") {
                let _ = std::fs::remove_file(f.path());
                continue;
            }
            let to = out_dir.join(&name);
            if to.exists() {
                let _ = std::fs::remove_file(f.path());
                moved += 1;
                continue;
            }
            // 볼륨이 다르므로 rename은 안 된다. 복사 후 원본을 지운다.
            match std::fs::copy(f.path(), &to) {
                Ok(_) => {
                    let _ = std::fs::remove_file(f.path());
                    moved += 1;
                }
                Err(_) => failed += 1,
            }
        }
        let _ = std::fs::remove_dir(shard.path());
    }
    let _ = std::fs::remove_dir(legacy);
    (moved, failed)
}

/// macOS가 exFAT에 만드는 AppleDouble 사이드카를 치운다.
///
/// 두 라이브러리 볼륨이 모두 exFAT인데, exFAT은 확장 속성을 담지 못한다.
/// 그래서 macOS가 파일마다 `._이름` 사이드카를 하나 더 만든다 —
/// **한 장에 16KB씩**이다. 실측: PHOTO 1에 64,108개 = 1,001MB.
///
/// 우리 캐시 파일에 붙는 확장 속성은 `com.apple.provenance` 하나뿐이고
/// 우리가 쓰지 않는다. 사이드카를 지워도 잃는 것이 없다.
///
/// 캐시 폴더 안만 훑는다. 원본 사진 쪽은 건드리지 않는다.
pub fn purge_sidecars(root: &Path) -> (usize, u64) {
    let mut n = 0;
    let mut bytes = 0;
    let Ok(shards) = std::fs::read_dir(root) else {
        return (0, 0);
    };
    for shard in shards.flatten() {
        let Ok(files) = std::fs::read_dir(shard.path()) else { continue };
        for f in files.flatten() {
            if !f.file_name().to_string_lossy().starts_with("._") {
                continue;
            }
            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(f.path()).is_ok() {
                n += 1;
                bytes += size;
            }
        }
    }
    (n, bytes)
}

/// 캐시 전체 용량과 개수. 설정 화면에서 보여준다.
///
/// 사이드카(`._…`)는 세지 않는다 — 우리 캐시가 아니라 파일시스템이 만든 것이다.
pub fn cache_stats(root: &Path) -> (u64, usize) {
    let mut bytes = 0;
    let mut count = 0;
    let Ok(shards) = std::fs::read_dir(root) else {
        return (0, 0);
    };
    for shard in shards.flatten() {
        let Ok(files) = std::fs::read_dir(shard.path()) else { continue };
        for f in files.flatten() {
            if f.file_name().to_string_lossy().starts_with("._") {
                continue;
            }
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
    fn cache_lives_next_to_the_database() {
        let base = Path::new("/Users/me/Library/Application Support/com.acut.media");
        assert_eq!(cache_root(base, 2), base.join("thumbs/2"));
        assert_eq!(preview_root(base, 2), base.join("previews/2"));
        // 라이브러리마다 나뉘어야 등록을 지울 때 그것만 지운다
        assert_ne!(cache_root(base, 1), cache_root(base, 2));
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
        let base = Path::new("/base");
        assert_ne!(preview_root(base, 1), cache_root(base, 1));
        let key = key_for("2018/a.jpg", 100, 1000);
        assert_ne!(
            thumb_path(&preview_root(base, 1), &key),
            thumb_path(&cache_root(base, 1), &key),
        );
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

    /// exFAT에서 macOS가 만드는 `._` 사이드카는 우리 캐시가 아니다.
    /// 세면 캐시 용량이 두 배로 보이고, 지우면 1GB가 돌아온다.
    #[test]
    fn legacy_cache_moves_without_losing_work() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let legacy = old.path().join(".acut").join("thumbs");
        std::fs::create_dir_all(legacy.join("ab")).unwrap();
        std::fs::write(legacy.join("ab").join("abcd.jpg"), b"thumb").unwrap();
        std::fs::write(legacy.join("ab").join("._abcd.jpg"), vec![0u8; 16384]).unwrap();

        let dest = new.path().join("thumbs").join("1");
        let (moved, failed) = migrate_from_legacy(&legacy, &dest);
        assert_eq!((moved, failed), (1, 0));
        assert!(dest.join("ab").join("abcd.jpg").is_file(), "썸네일은 살아 온다");
        assert!(!dest.join("ab").join("._abcd.jpg").exists(), "사이드카는 안 옮긴다");
        assert!(!legacy.exists(), "옛 폴더는 치운다");

        // 옛 폴더가 없으면 아무 일도 없다
        assert_eq!(migrate_from_legacy(&legacy, &dest), (0, 0));
    }

    #[test]
    fn sidecars_are_not_counted_and_can_be_purged() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("thumbs");
        std::fs::create_dir_all(root.join("ab")).unwrap();
        std::fs::write(root.join("ab").join("abcd.jpg"), b"12345").unwrap();
        std::fs::write(root.join("ab").join("._abcd.jpg"), vec![0u8; 16384]).unwrap();

        assert_eq!(cache_stats(&root), (5, 1), "사이드카는 캐시 용량이 아니다");

        let (n, bytes) = purge_sidecars(&root);
        assert_eq!((n, bytes), (1, 16384));
        assert!(root.join("ab").join("abcd.jpg").is_file(), "진짜 캐시는 남는다");
        assert!(!root.join("ab").join("._abcd.jpg").exists());

        // 두 번 돌려도 안전하다
        assert_eq!(purge_sidecars(&root), (0, 0));
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

#[cfg(test)]
mod audit {
    use super::*;

    /// 캐시에 실제 사진보다 많은 파일이 쌓였는지 본다.
    /// `cargo test --lib media::cache::audit -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 라이브러리 필요"]
    fn cache_has_no_orphans() {
        let base = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join("Library/Application Support/com.acut.media");
        let db_path = base.join("acut-v2.db");
        if !db_path.is_file() {
            return;
        }
        let c = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();

        let ids: Vec<i64> = {
            let mut st = c.prepare("SELECT id FROM libraries").unwrap();
            let it = st.query_map([], |r| r.get(0)).unwrap();
            it.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };

        for id in ids {
            let root = cache_root(&base, id);
            if !root.is_dir() {
                println!("라이브러리 {id}: 캐시 폴더 없음");
                continue;
            }
            let mut want = std::collections::HashSet::new();
            let mut st = c
                .prepare(
                    "SELECT fo.rel_path || CASE WHEN fo.rel_path='' THEN '' ELSE '/' END || fi.name,
                            fi.size, COALESCE(fi.modified_at,0)
                     FROM files fi JOIN folders fo ON fo.id=fi.folder_id
                     WHERE fo.library_id = ?1",
                )
                .unwrap();
            let rows = st
                .query_map([id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
                })
                .unwrap();
            for row in rows {
                let (rel, size, mtime) = row.unwrap();
                want.insert(key_for(&rel, size as u64, mtime));
            }

            let (mut have, mut orphan) = (0usize, 0usize);
            for shard in std::fs::read_dir(&root).unwrap().flatten() {
                let Ok(files) = std::fs::read_dir(shard.path()) else { continue };
                for f in files.flatten() {
                    have += 1;
                    let n = f.file_name().to_string_lossy().into_owned();
                    if !want.contains(n.trim_end_matches(".jpg")) {
                        orphan += 1;
                    }
                }
            }
            println!("라이브러리 {id}: 파일 {} · 캐시 {have} · 고아 {orphan}", want.len());
        }
    }
}
