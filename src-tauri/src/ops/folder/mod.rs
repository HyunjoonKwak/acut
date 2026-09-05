//! 일반 폴더 작업 — 생성·이름 변경·이동·복사·휴지통과 배치 되돌리기.
//!
//! 경로는 모두 라이브러리 기준 상대경로로 받고, 실제 경로를 만들기 전에 `..`,
//! 심볼릭 링크, 라이브러리 루트 작업을 막는다. 폴더 복사와 볼륨 간 이동은 임시
//! 갈래에 전부 복사한 뒤 SHA-256 manifest를 확인하고 마지막에 이름을 바꾼다.

use crate::db::conn::{Db, DbError, IoContext, Result};
use crate::db::libraries::Library;
use crate::ops::trash::{copy_mtime, free_path, Outcome};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Create,
    Rename,
    Move,
    Copy,
    Trash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    Skip,
    Rename,
}

fn default_policy() -> ConflictPolicy {
    ConflictPolicy::Skip
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub action: Action,
    pub source_library_id: i64,
    /// 생성에서는 부모, 나머지에서는 작업할 폴더. 라이브러리 기준 상대경로.
    pub source_dir: String,
    pub destination_library_id: Option<i64>,
    pub destination_parent: Option<String>,
    pub name: Option<String>,
    #[serde(default = "default_policy")]
    pub conflict_policy: ConflictPolicy,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Preview {
    pub source: String,
    pub destination: String,
    pub planned_name: String,
    pub conflict: String,
    pub action: String,
    pub files: usize,
    pub directories: usize,
    pub bytes: u64,
    pub cross_volume: bool,
    pub drive_sync_warning: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FolderOutcome {
    pub batch_id: i64,
    pub completed: usize,
    pub failed: usize,
    pub files: usize,
    pub directories: usize,
    pub bytes: u64,
    pub first_error: Option<String>,
    pub manifest_sha256: Option<String>,
}

#[derive(Debug, Clone)]
struct Manifest {
    /// 파일 내용 SHA-256 을 섞은 다이제스트. 내용을 읽지 않은 manifest 는 빈 문자열.
    sha256: String,
    /// 이름·크기·mtime 만 섞은 다이제스트. 같은 볼륨 이름변경·이동·휴지통의 undo 대조 기준.
    stat_sha256: String,
    files: usize,
    directories: usize,
    bytes: u64,
    file_hashes: HashMap<String, String>,
}

struct TreeEntry {
    stat_line: String,
    content_line: String,
    /// 파일이면 (상대경로, 내용 해시). 내용을 읽지 않았으면 해시는 None.
    file: Option<(String, Option<String>)>,
    size: u64,
}

fn bad(message: impl Into<String>) -> DbError {
    DbError::Invalid(message.into())
}

fn clean_rel(value: &str, allow_empty: bool) -> Result<String> {
    let clean = crate::scan::nfc(value.trim().trim_matches('/'));
    if clean.is_empty() {
        return if allow_empty {
            Ok(clean)
        } else {
            Err(bad("라이브러리 루트에는 이 작업을 할 수 없습니다"))
        };
    }
    let path = Path::new(&clean);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
        || clean.split('/').any(bad_name)
    {
        return Err(bad("라이브러리 안의 안전한 상대 폴더를 지정하세요"));
    }
    Ok(clean)
}

fn clean_name(value: Option<&str>) -> Result<String> {
    let name = crate::scan::nfc(value.unwrap_or_default().trim());
    if name.is_empty() || name.contains('/') || bad_name(&name) {
        return Err(bad("비어 있거나 사용할 수 없는 폴더 이름입니다"));
    }
    Ok(name)
}

fn bad_name(part: &str) -> bool {
    part.is_empty()
        || part == "."
        || part == ".."
        || part.starts_with('.')
        || part.contains(':')
        || part.chars().any(|c| (c as u32) < 0x20)
}

fn library(db: &Db, id: i64) -> Result<Library> {
    crate::db::libraries::get(db, id)?.ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))
}

fn online_root(lib: &Library) -> Result<PathBuf> {
    let root = lib
        .dir
        .clone()
        .ok_or_else(|| bad(format!("「{}」 디스크가 연결되어 있지 않습니다", lib.name)))?;
    let root = root.canonicalize().map_err(|_| {
        bad(format!(
            "「{}」 라이브러리 경로를 확인할 수 없습니다",
            lib.name
        ))
    })?;
    if !root.is_dir() {
        return Err(bad(format!(
            "「{}」 라이브러리가 폴더가 아닙니다",
            lib.name
        )));
    }
    Ok(root)
}

