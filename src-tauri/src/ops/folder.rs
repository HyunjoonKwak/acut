//! 일반 폴더 작업 — 생성·이름 변경·이동·복사·휴지통과 배치 되돌리기.
//!
//! 경로는 모두 라이브러리 기준 상대경로로 받고, 실제 경로를 만들기 전에 `..`,
//! 심볼릭 링크, 라이브러리 루트 작업을 막는다. 폴더 복사와 볼륨 간 이동은 임시
//! 갈래에 전부 복사한 뒤 SHA-256 manifest를 확인하고 마지막에 이름을 바꾼다.

use crate::db::conn::{Db, DbError, Result};
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
    let root = root.canonicalize()?;
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
    let root = root.canonicalize()?;
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
                Some(crate::cull::hash::full(entry.path())?)
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

fn temp_sibling(target: &Path, batch: i64) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{name}.photo-desk-{batch}.tmp"))
}

fn copy_tree_verified(
    source: &Path,
    target: &Path,
    batch: i64,
    fail_after: Option<usize>,
    before: &Manifest,
) -> Result<()> {
    let temp = temp_sibling(target, batch);
    if temp.exists() {
        remove_tree(&temp)?;
    }
    std::fs::create_dir_all(&temp)?;
    let result = (|| -> Result<()> {
        let mut copied = 0usize;
        for entry in WalkDir::new(source).min_depth(1).follow_links(false) {
            let entry = entry.map_err(|e| bad(e.to_string()))?;
            if entry.file_type().is_symlink() {
                return Err(bad("심볼릭 링크가 든 폴더는 복사할 수 없습니다"));
            }
            if entry.file_type().is_file() && is_appledouble(entry.path()) {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(source)
                .map_err(|_| bad("복사 경로 오류"))?;
            let dest = temp.join(rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&dest)?;
            } else if entry.file_type().is_file() {
                if fail_after.is_some_and(|limit| copied >= limit) {
                    return Err(bad("시험용 부분 실패"));
                }
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(entry.path(), &dest)?;
                copy_mtime(entry.path(), &dest);
                std::fs::File::open(&dest)?.sync_all()?;
                copied += 1;
            }
        }
        let after = manifest(&temp)?;
        if before.sha256 != after.sha256 {
            return Err(bad("폴더 사본의 SHA-256 manifest가 원본과 다릅니다"));
        }
        if target.exists() {
            return Err(bad("실행 직전 목적지에 같은 이름이 생겼습니다"));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&temp, target)?;
        if let Some(parent) = target.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = remove_tree(&temp);
        return Err(error);
    }
    Ok(())
}

/// 폴더 이동의 물리 단계. 다른 볼륨이면 검증된 사본을 먼저 완성하고, 원본은
/// 같은 볼륨 안의 임시 백업 이름으로 원자적으로 치워 둔다. DB 갱신 실패 시
/// 호출자가 그 백업을 제자리로 돌릴 수 있다.
fn stage_move(
    source: &Path,
    destination: &Path,
    batch: i64,
    cross_volume: bool,
    info: &Manifest,
) -> Result<Option<PathBuf>> {
    if cross_volume {
        copy_tree_verified(source, destination, batch, None, info)?;
        let backup = free_path(
            source
                .parent()
                .unwrap_or(source)
                .join(format!(".photo-desk-move-{batch}.bak")),
        );
        if let Err(error) = std::fs::rename(source, &backup) {
            let _ = remove_tree(destination);
            return Err(error.into());
        }
        Ok(Some(backup))
    } else {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(source, destination)?;
        Ok(None)
    }
}

fn vol_rel(lib: &Library, rel: &str) -> String {
    crate::media::cache::rel_path(&lib.rel_path, rel)
}

fn rows_in_subtree(db: &Db, library_id: i64, root: &str) -> Result<Vec<(i64, String)>> {
    let root = vol_rel(&library(db, library_id)?, root);
    let escaped = crate::db::query::escape_like(&root);
    db.read(|c| {
        let mut statement = c.prepare(
            "SELECT id,rel_path FROM folders WHERE library_id=?1
             AND (rel_path=?2 OR rel_path LIKE ?3 || '/%' ESCAPE '\\') ORDER BY length(rel_path),rel_path",
        )?;
        let rows = statement.query_map(rusqlite::params![library_id, root, escaped], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
}

fn ensure_folder_row(db: &Db, lib: &Library, rel: &str, marker: i64) -> Result<i64> {
    let path = vol_rel(lib, rel);
    let name = rel.rsplit('/').next().unwrap_or(&lib.name);
    db.write(|c| {
        c.execute(
            "INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET library_id=excluded.library_id",
            rusqlite::params![lib.volume_uuid, lib.id, path, name, lib.area, marker],
        )
    })?;
    db.read(|c| {
        c.query_row(
            "SELECT id FROM folders WHERE volume_uuid=?1 AND rel_path=?2",
            rusqlite::params![lib.volume_uuid, path],
            |r| r.get(0),
        )
    })
}

fn refresh_counts(db: &Db, library_id: i64) -> Result<()> {
    db.write(|c| c.execute(
        "UPDATE folders SET file_count=(SELECT COUNT(*) FROM files WHERE files.folder_id=folders.id AND files.trashed_at IS NULL) WHERE library_id=?1",
        [library_id],
    ))?;
    Ok(())
}

fn move_db_rows(
    db: &Db,
    source_lib: &Library,
    source_rel: &str,
    destination_lib: &Library,
    destination_rel: &str,
) -> Result<()> {
    let rows = rows_in_subtree(db, source_lib.id, source_rel)?;
    let source_vol = vol_rel(source_lib, source_rel);
    let destination_vol = vol_rel(destination_lib, destination_rel);
    db.transaction(|tx| {
        for (id, old) in &rows {
            if source_lib.id != destination_lib.id {
                tx.execute(
                    "DELETE FROM thumbs WHERE file_id IN (SELECT id FROM files WHERE folder_id=?1)",
                    [id],
                )?;
            }
            let suffix = old.strip_prefix(&source_vol).unwrap_or_default();
            let new_path = format!("{destination_vol}{suffix}");
            let name = new_path.rsplit('/').next().unwrap_or(&new_path);
            tx.execute(
                "UPDATE folders SET volume_uuid=?2,library_id=?3,rel_path=?4,name=?5,area=?6,
                 parent_id=CASE WHEN id=?1 THEN NULL ELSE parent_id END WHERE id=?1",
                rusqlite::params![
                    id,
                    destination_lib.volume_uuid,
                    destination_lib.id,
                    new_path,
                    name,
                    destination_lib.area
                ],
            )?;
        }
        Ok(())
    })?;
    if rows.is_empty() {
        ensure_folder_row(db, destination_lib, destination_rel, -1)?;
    }
    if let Err(error) = refresh_counts(db, source_lib.id) {
        log::warn!("폴더 이동 뒤 원본 장수 갱신 보류: {error}");
    }
    if source_lib.id != destination_lib.id {
        if let Err(error) = refresh_counts(db, destination_lib.id) {
            log::warn!("폴더 이동 뒤 목적지 장수 갱신 보류: {error}");
        }
    }
    Ok(())
}

fn copy_db_rows(
    db: &Db,
    source_lib: &Library,
    source_rel: &str,
    destination_lib: &Library,
    destination_rel: &str,
    hashes: &HashMap<String, String>,
) -> Result<()> {
    let rows = rows_in_subtree(db, source_lib.id, source_rel)?;
    let source_vol = vol_rel(source_lib, source_rel);
    let mut mapped = HashMap::new();
    let root_id = ensure_folder_row(db, destination_lib, destination_rel, -1)?;
    for (old_id, old_path) in rows {
        let suffix = old_path
            .strip_prefix(&source_vol)
            .unwrap_or_default()
            .trim_start_matches('/');
        let new_rel = if suffix.is_empty() {
            destination_rel.to_string()
        } else {
            join_rel(destination_rel, suffix)
        };
        let new_id = ensure_folder_row(db, destination_lib, &new_rel, -1)?;
        mapped.insert(old_id, (new_id, suffix.to_string()));
    }
    if mapped.is_empty() {
        mapped.insert(-1, (root_id, String::new()));
    }
    for (old_folder, (new_folder, suffix)) in &mapped {
        if *old_folder == -1 {
            continue;
        }
        let files: Vec<(i64, String)> = db.read(|c| {
            let mut statement = c.prepare(
                "SELECT id,name FROM files WHERE folder_id=?1 AND trashed_at IS NULL ORDER BY id",
            )?;
            let rows = statement.query_map([old_folder], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        for (file_id, name) in files {
            let rel = if suffix.is_empty() {
                name.clone()
            } else {
                join_rel(suffix, &name)
            };
            let hash = hashes
                .get(&rel)
                .ok_or_else(|| bad(format!("복사 manifest에 파일이 없습니다: {rel}")))?;
            crate::ops::transfer::clone_row(db, file_id, *new_folder, &name, hash)?;
        }
    }
    if let Err(error) = refresh_counts(db, destination_lib.id) {
        log::warn!("폴더 복사 뒤 장수 갱신 보류: {error}");
    }
    Ok(())
}

fn delete_copied_db(db: &Db, lib: &Library, rel: &str) -> Result<()> {
    let rows = rows_in_subtree(db, lib.id, rel)?;
    db.transaction(|tx| {
        for (id, _) in rows.iter().rev() {
            tx.execute("DELETE FROM folders WHERE id=?1", [id])?;
        }
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
fn record_folder(
    db: &Db,
    batch: i64,
    op: &str,
    source_lib: i64,
    source: &str,
    destination_lib: Option<i64>,
    destination: Option<&str>,
    info: &Manifest,
    cross_volume: bool,
) -> Result<()> {
    db.write(|c| c.execute(
        "INSERT INTO folder_journal(batch_id,op,source_library_id,source_path,destination_library_id,destination_path,file_count,dir_count,bytes,manifest_sha256,cross_volume,stat_sha256)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        rusqlite::params![batch,op,source_lib,source,destination_lib,destination,info.files as i64,info.directories as i64,info.bytes as i64,info.sha256,cross_volume as i32,info.stat_sha256],
    ))?;
    Ok(())
}

fn discard_batch(db: &Db, batch: i64) {
    let _ = db.write(|c| c.execute("DELETE FROM batches WHERE id=?1", [batch]));
}

pub fn execute(db: &Db, request: &Request, label: &str) -> Result<FolderOutcome> {
    // 실행 시점의 계획과 manifest를 한 번에 만든다. 프론트 미리보기 값은 오래됐을
    // 수 있어 신뢰하지 않되, 같은 실행 안에서 원본 트리를 서너 번 다시 읽지는 않는다.
    let OperationPlan {
        preview,
        info,
        source_lib,
        destination_lib,
        source_rel,
        source,
        destination,
    } = operation_plan(db, request, PlanDetail::Verified)?;
    if preview.action == "skip" {
        return Ok(FolderOutcome {
            batch_id: 0,
            completed: 0,
            failed: 0,
            files: preview.files,
            directories: preview.directories,
            bytes: preview.bytes,
            first_error: Some("같은 이름이 있어 실행하지 않았습니다".into()),
            manifest_sha256: None,
        });
    }
    let same_existing_folder = request.action == Action::Rename
        && destination.exists()
        && matches!(
            (source.canonicalize(), destination.canonicalize()),
            (Ok(source), Ok(destination)) if source == destination
        );
    if destination.exists() && request.action != Action::Trash && !same_existing_folder {
        return Err(bad("실행 직전 목적지에 같은 이름이 생겼습니다"));
    }
    let kind = match request.action {
        Action::Create => "folder_create",
        Action::Rename => "folder_rename",
        Action::Move => "folder_move",
        Action::Copy => "folder_copy",
        Action::Trash => "folder_trash",
    };
    let batch = super::open_batch(db, kind, label)?;
    // 작업은 끝났지만 사용자가 알아야 할 뒷정리 실패 — 결과의 first_error 로 보여 준다
    let mut warning: Option<String> = None;
    let operation = (|| -> Result<Manifest> {
        match request.action {
            Action::Create => {
                record_folder(
                    db,
                    batch,
                    "create",
                    source_lib.id,
                    &source_rel,
                    Some(destination_lib.id),
                    Some(&preview.destination),
                    &info,
                    false,
                )?;
                std::fs::create_dir(&destination)?;
                if let Err(error) =
                    ensure_folder_row(db, &destination_lib, &preview.destination, -1)
                {
                    let _ = std::fs::remove_dir(&destination);
                    return Err(error);
                }
                Ok(info.clone())
            }
            Action::Copy => {
                record_folder(
                    db,
                    batch,
                    "copy",
                    source_lib.id,
                    &source_rel,
                    Some(destination_lib.id),
                    Some(&preview.destination),
                    &info,
                    preview.cross_volume,
                )?;
                copy_tree_verified(&source, &destination, batch, None, &info)?;
                if let Err(error) = copy_db_rows(
                    db,
                    &source_lib,
                    &source_rel,
                    &destination_lib,
                    &preview.destination,
                    &info.file_hashes,
                ) {
                    let _ = delete_copied_db(db, &destination_lib, &preview.destination);
                    let _ = remove_tree(&destination);
                    return Err(error);
                }
                Ok(info.clone())
            }
            Action::Move | Action::Rename => {
                let cross = source_lib.volume_uuid != destination_lib.volume_uuid;
                record_folder(
                    db,
                    batch,
                    if request.action == Action::Rename {
                        "rename"
                    } else {
                        "move"
                    },
                    source_lib.id,
                    &source_rel,
                    Some(destination_lib.id),
                    Some(&preview.destination),
                    &info,
                    cross,
                )?;
                let backup = stage_move(&source, &destination, batch, cross, &info)?;
                if let Err(error) = move_db_rows(
                    db,
                    &source_lib,
                    &source_rel,
                    &destination_lib,
                    &preview.destination,
                ) {
                    if let Some(backup) = &backup {
                        let _ = std::fs::rename(backup, &source);
                        let _ = remove_tree(&destination);
                    } else {
                        let _ = std::fs::rename(&destination, &source);
                    }
                    return Err(error);
                }
                if let Some(backup) = backup {
                    if let Err(error) = remove_tree(&backup) {
                        // 원본 전체가 든 숨은 폴더가 출발 디스크에 남는다 — 조용히 넘기면
                        // 사용자는 공간이 왜 안 비는지 모른다
                        log::warn!(
                            "볼륨 간 이동 뒤 백업 정리 실패 {}: {error}",
                            backup.display()
                        );
                        warning = Some(format!(
                            "옮기기는 끝났지만 원본 쪽 임시 백업을 지우지 못했습니다: {} ({error}). Finder 에서 지워 주세요",
                            backup.display()
                        ));
                    }
                }
                Ok(info.clone())
            }
            Action::Trash => {
                record_folder(
                    db,
                    batch,
                    "trash",
                    source_lib.id,
                    &source_rel,
                    Some(source_lib.id),
                    Some(&preview.destination),
                    &info,
                    false,
                )?;
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&source, &destination)?;
                let rows = rows_in_subtree(db, source_lib.id, &source_rel)?;
                let source_vol = vol_rel(&source_lib, &source_rel);
                let trash_prefix = preview.destination.clone();
                let changed=db.transaction(|tx|{
                    for (folder_id,folder_path) in &rows {
                        let suffix=folder_path.strip_prefix(&source_vol).unwrap_or_default().trim_start_matches('/');
                        let files:Vec<(i64,String)>= { let mut st=tx.prepare("SELECT id,name FROM files WHERE folder_id=?1 AND trashed_at IS NULL")?; let it=st.query_map([folder_id],|r|Ok((r.get(0)?,r.get(1)?)))?; it.collect::<rusqlite::Result<Vec<_>>>()? };
                        for (file_id,name) in files {
                            let sub=if suffix.is_empty(){name}else{join_rel(suffix,&name)};
                            let trash_path=join_rel(&trash_prefix,&sub);
                            tx.execute("UPDATE files SET trashed_at=strftime('%s','now'),trash_path=?2,trash_batch=?3 WHERE id=?1",rusqlite::params![file_id,trash_path,batch])?;
                        }
                        tx.execute("UPDATE folders SET scanned_at=CASE WHEN scanned_at=-1 THEN -2 ELSE scanned_at END,file_count=0 WHERE id=?1",[folder_id])?;
                    }
                    Ok(())
                });
                if let Err(error) = changed {
                    let _ = std::fs::rename(&destination, &source);
                    return Err(error);
                }
                Ok(info.clone())
            }
        }
    })();
    match operation {
        Ok(info) => {
            super::close_batch(db, batch, 1)?;
            Ok(FolderOutcome {
                batch_id: batch,
                completed: 1,
                failed: 0,
                files: info.files,
                directories: info.directories,
                bytes: info.bytes,
                first_error: warning,
                manifest_sha256: Some(info.sha256).filter(|digest| !digest.is_empty()),
            })
        }
        Err(error) => {
            discard_batch(db, batch);
            Ok(FolderOutcome {
                batch_id: 0,
                completed: 0,
                failed: 1,
                files: preview.files,
                directories: preview.directories,
                bytes: preview.bytes,
                first_error: Some(error.to_string()),
                manifest_sha256: None,
            })
        }
    }
}

#[derive(Debug)]
struct JournalRow {
    op: String,
    source_library_id: i64,
    source_path: String,
    destination_library_id: Option<i64>,
    destination_path: Option<String>,
    /// 내용 다이제스트. 같은 볼륨 작업(0.9.2+)은 빈 문자열로 남기고 `stat_sha256` 로 대조한다.
    manifest_sha256: String,
    /// 이름·크기·mtime 다이제스트. 0.9.1 이전 저널은 NULL.
    stat_sha256: Option<String>,
    files: usize,
    dirs: usize,
    bytes: u64,
}

fn journal(db: &Db, batch: i64) -> Result<JournalRow> {
    db.read(|c|c.query_row("SELECT op,source_library_id,source_path,destination_library_id,destination_path,manifest_sha256,file_count,dir_count,bytes,stat_sha256 FROM folder_journal WHERE batch_id=?1",[batch],|r|Ok(JournalRow{op:r.get(0)?,source_library_id:r.get(1)?,source_path:r.get(2)?,destination_library_id:r.get(3)?,destination_path:r.get(4)?,manifest_sha256:r.get(5)?,files:r.get::<_,i64>(6)? as usize,dirs:r.get::<_,i64>(7)? as usize,bytes:r.get::<_,i64>(8)? as u64,stat_sha256:r.get(9)?})))
}

pub fn undo(db: &Db, batch: i64) -> Result<Outcome> {
    let row = journal(db, batch)?;
    let source_lib = library(db, row.source_library_id)?;
    let source_root = online_root(&source_lib)?;
    let destination_lib = library(
        db,
        row.destination_library_id.unwrap_or(row.source_library_id),
    )?;
    let destination_rel = row
        .destination_path
        .clone()
        .ok_or_else(|| bad("되돌릴 목적지 기록이 없습니다"))?;
    let destination = online_root(&destination_lib)?.join(&destination_rel);
    let mut out = Outcome {
        batch_id: batch,
        ..Default::default()
    };
    let fail = |out: &mut Outcome, message: String| {
        out.failed = 1;
        out.first_error = Some(message);
    };
    if row.op == "create" {
        if std::fs::read_dir(&destination)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(true)
        {
            fail(
                &mut out,
                "폴더 안에 새 항목이 있어 생성 작업을 되돌리지 않았습니다".into(),
            );
            return Ok(out);
        }
        let staged = free_path(temp_sibling(&destination, batch));
        std::fs::rename(&destination, &staged)?;
        if let Err(error) = delete_copied_db(db, &destination_lib, &destination_rel) {
            let _ = std::fs::rename(&staged, &destination);
            return Err(error);
        }
        if let Err(error) = remove_tree(&staged) {
            log::warn!(
                "폴더 생성 undo 임시 갈래 정리 보류 {}: {error}",
                staged.display()
            );
        }
    } else {
        if !destination.is_dir() {
            fail(&mut out, "되돌릴 폴더가 디스크에 없습니다".into());
            return Ok(out);
        }
        // 대조 기준: 내용 해시가 기록돼 있으면(복사·볼륨 간 이동·0.9.1 저널) 그것으로,
        // 없으면 같은 볼륨 작업이 남긴 이름·크기·mtime 다이제스트로 본다.
        let now = if row.manifest_sha256.is_empty() {
            let summary = tree_summary(&destination)?;
            if row.stat_sha256.as_deref() != Some(summary.stat_sha256.as_str()) {
                fail(
                    &mut out,
                    "작업 뒤 폴더 내용이 바뀌어 안전하게 되돌릴 수 없습니다".into(),
                );
                return Ok(out);
            }
            summary
        } else {
            let full = manifest(&destination)?;
            if full.sha256 != row.manifest_sha256 {
                fail(
                    &mut out,
                    "작업 뒤 폴더 내용이 바뀌어 안전하게 되돌릴 수 없습니다".into(),
                );
                return Ok(out);
            }
            full
        };
        match row.op.as_str() {
            "copy" => {
                let staged = free_path(temp_sibling(&destination, batch));
                std::fs::rename(&destination, &staged)?;
                if let Err(error) = delete_copied_db(db, &destination_lib, &destination_rel) {
                    let _ = std::fs::rename(&staged, &destination);
                    return Err(error);
                }
                if let Err(error) = remove_tree(&staged) {
                    log::warn!(
                        "폴더 복사 undo 임시 갈래 정리 보류 {}: {error}",
                        staged.display()
                    );
                }
            }
            "move" | "rename" => {
                let wanted = source_root.join(&row.source_path);
                let target = if wanted.exists() {
                    free_path(wanted)
                } else {
                    wanted
                };
                let restored_rel = target
                    .strip_prefix(&source_root)
                    .map_err(|_| bad("복원 경로 오류"))?
                    .to_string_lossy()
                    .into_owned();
                if restored_rel != row.source_path {
                    // 원래 자리가 차 있어 옆 이름으로 돌아왔다 — «되돌렸다»만 보이면 사용자는 모른다
                    out.first_error = Some(format!(
                        "원래 자리에 같은 이름이 있어 «{restored_rel}» 로 되돌렸습니다"
                    ));
                }
                if source_lib.volume_uuid == destination_lib.volume_uuid {
                    if let Some(p) = target.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    std::fs::rename(&destination, &target)?;
                    if let Err(error) = move_db_rows(
                        db,
                        &destination_lib,
                        &destination_rel,
                        &source_lib,
                        &restored_rel,
                    ) {
                        let _ = std::fs::rename(&target, &destination);
                        return Err(error);
                    }
                } else {
                    // 볼륨을 건너는 복원은 사본을 내용 해시로 검증한다
                    let verified = if now.sha256.is_empty() {
                        manifest(&destination)?
                    } else {
                        now.clone()
                    };
                    copy_tree_verified(&destination, &target, batch, None, &verified)?;
                    let staged = free_path(temp_sibling(&destination, batch));
                    if let Err(error) = std::fs::rename(&destination, &staged) {
                        let _ = remove_tree(&target);
                        return Err(error.into());
                    }
                    if let Err(error) = move_db_rows(
                        db,
                        &destination_lib,
                        &destination_rel,
                        &source_lib,
                        &restored_rel,
                    ) {
                        let _ = remove_tree(&target);
                        let _ = std::fs::rename(&staged, &destination);
                        return Err(error);
                    }
                    if let Err(error) = remove_tree(&staged) {
                        log::warn!(
                            "폴더 이동 undo 임시 갈래 정리 보류 {}: {error}",
                            staged.display()
                        );
                    }
                }
            }
            "trash" => {
                let wanted = source_root.join(&row.source_path);
                let target = if wanted.exists() {
                    free_path(wanted)
                } else {
                    wanted
                };
                if let Some(p) = target.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::rename(&destination, &target)?;
                let restored_rel = target
                    .strip_prefix(&source_root)
                    .map_err(|_| bad("복원 경로 오류"))?
                    .to_string_lossy()
                    .into_owned();
                if restored_rel != row.source_path {
                    // 원래 자리가 차 있어 옆 이름으로 돌아왔다 — «되돌렸다»만 보이면 사용자는 모른다
                    out.first_error = Some(format!(
                        "원래 자리에 같은 이름이 있어 «{restored_rel}» 로 되돌렸습니다"
                    ));
                }
                let moved_in_db = restored_rel != row.source_path;
                if moved_in_db {
                    if let Err(error) = move_db_rows(
                        db,
                        &source_lib,
                        &row.source_path,
                        &source_lib,
                        &restored_rel,
                    ) {
                        let _ = std::fs::rename(&target, &destination);
                        return Err(error);
                    }
                }
                let rows = rows_in_subtree(db, source_lib.id, &restored_rel)?;
                let changed = db.transaction(|tx|{for (id,_) in &rows{tx.execute("UPDATE files SET trashed_at=NULL,trash_path=NULL,trash_batch=NULL WHERE folder_id=?1 AND trash_batch=?2",rusqlite::params![id,batch])?;tx.execute("UPDATE folders SET scanned_at=CASE WHEN scanned_at=-2 THEN -1 ELSE scanned_at END WHERE id=?1",[id])?;}Ok(())});
                if let Err(error) = changed {
                    if moved_in_db {
                        let _ = move_db_rows(
                            db,
                            &source_lib,
                            &restored_rel,
                            &source_lib,
                            &row.source_path,
                        );
                    }
                    let _ = std::fs::rename(&target, &destination);
                    return Err(error);
                }
                if let Err(error) = refresh_counts(db, source_lib.id) {
                    log::warn!("폴더 휴지통 undo 장수 갱신 보류: {error}");
                }
            }
            _ => return Err(bad("알 수 없는 폴더 저널입니다")),
        }
    }
    crate::ops::undo::mark_undone(db, batch)?;
    out.moved = 1;
    out.bytes = row.bytes as i64;
    out.folders_removed = row.dirs;
    let _ = row.files;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Db, Library, Library) {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("A");
        let b = temp.path().join("B");
        std::fs::create_dir_all(a.join("부모/자식")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        for i in 0..20 {
            std::fs::write(a.join(format!("부모/자식/{i}.jpg")), format!("photo-{i}")).unwrap();
        }
        std::fs::create_dir_all(a.join("부모/빈폴더")).unwrap();
        let db = Db::open(temp.path().join("t.db")).unwrap();
        let la = crate::db::libraries::add(&db, &a, 1).unwrap();
        let lb = crate::db::libraries::add(&db, &b, 2).unwrap();
        crate::scan::scan_folder(&db, la.id, &a, 1, |_| {}).unwrap();
        (temp, db, la, lb)
    }

    fn req(action: Action, source: i64, path: &str) -> Request {
        Request {
            action,
            source_library_id: source,
            source_dir: path.into(),
            destination_library_id: None,
            destination_parent: None,
            name: None,
            conflict_policy: ConflictPolicy::Skip,
        }
    }

    #[test]
    fn create_rename_move_copy_trash_and_undo_keep_manifest() {
        let (_temp, db, la, lb) = setup();
        let mut create = req(Action::Create, la.id, "");
        create.name = Some("새 폴더".into());
        let made = execute(&db, &create, "생성").unwrap();
        assert_eq!(made.completed, 1);
        assert!(la.dir.as_ref().unwrap().join("새 폴더").is_dir());
        assert_eq!(undo(&db, made.batch_id).unwrap().moved, 1);
        assert!(!la.dir.as_ref().unwrap().join("새 폴더").exists());

        let mut rename = req(Action::Rename, la.id, "부모/자식");
        rename.destination_parent = Some("부모".into());
        rename.name = Some("이름변경".into());
        let renamed = execute(&db, &rename, "이름 변경").unwrap();
        assert_eq!(renamed.completed, 1);
        assert!(la
            .dir
            .as_ref()
            .unwrap()
            .join("부모/이름변경/3.jpg")
            .is_file());
        assert_eq!(undo(&db, renamed.batch_id).unwrap().moved, 1);
        assert!(la.dir.as_ref().unwrap().join("부모/자식/3.jpg").is_file());

        let mut copy = req(Action::Copy, la.id, "부모/자식");
        copy.destination_library_id = Some(lb.id);
        copy.destination_parent = Some("".into());
        let copied = execute(&db, &copy, "복사").unwrap();
        assert_eq!(copied.completed, 1);
        assert!(lb.dir.as_ref().unwrap().join("자식/19.jpg").is_file());
        assert_eq!(undo(&db, copied.batch_id).unwrap().moved, 1);
        assert!(!lb.dir.as_ref().unwrap().join("자식").exists());
        assert!(la.dir.as_ref().unwrap().join("부모/자식/19.jpg").is_file());

        let trashed = execute(&db, &req(Action::Trash, la.id, "부모/자식"), "폴더 휴지통").unwrap();
        assert_eq!(trashed.completed, 1);
        assert!(!la.dir.as_ref().unwrap().join("부모/자식").exists());
        assert_eq!(undo(&db, trashed.batch_id).unwrap().moved, 1);
        assert!(la.dir.as_ref().unwrap().join("부모/자식/0.jpg").is_file());

        let empty = execute(
            &db,
            &req(Action::Trash, la.id, "부모/빈폴더"),
            "빈 폴더 휴지통",
        )
        .unwrap();
        assert_eq!((empty.completed, empty.files), (1, 0));
        assert!(!la.dir.as_ref().unwrap().join("부모/빈폴더").exists());
        assert_eq!(undo(&db, empty.batch_id).unwrap().moved, 1);
        assert!(la.dir.as_ref().unwrap().join("부모/빈폴더").is_dir());
    }

    /// 같은 볼륨 이름변경은 내용을 읽지 않고 이름·크기·mtime 만 남긴다. undo 도 그 값으로 대조한다
    #[test]
    fn same_volume_rename_uses_a_stat_manifest_and_undo_refuses_a_changed_file() {
        let (_temp, db, la, _lb) = setup();
        let mut rename = req(Action::Rename, la.id, "부모/자식");
        rename.destination_parent = Some("부모".into());
        rename.name = Some("이름변경".into());
        let renamed = execute(&db, &rename, "이름 변경").unwrap();
        assert_eq!(renamed.completed, 1);
        assert!(
            renamed.manifest_sha256.is_none(),
            "같은 볼륨 이름변경은 내용 해시를 계산하지 않는다"
        );
        let (content, stat): (String, Option<String>) = db
            .read(|c| {
                c.query_row(
                    "SELECT manifest_sha256, stat_sha256 FROM folder_journal WHERE batch_id=?1",
                    [renamed.batch_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert!(content.is_empty());
        assert!(stat.is_some_and(|digest| !digest.is_empty()));

        let root = la.dir.clone().unwrap();
        std::fs::write(
            root.join("부모/이름변경/3.jpg"),
            b"edited after the rename, and longer",
        )
        .unwrap();
        let out = undo(&db, renamed.batch_id).unwrap();
        assert_eq!((out.moved, out.failed), (0, 1), "{:?}", out.first_error);
        assert!(out
            .first_error
            .as_deref()
            .unwrap_or_default()
            .contains("바뀌어"));
        assert!(
            root.join("부모/이름변경/3.jpg").is_file(),
            "바뀐 폴더는 그 자리에 둔다"
        );
    }

    /// 0.9.1 저널(내용 해시만, stat 없음)은 내용 해시로 대조해 그대로 되돌린다
    #[test]
    fn a_journal_without_stat_is_verified_by_content_hash() {
        let (_temp, db, la, _lb) = setup();
        let mut rename = req(Action::Rename, la.id, "부모/자식");
        rename.destination_parent = Some("부모".into());
        rename.name = Some("이름변경".into());
        let renamed = execute(&db, &rename, "이름 변경").unwrap();
        let root = la.dir.clone().unwrap();
        let full = manifest(&root.join("부모/이름변경")).unwrap();
        assert!(!full.sha256.is_empty());
        db.write(|c| {
            c.execute(
                "UPDATE folder_journal SET manifest_sha256=?2, stat_sha256=NULL WHERE batch_id=?1",
                rusqlite::params![renamed.batch_id, full.sha256],
            )
        })
        .unwrap();
        assert_eq!(undo(&db, renamed.batch_id).unwrap().moved, 1);
        assert!(root.join("부모/자식/3.jpg").is_file());
    }

    #[test]
    fn cycle_root_offline_and_collision_are_blocked() {
        let (_temp, db, la, _) = setup();
        let mut cycle = req(Action::Move, la.id, "부모");
        cycle.destination_library_id = Some(la.id);
        cycle.destination_parent = Some("부모/자식".into());
        assert!(preview(&db, &cycle)
            .unwrap_err()
            .to_string()
            .contains("자기"));
        assert!(preview(&db, &req(Action::Trash, la.id, "")).is_err());
        let mut collide = req(Action::Create, la.id, "부모");
        collide.name = Some("자식".into());
        assert_eq!(preview(&db, &collide).unwrap().conflict, "name_exists");
        let missing = req(Action::Trash, 9999, "x");
        assert!(preview(&db, &missing).is_err());

        let offline_dir = la.dir.as_ref().unwrap().to_path_buf();
        let hidden = offline_dir.with_file_name("A-offline");
        std::fs::rename(&offline_dir, &hidden).unwrap();
        assert!(preview(&db, &req(Action::Trash, la.id, "부모"))
            .unwrap_err()
            .to_string()
            .contains("연결"));
    }

    #[test]
    fn hidden_names_are_blocked_and_rename_cannot_change_libraries() {
        let (_temp, db, la, lb) = setup();
        let mut hidden = req(Action::Create, la.id, "");
        hidden.name = Some(".acut".into());
        assert!(preview(&db, &hidden).is_err());

        let mut rename = req(Action::Rename, la.id, "부모/자식");
        rename.destination_library_id = Some(lb.id);
        rename.destination_parent = Some(String::new());
        rename.name = Some("안전한이름".into());
        let plan = preview(&db, &rename).unwrap();
        assert_eq!(plan.destination, "부모/안전한이름");
        let out = execute(&db, &rename, "이름 변경").unwrap();
        assert_eq!((out.completed, out.failed), (1, 0));
        assert!(la
            .dir
            .as_ref()
            .unwrap()
            .join("부모/안전한이름/0.jpg")
            .is_file());
        assert!(!lb.dir.as_ref().unwrap().join("안전한이름").exists());
    }

    #[test]
    fn partial_copy_failure_removes_temp_and_keeps_source() {
        let (_temp, _db, la, _) = setup();
        let source = la.dir.as_ref().unwrap().join("부모/자식");
        let target = la.dir.as_ref().unwrap().join("부분");
        let before = manifest(&source).unwrap();
        assert!(copy_tree_verified(&source, &target, 44, Some(3), &before).is_err());
        assert!(!target.exists());
        assert!(source.join("19.jpg").is_file());
        assert!(!temp_sibling(&target, 44).exists());
    }

    #[test]
    fn staged_cross_volume_move_keeps_a_rollback_copy_until_db_commit() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("원본");
        let destination = temp.path().join("다른볼륨-역할");
        std::fs::create_dir_all(source.join("빈폴더")).unwrap();
        std::fs::write(source.join("photo.jpg"), b"pixels").unwrap();
        std::fs::write(source.join("photo.xmp"), b"sidecar").unwrap();
        let before = manifest(&source).unwrap();

        let backup = stage_move(&source, &destination, 77, true, &before).unwrap();
        let backup = backup.expect("볼륨 간 이동은 원본 쪽 rollback 백업을 둔다");
        assert_eq!(before.sha256, manifest(&destination).unwrap().sha256);
        assert!(!source.exists());
        assert!(
            backup.join("photo.xmp").is_file(),
            "sidecar도 백업에 남는다"
        );

        std::fs::rename(&backup, &source).unwrap();
        std::fs::remove_dir_all(&destination).unwrap();
        assert_eq!(before.sha256, manifest(&source).unwrap().sha256);
    }

    #[test]
    fn cross_library_folder_move_invalidates_thumbnail_rows() {
        let (_temp, db, la, lb) = setup();
        let file_id = db
            .read(|c| {
                c.query_row(
                    "SELECT fi.id FROM files fi JOIN folders fo ON fo.id=fi.folder_id
                     WHERE fo.library_id=?1 LIMIT 1",
                    [la.id],
                    |r| r.get::<_, i64>(0),
                )
            })
            .unwrap();
        db.write(|c| {
            c.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state) VALUES(?1,'aa/old.jpg',1,1,1)",
                [file_id],
            )
        })
        .unwrap();

        let mut move_request = req(Action::Move, la.id, "부모/자식");
        move_request.destination_library_id = Some(lb.id);
        move_request.destination_parent = Some(String::new());
        let moved = execute(&db, &move_request, "라이브러리 간 폴더 이동").unwrap();
        assert_eq!((moved.completed, moved.failed), (1, 0));
        assert_eq!(
            db.read(|c| c.query_row(
                "SELECT COUNT(*) FROM thumbs WHERE file_id=?1",
                [file_id],
                |r| r.get::<_, i64>(0),
            ))
            .unwrap(),
            0
        );
    }

    #[test]
    fn nested_empty_and_large_folder_manifest_is_stable() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("큰 폴더");
        std::fs::create_dir_all(source.join("빈/더빈")).unwrap();
        for i in 0..1000 {
            std::fs::write(source.join(format!("{i}.txt")), b"x").unwrap();
        }
        std::fs::write(source.join("._0.txt"), b"AppleDouble must not travel").unwrap();
        let decomposed = "\u{1100}\u{1161}.txt";
        std::fs::write(source.join(decomposed), b"nfc path").unwrap();
        let target = temp.path().join("사본");
        let before = manifest(&source).unwrap();
        copy_tree_verified(&source, &target, 55, None, &before).unwrap();
        let after = manifest(&target).unwrap();
        assert_eq!(before.sha256, after.sha256);
        assert_eq!(before.files, 1001);
        assert!(!target.join("._0.txt").exists());
        assert!(
            before.file_hashes.contains_key("가.txt"),
            "manifest 경로는 유니코드 NFC여야 한다"
        );
        assert!(target.join("빈/더빈").is_dir());
    }
}
