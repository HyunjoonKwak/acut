//! 휴지통 — 제외로 판정한 사진을 실제로 치운다.
//!
//! **시스템 휴지통이 아니라 라이브러리 안에 둔다.** 이유가 셋이다:
//!   - 같은 볼륨이라 이름만 바뀐다(rename). 71GB를 복사하지 않는다.
//!   - 원래 폴더 구조를 그대로 보존하므로 되돌리기가 정확하다.
//!   - 사용자가 NAS에서 이미 쓰는 `#recycle` 방식과 같다.
//!
//! 파일 행은 **지우지 않는다.** `trashed_at`을 찍을 뿐이다. 그래야 되돌릴 때
//! 평점·판정·태그가 살아남는다. 진짜 삭제는 [`empty`]에서만 일어난다.
//!
//! 모든 이동은 `batches`/`journal`에 남는다. 무엇을 언제 옮겼는지 모르면
//! 되돌릴 수 없다.

use crate::db::conn::{Db, Result};
use crate::db::libraries;
use rusqlite::OptionalExtension;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

type LibrariesById = HashMap<i64, libraries::Library>;
type MountsByVolume = HashMap<String, Option<PathBuf>>;

/// 휴지통 폴더. 라이브러리마다 따로 있다.
pub fn trash_root(library_dir: &Path) -> PathBuf {
    library_dir.join(".acut").join("휴지통")
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Outcome {
    pub batch_id: i64,
    pub moved: usize,
    pub failed: usize,
    pub bytes: i64,
    /// 첫 실패 사유. 전부 나열하면 화면에 담기지 않는다.
    pub first_error: Option<String>,
    /// 부분 실패 뒤 UI가 실패한 사진만 다시 고를 수 있게 한다. 이 값을 채우지
    /// 않는 작업은 빈 배열로 직렬화된다.
    pub failed_ids: Vec<i64>,
    /// 사진이 다 나가 디스크에서 지운 폴더 수(치우기) · 행까지 지운 폴더 수(비우기)
    pub folders_removed: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Summary {
    pub files: i64,
    pub bytes: i64,
}

/// 한 파일에 대해 알아야 할 것.
struct Item {
    id: i64,
    library_id: i64,
    folder_id: i64,
    /// 볼륨 기준 상대경로
    vol_rel: String,
    /// 라이브러리 기준 상대경로 — 휴지통 안에서 이 구조를 그대로 쓴다
    lib_rel: String,
    size: i64,
    volume_uuid: String,
    trash_path: Option<String>,
}

fn load(db: &Db, ids: &[i64], trashed: bool) -> Result<Vec<Item>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT fi.id, fo.library_id, fo.volume_uuid,
                fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name,
                CASE WHEN l.rel_path = '' THEN fo.rel_path
                     ELSE substr(fo.rel_path, length(l.rel_path) + 2) END,
                fi.name, fi.size, fi.folder_id, fi.trash_path
         FROM files fi
         JOIN folders fo ON fo.id = fi.folder_id
         JOIN libraries l ON l.id = fo.library_id
         WHERE fi.id IN ({list})
           AND fi.trashed_at IS {} NULL",
        if trashed { "NOT" } else { "" }
    );
    db.read(|c| {
        let mut st = c.prepare(&sql)?;
        let it = st.query_map([], |r| {
            let lib_dir: String = r.get(4)?;
            let name: String = r.get(5)?;
            Ok(Item {
                id: r.get(0)?,
                library_id: r.get(1)?,
                folder_id: r.get(7)?,
                volume_uuid: r.get(2)?,
                vol_rel: r.get(3)?,
                lib_rel: crate::media::cache::rel_path(&lib_dir, &name),
                size: r.get(6)?,
                trash_path: r.get(8)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 이 파일들이 속한 라이브러리와 볼륨 마운트를 한 번에 찾아 둔다.
fn lookups(db: &Db, items: &[Item]) -> Result<(LibrariesById, MountsByVolume)> {
    let libs = libraries::list(db)?
        .into_iter()
        .map(|l| (l.id, l))
        .collect();
    let mounts = items
        .iter()
        .map(|it| it.volume_uuid.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|u| {
            let m = crate::db::volumes::find_mount(&u);
            (u, m)
        })
        .collect();
    Ok((libs, mounts))
}

/// 겹치지 않는 이름을 찾는다. 같은 이름이 이미 휴지통에 있으면 뒤에 번호를 붙인다.
pub fn free_path(want: PathBuf) -> PathBuf {
    let stem = want.file_stem().map(|s| s.to_string_lossy().into_owned());
    let ext = want.extension().map(|s| s.to_string_lossy().into_owned());
    let dir = want.parent().map(Path::to_path_buf).unwrap_or_default();
    let name_for = |n: &str| match (&stem, &ext) {
        (Some(s), Some(e)) => format!("{s} ({n}).{e}"),
        (Some(s), None) => format!("{s} ({n})"),
        _ => n.to_string(),
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        if !want.exists() {
            return want;
        }
        for n in 2..10_000 {
            let p = dir.join(name_for(&n.to_string()));
            if !p.exists() {
                return p;
            }
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
        return dir.join(name_for(&stamp));
    };
    let existing: std::collections::HashSet<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .collect();
    let wanted_name = want
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase());
    if wanted_name
        .as_ref()
        .is_none_or(|name| !existing.contains(name))
    {
        return want;
    }
    for n in 2..10_000 {
        let name = name_for(&n.to_string());
        if !existing.contains(&name.to_lowercase()) {
            return dir.join(name);
        }
    }
    // 9,999개가 다 찼다 — 원래 경로를 돌려주면 덮어쓴다. 시각을 붙여 빈 이름을 만든다
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    dir.join(name_for(&stamp))
}

/// 파일의 사이드카 — 편집 정보를 담는 `.xmp`(같은 줄기 `IMG_1.xmp`, 또는 전체 이름
/// `IMG_1.CR2.xmp`). 사진이 옮겨질 때 따라가야 한다: RAW 는 로컬 한 벌뿐이라 편집 내용을
/// 잃으면 되찾을 곳이 없다. macOS 가 스스로 관리하는 `._` 파일은 건드리지 않는다.
///
/// (원래 경로의 사이드카, 새 경로에 놓일 이름) 짝. 없는 것은 안 돌려준다.
pub fn sidecars(from: &Path, to: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    let (Some(from_dir), Some(to_dir)) = (from.parent(), to.parent()) else {
        return out;
    };
    let (Some(fname), Some(tname)) = (from.file_name(), to.file_name()) else {
        return out;
    };
    let (fname, tname) = (fname.to_string_lossy(), tname.to_string_lossy());
    let stem = |n: &str| {
        n.rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or_else(|| n.to_string())
    };
    for (a, b) in [
        (
            format!("{}.xmp", stem(&fname)),
            format!("{}.xmp", stem(&tname)),
        ),
        (format!("{fname}.xmp"), format!("{tname}.xmp")),
    ] {
        let p = from_dir.join(&a);
        // 사진 자신이 .xmp 인 경우는 없지만, 줄기 사이드카와 전체 이름 사이드카가 같은 이름이면 한 번만
        if p != from && p.is_file() && !out.iter().any(|(x, _): &(PathBuf, PathBuf)| *x == p) {
            out.push((p, to_dir.join(b)));
        }
    }
    out
}

fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

fn sync_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// 파일과 그 사이드카를 한 덩어리로 옮긴다.
///
/// 사이드카 목적지가 이미 있으면 이름을 임의로 비켜 쓰지 않는다. 실제 목적지를
/// 저널이 모르는 상태에서 undo하면 무관한 XMP를 움직일 수 있기 때문이다. 중간
/// 실패 때는 이미 옮긴 사이드카와 본 파일을 역순으로 원위치시킨다.
pub fn move_with_sidecars(from: &Path, to: &Path) -> std::io::Result<()> {
    let cars = sidecars(from, to);
    if to.exists() && !same_file(from, to) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("목적지 파일이 이미 있습니다: {}", to.display()),
        ));
    }
    for (_, target) in &cars {
        if target.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("목적지 사이드카가 이미 있습니다: {}", target.display()),
            ));
        }
    }
    move_file(from, to)?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (source, target) in cars {
        if let Err(error) = move_file(&source, &target) {
            let mut rollback_error = None;
            for (old, new) in moved.iter().rev() {
                if let Err(rollback) = move_file(new, old) {
                    rollback_error.get_or_insert(rollback);
                }
            }
            if let Err(rollback) = move_file(to, from) {
                rollback_error.get_or_insert(rollback);
            }
            return Err(match rollback_error {
                Some(rollback) => std::io::Error::other(format!(
                    "사이드카 이동 실패: {error}; 원위치 복구도 실패: {rollback}"
                )),
                None => error,
            });
        }
        moved.push((source, target));
    }
    Ok(())
}

/// 사이드카를 지운다 — 휴지통 비우기에서, 사진을 지운 뒤
pub fn remove_sidecars(of: &Path) {
    for (a, _) in sidecars(of, of) {
        let _ = std::fs::remove_file(a);
    }
}

/// 파일을 옮긴다. 같은 볼륨이면 rename, 아니면 복사 후 삭제.
///
/// `.acut`은 라이브러리 안에 있으니 실제로는 언제나 rename이다. 그래도
/// 폴백을 둔다 — 라이브러리가 심볼릭 링크로 다른 장치를 가리킬 수 있다.
pub fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if to.exists() && !same_file(from, to) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("목적지가 이미 있습니다: {}", to.display()),
        ));
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            // 다른 볼륨 — 임시 파일을 완성·동기화하고 SHA-256까지 확인한 뒤
            // 최종 이름으로 바꾼다. 원본 삭제가 실패하면 사본도 거둬 원상태를 지킨다.
            let want = std::fs::metadata(from)?.len();
            let temp = to.with_file_name(format!(
                ".{}.photo-desk-move-{}.tmp",
                to.file_name().unwrap_or_default().to_string_lossy(),
                std::process::id()
            ));
            let _ = std::fs::remove_file(&temp);
            let got = std::fs::copy(from, &temp)?;
            if got != want || std::fs::metadata(&temp).map(|m| m.len()).unwrap_or(0) != want {
                let _ = std::fs::remove_file(&temp);
                return Err(std::io::Error::other(format!(
                    "복사가 끝까지 안 됐습니다 ({got} / {want} bytes) — 디스크가 찼나요?"
                )));
            }
            let source_hash = crate::cull::hash::full(from)?;
            let copied_hash = crate::cull::hash::full(&temp)?;
            if source_hash != copied_hash {
                let _ = std::fs::remove_file(&temp);
                return Err(std::io::Error::other("사본 SHA-256이 원본과 다릅니다"));
            }
            copy_mtime(from, &temp);
            std::fs::File::open(&temp)?.sync_all()?;
            std::fs::rename(&temp, to)?;
            if let Err(error) = sync_parent(to) {
                // 사본이 목적지 이름으로 남은 채 실패를 돌려주면 원본과 사본이 둘 다 남는다
                let _ = std::fs::remove_file(to);
                return Err(error);
            }
            if let Err(error) = std::fs::remove_file(from) {
                let _ = std::fs::remove_file(to);
                return Err(error);
            }
            // 원본은 이미 지워졌다 — 여기서 실패를 돌려주면 호출자가 DB 를 갱신하지 않아
            // 파일은 목적지에, 행은 출발지에 남는다. 동기화 실패는 기록만 한다.
            if let Err(error) = sync_parent(from) {
                log::warn!("원본 폴더 동기화 실패 {}: {error}", from.display());
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// 사본의 수정 시각을 원본대로 맞춘다. `fs::copy`는 내용만 옮겨 옮긴 날이 찍힌다 —
/// 촬영일 없는 파일은 이 값으로 날짜를 잡으니 어긋나면 «오늘 찍은 사진»이 된다 (리뷰 H3)
pub fn copy_mtime(from: &Path, to: &Path) {
    let Ok(m) = std::fs::metadata(from).and_then(|m| m.modified()) else {
        return;
    };
    if let Ok(f) = std::fs::File::options().write(true).open(to) {
        let _ = f.set_modified(m);
    }
}

/// 제외로 판정한 것들을 휴지통으로 옮긴다.
mod ops;
mod summaries;

pub use ops::{empty, prune_empty_dirs, restore, to_trash};
pub use summaries::{pending, pending_in_folders, summary, summary_by_library, LibrarySummary};

#[cfg(test)]
mod tests;