fn existing_inside(root: &Path, rel: &str) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .io_context("라이브러리 루트 경로를 확인하다가 실패했습니다")?;
    let path = root.join(rel);
    let real = path
        .canonicalize()
        .map_err(|_| bad(format!("폴더가 없습니다: {rel}")))?;
    if !real.starts_with(&root) || !real.is_dir() {
        return Err(bad("라이브러리 밖이거나 폴더가 아닌 경로입니다"));
    }
    Ok(real)
}

fn parent_inside(root: &Path, rel: &str) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .io_context("목적지 라이브러리 경로를 확인하다가 실패했습니다")?;
    let path = if rel.is_empty() {
        root.clone()
    } else {
        root.join(rel)
    };
    let real = path
        .canonicalize()
        .map_err(|_| bad(format!("목적지 부모 폴더가 없습니다: {rel}")))?;
    if !real.starts_with(&root) || !real.is_dir() {
        return Err(bad("목적지 부모가 라이브러리 밖이거나 폴더가 아닙니다"));
    }
    Ok(real)
}

fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn is_same_or_child(parent: &str, candidate: &str) -> bool {
    candidate == parent
        || candidate
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn is_appledouble(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("._"))
}

fn appledouble_sibling(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy();
    Some(path.with_file_name(format!("._{name}")))
}

/// exFAT에서는 디렉터리를 지우는 도중 macOS가 `._이름`을 뒤늦게 만들 수 있어
/// `remove_dir_all`이 DirectoryNotEmpty로 끝나는 경우가 있다. 검증된 작업 경로
/// 안쪽을 bottom-up으로 다시 비우고, 그 경로의 AppleDouble sibling까지 정리한다.
pub(crate) fn remove_tree(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        if let Some(sidecar) = appledouble_sibling(path) {
            let _ = std::fs::remove_file(sidecar);
        }
        return Ok(());
    }
    let first_error = match std::fs::remove_dir_all(path) {
        Ok(()) => {
            if let Some(sidecar) = appledouble_sibling(path) {
                let _ = std::fs::remove_file(sidecar);
            }
            return Ok(());
        }
        Err(error) => error,
    };
    for _ in 0..4 {
        if !path.exists() {
            break;
        }
        // 첫 판에서 뒤늦게 생긴 AppleDouble을 다음 판의 표준 구현이 다시
        // 열거하도록 한다. exFAT에서는 이 재시도만으로 끝나는 경우가 대부분이다.
        let _ = std::fs::remove_dir_all(path);
        if !path.exists() {
            break;
        }
        let entries = WalkDir::new(path)
            .min_depth(1)
            .contents_first(true)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .map(|entry| {
                // 일부 exFAT 드라이버는 read_dir에서 NFD 이름을 주지만 삭제 syscall은
                // NFC 이름만 찾는다. 스캔·DB와 같은 정규형으로 되돌려 삭제한다.
                let normalized = crate::scan::nfc(&entry.path().to_string_lossy());
                (PathBuf::from(normalized), entry.file_type())
            })
            .collect::<Vec<_>>();
        for (entry, kind) in entries {
            if kind.is_dir() {
                std::fs::remove_dir(&entry)
            } else {
                std::fs::remove_file(&entry)
            }
            .ok();
            if let Some(sidecar) = appledouble_sibling(&entry) {
                let _ = std::fs::remove_file(sidecar);
            }
        }
        let _ = std::fs::remove_dir(path);
    }
    if let Some(sidecar) = appledouble_sibling(path) {
        let _ = std::fs::remove_file(sidecar);
    }
    if path.exists() {
        Err(first_error)
    } else {
        Ok(())
    }
}

/// 폴더 트리를 읽어 manifest 를 만든다. `hash_contents` 가 참이면 파일 내용의 SHA-256 도
/// 계산한다 — 복사·볼륨 간 이동의 사본 검증에 쓴다. 거짓이면 이름·크기·mtime 만 읽어 큰
/// 폴더도 곧 끝난다. 같은 볼륨 이름변경·이동·휴지통은 rename 한 번이라 이 값으로 충분하고,
/// undo 도 같은 값으로 «그새 바뀌었나»를 본다 (2차 리뷰 M-11).
fn walk_tree(root: &Path, hash_contents: bool) -> Result<Manifest> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter() {
        let entry = entry.map_err(|e| bad(e.to_string()))?;
        if entry.file_type().is_symlink() {
            return Err(bad(format!(
                "심볼릭 링크가 든 폴더는 안전하게 작업할 수 없습니다: {}",
                entry.path().display()
            )));
        }
        if entry.file_type().is_file() && is_appledouble(entry.path()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| bad("폴더 manifest 경로를 계산하지 못했습니다"))?
            .to_string_lossy();
        // APFS는 NFC, exFAT은 NFD로 보일 수 있다. 같은 이름의 같은 파일을
        // 볼륨 표현 차이 때문에 다른 manifest로 판정하지 않는다.
        let rel = crate::scan::nfc(&rel);
        if entry.file_type().is_dir() {
            entries.push(TreeEntry {
                stat_line: format!("D\0{rel}"),
                content_line: format!("D\0{rel}"),
                file: None,
                size: 0,
            });
        } else if entry.file_type().is_file() {
            let meta = entry.metadata().map_err(|e| bad(e.to_string()))?;
            let size = meta.len();
            let mtime = filetime::FileTime::from_last_modification_time(&meta).unix_seconds();
            let hash = if hash_contents {
                Some(
                    crate::cull::hash::full(entry.path())
                        .io_context("폴더 manifest용 파일 해시를 읽다가 실패했습니다")?,
                )
            } else {
                None
            };
            entries.push(TreeEntry {
                stat_line: format!("F\0{rel}\0{size}\0{mtime}"),
                content_line: hash
                    .as_deref()
                    .map(|hash| format!("F\0{rel}\0{size}\0{hash}"))
                    .unwrap_or_default(),
                file: Some((rel, hash)),
                size,
            });
        } else {
            return Err(bad(format!(
                "지원하지 않는 폴더 항목입니다: {}",
                entry.path().display()
            )));
        }
    }
    // 상대경로가 유일하므로 stat 줄 순서와 내용 줄 순서는 같다 — 내용 다이제스트는
    // 0.9.1 저널이 남긴 값과 바이트 단위로 같아야 한다.
    entries.sort_by(|a, b| a.stat_line.cmp(&b.stat_line));
    let mut content = Sha256::new();
    let mut stat = Sha256::new();
    let mut file_hashes = HashMap::new();
    let mut files = 0usize;
    let mut directories = 0usize;
    let mut bytes = 0u64;
    for entry in entries {
        stat.update(entry.stat_line.as_bytes());
        stat.update(b"\n");
        if hash_contents {
            content.update(entry.content_line.as_bytes());
            content.update(b"\n");
        }
        match entry.file {
            Some((rel, hash)) => {
                files += 1;
                bytes = bytes
                    .checked_add(entry.size)
                    .ok_or_else(|| bad("폴더 용량이 표현 범위를 넘습니다"))?;
                if let Some(hash) = hash {
                    file_hashes.insert(rel, hash);
                }
            }
            None => directories += 1,
        }
    }
    Ok(Manifest {
        sha256: if hash_contents {
            format!("{:x}", content.finalize())
        } else {
            String::new()
        },
        stat_sha256: format!("{:x}", stat.finalize()),
        files,
        directories,
        bytes,
        file_hashes,
    })
}

/// 내용 해시까지 든 manifest — 복사·볼륨 간 이동의 사본 검증용.
fn manifest(root: &Path) -> Result<Manifest> {
    walk_tree(root, true)
}

/// 이름·크기·mtime 만 읽은 manifest — 미리보기와 같은 볼륨 작업용. 파일 내용은 읽지 않는다.
fn tree_summary(root: &Path) -> Result<Manifest> {
    walk_tree(root, false)
}

fn planned(
    request: &Request,
    db: &Db,
) -> Result<(Library, Library, String, String, PathBuf, PathBuf)> {
    let source_lib = library(db, request.source_library_id)?;
    let source_root = online_root(&source_lib)?;
    let source_rel = clean_rel(&request.source_dir, request.action == Action::Create)?;

    if request.action == Action::Create {
        let parent = parent_inside(&source_root, &source_rel)?;
        let name = clean_name(request.name.as_deref())?;
        let dest_rel = join_rel(&source_rel, &name);
        let destination = source_root.join(&dest_rel);
        return Ok((
            source_lib.clone(),
            source_lib,
            source_rel,
            dest_rel,
            parent,
            destination,
        ));
    }

    let source = existing_inside(&source_root, &source_rel)?;
    if request.action == Action::Trash {
        let trash_parent = source_root.join(".acut/휴지통");
        let wanted = trash_parent.join(&source_rel);
        let dest = if wanted.exists() {
            free_path(wanted)
        } else {
            wanted
        };
        let dest_rel = dest
            .strip_prefix(&source_root)
            .map_err(|_| bad("휴지통 경로 오류"))?
            .to_string_lossy()
            .into_owned();
        return Ok((
            source_lib.clone(),
            source_lib,
            source_rel,
            dest_rel,
            source,
            dest,
        ));
    }

    // 이름 변경은 같은 부모 안의 이름만 바꾼다. 숨겨진 UI 상태나 직접 invoke한
    // payload가 다른 라이브러리를 넣어도 물리 이동으로 변질되면 안 된다.
    let destination_id = if request.action == Action::Rename {
        source_lib.id
    } else {
        request.destination_library_id.unwrap_or(source_lib.id)
    };
    let destination_lib = library(db, destination_id)?;
    let destination_root = online_root(&destination_lib)?;
    let parent_rel = if request.action == Action::Rename {
        source_rel
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("")
            .to_string()
    } else {
        clean_rel(request.destination_parent.as_deref().unwrap_or(""), true)?
    };
    let parent = parent_inside(&destination_root, &parent_rel)?;
    let name = match request.action {
        Action::Rename => clean_name(request.name.as_deref())?,
        _ => request
            .name
            .as_deref()
            .map(|_| clean_name(request.name.as_deref()))
            .transpose()?
            .unwrap_or_else(|| source_rel.rsplit('/').next().unwrap_or("폴더").to_string()),
    };
    let destination_rel = join_rel(&parent_rel, &name);
    if source_lib.id == destination_lib.id
        && matches!(request.action, Action::Move | Action::Rename | Action::Copy)
        && is_same_or_child(&source_rel, &destination_rel)
    {
        return Err(bad("폴더를 자기 자신이나 자기 하위로 옮길 수 없습니다"));
    }
    Ok((
        source_lib,
        destination_lib,
        source_rel,
        destination_rel,
        source,
        parent.join(name),
    ))
}

struct OperationPlan {
    preview: Preview,
    info: Manifest,
    source_lib: Library,
    destination_lib: Library,
    source_rel: String,
    source: PathBuf,
    destination: PathBuf,
}

enum PlanDetail {
    Summary,
    Verified,
}

fn operation_plan(db: &Db, request: &Request, detail: PlanDetail) -> Result<OperationPlan> {
    let (source_lib, destination_lib, source_rel, destination_rel, source, wanted) =
        planned(request, db)?;
    let cross_volume = source_lib.volume_uuid != destination_lib.volume_uuid;
    let info = if request.action == Action::Create {
        Manifest {
            sha256: String::new(),
            stat_sha256: String::new(),
            files: 0,
            directories: 1,
            bytes: 0,
            file_hashes: HashMap::new(),
        }
    } else {
        // 내용 해시는 복사와 볼륨 간 이동의 사본 검증에만 필요하다. 같은 볼륨의
        // 이름변경·이동·휴지통은 rename 한 번이라 이름·크기·mtime 만 읽는다.
        let needs_hashes = matches!(detail, PlanDetail::Verified)
            && (request.action == Action::Copy || (request.action == Action::Move && cross_volume));
        if needs_hashes {
            manifest(&source)?
        } else {
            tree_summary(&source)?
        }
    };
    let same_existing_folder = request.action == Action::Rename
        && wanted.exists()
        && matches!(
            (source.canonicalize(), wanted.canonicalize()),
            (Ok(source), Ok(wanted)) if source == wanted
        );
    let exists = wanted.exists() && !same_existing_folder;
    let (conflict, action, destination) = if exists && request.action != Action::Trash {
        match request.conflict_policy {
            ConflictPolicy::Skip => ("name_exists", "skip", wanted),
            ConflictPolicy::Rename => ("name_exists", "rename", free_path(wanted)),
        }
    } else if request.action == Action::Trash
        && destination_rel != join_rel(".acut/휴지통", &source_rel)
    {
        ("name_exists", "rename", wanted)
    } else {
        ("none", "run", wanted)
    };
    let destination_path = destination;
    let destination = destination_path
        .strip_prefix(online_root(&destination_lib)?)
        .map_err(|_| bad("목적지 경로가 라이브러리 밖입니다"))?
        .to_string_lossy()
        .into_owned();
    let preview = Preview {
        source: source_rel,
        planned_name: destination
            .rsplit('/')
            .next()
            .unwrap_or(&destination)
            .to_string(),
        destination,
        conflict: conflict.into(),
        action: action.into(),
        files: info.files,
        directories: info.directories,
        bytes: info.bytes,
        cross_volume,
        drive_sync_warning: [source_lib.area, destination_lib.area]
            .iter()
            .any(|area| [1, 2].contains(area)),
    };
    Ok(OperationPlan {
        source_rel: preview.source.clone(),
        preview,
        info,
        source_lib,
        destination_lib,
        source,
        destination: destination_path,
    })
}

pub fn preview(db: &Db, request: &Request) -> Result<Preview> {
    Ok(operation_plan(db, request, PlanDetail::Summary)?.preview)
}

mod execute;
mod undo;

pub use execute::execute;
pub use undo::undo;

#[cfg(test)]
use execute::{copy_tree_verified, move_db_rows, stage_move, temp_sibling};

#[cfg(test)]
mod tests;
