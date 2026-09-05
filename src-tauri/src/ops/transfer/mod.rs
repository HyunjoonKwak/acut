//! 임의 목적지 이동·복사와 공용 발행.
//!
//! 실행 전에 같은 이름과 이미 발행한 hash를 모두 보여 주고, 실제 작업은 기존
//! 볼륨 경로·sidecar·batch journal 규칙을 따른다.

use crate::db::conn::{Db, DbError, IoContext, Result};
use crate::ops::trash::{copy_mtime, free_path, move_with_sidecars, sidecars, Outcome};
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Move,
    Copy,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    Skip,
    Rename,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub ids: Vec<i64>,
    pub destination_library_id: i64,
    pub destination_dir: String,
    pub mode: Mode,
    #[serde(default = "default_policy")]
    pub conflict_policy: ConflictPolicy,
    #[serde(default)]
    pub publish: bool,
}

fn default_policy() -> ConflictPolicy {
    ConflictPolicy::Skip
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PreviewItem {
    pub id: i64,
    pub source: String,
    pub destination: String,
    pub planned_name: String,
    pub conflict: String,
    pub action: String,
    pub source_sha256: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Preview {
    pub mode: String,
    pub publish: bool,
    pub source_area: Option<i32>,
    pub destination_area: i32,
    pub drive_sync_warning: bool,
    pub items: Vec<PreviewItem>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TransferOutcome {
    pub batch_id: i64,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub already_published: usize,
    pub bytes: i64,
    pub first_error: Option<String>,
    pub failed_ids: Vec<i64>,
}

#[derive(Debug, Clone)]
struct Item {
    id: i64,
    library_id: i64,
    source_area: i32,
    volume_uuid: String,
    vol_rel: String,
    name: String,
    size: i64,
}

fn validate_rel(rel: &str) -> Result<String> {
    let clean = crate::scan::nfc(rel.trim().trim_matches('/'));
    if clean.is_empty() {
        return Ok(clean);
    }
    let p = Path::new(&clean);
    if p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(DbError::Invalid(
            "목적지는 라이브러리 안의 상대 폴더여야 합니다".into(),
        ));
    }
    if clean.split('/').any(|part| {
        part.is_empty()
            || part == "."
            || part.starts_with('.')
            || part.contains(':')
            || part.chars().any(|c| (c as u32) < 0x20)
    }) {
        return Err(DbError::Invalid(
            "목적지 폴더 이름에 쓸 수 없는 문자가 있습니다".into(),
        ));
    }
    Ok(clean)
}

fn destination_inside(root: &Path, rel: &str) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .map_err(|_| DbError::Invalid("목적지 라이브러리 경로를 확인할 수 없습니다".into()))?;
    if !root.is_dir() {
        return Err(DbError::Invalid(
            "목적지 라이브러리가 폴더가 아닙니다".into(),
        ));
    }
    let destination = if rel.is_empty() {
        root.clone()
    } else {
        root.join(rel)
    };
    let mut ancestor = destination.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or_else(|| {
            DbError::Invalid("목적지의 기존 부모 폴더를 확인할 수 없습니다".into())
        })?;
    }
    let real = ancestor
        .canonicalize()
        .io_context("목적지의 기존 부모 경로를 확인하다가 실패했습니다")?;
    if !real.starts_with(&root) {
        return Err(DbError::Invalid(
            "심볼릭 링크를 통해 라이브러리 밖으로 쓸 수 없습니다".into(),
        ));
    }
    if destination.exists() {
        let real_destination = destination
            .canonicalize()
            .io_context("목적지 경로를 확인하다가 실패했습니다")?;
        if !real_destination.starts_with(&root) || !real_destination.is_dir() {
            return Err(DbError::Invalid(
                "목적지가 라이브러리 밖이거나 폴더가 아닙니다".into(),
            ));
        }
        return Ok(real_destination);
    }
    Ok(destination)
}

fn load(db: &Db, ids: &[i64]) -> Result<Vec<Item>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    // SQLite 빌드별 bind 상한보다 충분히 작게 나눠, 큰 전체 선택도 오류 대신
    // 같은 규칙으로 처리한다.
    for chunk in ids.chunks(5_000) {
        let marks = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let mut rows = db.read(|c| {
            let mut st = c.prepare(&format!(
                "SELECT fi.id,fo.library_id,l.area,fo.volume_uuid,
                        fo.rel_path || CASE WHEN fo.rel_path='' THEN '' ELSE '/' END || fi.name,
                        fi.name,fi.size
                 FROM files fi JOIN folders fo ON fo.id=fi.folder_id JOIN libraries l ON l.id=fo.library_id
                 WHERE fi.trashed_at IS NULL AND fi.id IN ({marks}) ORDER BY fi.id"
            ))?;
            let rows = st.query_map(rusqlite::params_from_iter(chunk.iter()), |r| Ok(Item {
                id:r.get(0)?,library_id:r.get(1)?,source_area:r.get(2)?,
                volume_uuid:r.get(3)?,vol_rel:r.get(4)?,name:r.get(5)?,size:r.get(6)?,
            }))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        out.append(&mut rows);
    }
    out.sort_by_key(|item| item.id);
    out.dedup_by_key(|item| item.id);
    Ok(out)
}

fn mount_path(it: &Item) -> Result<PathBuf> {
    let mount = crate::db::volumes::find_mount(&it.volume_uuid)
        .ok_or_else(|| DbError::Invalid("원본 디스크가 연결되어 있지 않습니다".into()))?;
    let path = mount.join(&it.vol_rel);
    if !path.is_file() {
        return Err(DbError::Invalid(format!(
            "원본 파일이 없습니다: {}",
            it.vol_rel
        )));
    }
    Ok(path)
}

fn hash(path: &Path) -> Result<String> {
    crate::cull::hash::full(path).io_context("전송할 파일의 해시를 읽다가 실패했습니다")
}

fn dest(db: &Db, request: &Request) -> Result<(crate::db::libraries::Library, String, PathBuf)> {
    let rel = validate_rel(&request.destination_dir)?;
    let lib = crate::db::libraries::get(db, request.destination_library_id)?
        .ok_or_else(|| DbError::Invalid("등록되지 않은 목적지 라이브러리입니다".into()))?;
    let dir = lib
        .dir
        .clone()
        .ok_or_else(|| DbError::Invalid("목적지 디스크가 연결되어 있지 않습니다".into()))?;
    let full = destination_inside(&dir, &rel)?;
    Ok((lib, rel, full))
}

fn published_path(db: &Db, source_hash: &str, library_id: i64) -> Result<Option<String>> {
    db.read(|c|c.query_row(
        "SELECT destination_path FROM publication_ledger WHERE source_sha256=?1 AND destination_library_id=?2 ORDER BY id DESC LIMIT 1",
        rusqlite::params![source_hash,library_id],|r|r.get(0)).optional())
}

pub fn preview(db: &Db, request: &Request) -> Result<Preview> {
    let items = load(db, &request.ids)?;
    let (lib, rel, dir) = dest(db, request)?;
    let source_area = items
        .first()
        .map(|i| i.source_area)
        .filter(|a| items.iter().all(|i| i.source_area == *a));
    let publish =
        request.publish || (source_area == Some(1) && lib.area == 2 && request.mode == Mode::Copy);
    let mut out = Vec::with_capacity(items.len());
    let mut reserved = std::collections::HashSet::new();
    for it in items {
        let source_path = match mount_path(&it) {
            Ok(p) => p,
            Err(_) => {
                out.push(PreviewItem {
                    id: it.id,
                    source: it.vol_rel,
                    destination: if rel.is_empty() {
                        it.name.clone()
                    } else {
                        format!("{rel}/{}", it.name)
                    },
                    planned_name: it.name,
                    conflict: "source_missing".into(),
                    action: "run".into(),
                    source_sha256: None,
                });
                continue;
            }
        };
        // 읽을 수 없는 파일 하나가 미리보기 전체를 막으면 안 된다 — 그 항목만 표시하고
        // 실행에서 제 오류로 실패하게 둔다 (source_missing 과 같은 규칙)
        let source_hash = if publish {
            match hash(&source_path) {
                Ok(digest) => Some(digest),
                Err(_) => {
                    out.push(PreviewItem {
                        id: it.id,
                        source: it.vol_rel,
                        destination: if rel.is_empty() {
                            it.name.clone()
                        } else {
                            format!("{rel}/{}", it.name)
                        },
                        planned_name: it.name,
                        conflict: "source_unreadable".into(),
                        action: "run".into(),
                        source_sha256: None,
                    });
                    continue;
                }
            }
        } else {
            None
        };
        let existing_publication = match source_hash.as_deref() {
            Some(h) => published_path(db, h, lib.id)?,
            None => None,
        };
        let wanted = dir.join(&it.name);
        let (mut conflict, mut action, mut planned) = if let Some(path) = existing_publication {
            let valid = lib.dir.as_ref().is_some_and(|dir| {
                let full = dir.join(&path);
                full.is_file()
                    && source_hash
                        .as_deref()
                        .is_some_and(|h| crate::cull::hash::full(&full).ok().as_deref() == Some(h))
            });
            if valid {
                ("already_published", "skip", it.name.clone())
            } else if wanted.exists() {
                conflict_plan(&wanted, request.conflict_policy)
            } else {
                ("stale_ledger", "run", it.name.clone())
            }
        } else if wanted.exists() {
            conflict_plan(&wanted, request.conflict_policy)
        } else {
            ("none", "run", it.name.clone())
        };
        let key = planned.to_lowercase();
        if action != "skip" && reserved.contains(&key) {
            match request.conflict_policy {
                ConflictPolicy::Skip => {
                    conflict = "batch_name_exists";
                    action = "skip";
                }
                ConflictPolicy::Rename => {
                    planned = free_name(&dir, &planned, &reserved);
                    conflict = "batch_name_exists";
                    action = "rename";
                }
            }
        }
        if action != "skip" {
            reserved.insert(planned.to_lowercase());
        }
        out.push(PreviewItem {
            id: it.id,
            source: it.vol_rel,
            destination: if rel.is_empty() {
                planned.clone()
            } else {
                format!("{rel}/{planned}")
            },
            planned_name: planned,
            conflict: conflict.into(),
            action: action.into(),
            source_sha256: source_hash,
        });
    }
    Ok(Preview {
        mode: match request.mode {
            Mode::Move => "move",
            Mode::Copy => "copy",
        }
        .into(),
        publish,
        source_area,
        destination_area: lib.area,
        drive_sync_warning: source_area.is_some_and(|a| [1, 2].contains(&a))
            || [1, 2].contains(&lib.area),
        items: out,
    })
}

fn free_name(
    dir: &Path,
    wanted_name: &str,
    reserved: &std::collections::HashSet<String>,
) -> String {
    let wanted = Path::new(wanted_name);
    let stem = wanted
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| wanted_name.to_string());
    let extension = wanted
        .extension()
        .map(|value| value.to_string_lossy().into_owned());
    for number in 2..10_000 {
        let name = match &extension {
            Some(extension) => format!("{stem} ({number}).{extension}"),
            None => format!("{stem} ({number})"),
        };
        if !reserved.contains(&name.to_lowercase()) && !dir.join(&name).exists() {
            return name;
        }
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    match extension {
        Some(extension) => format!("{stem} ({stamp}).{extension}"),
        None => format!("{stem} ({stamp})"),
    }
}

fn conflict_plan(wanted: &Path, policy: ConflictPolicy) -> (&'static str, &'static str, String) {
    match policy {
        ConflictPolicy::Skip => (
            "name_exists",
            "skip",
            wanted
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        ),
        ConflictPolicy::Rename => {
            let p = free_path(wanted.to_path_buf());
            (
                "name_exists",
                "rename",
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }
}

mod execute;
mod undo;

pub(crate) use execute::clone_row;
pub use execute::execute;
pub use undo::undo_copy;

#[cfg(test)]
use execute::ensure_folder;

#[cfg(test)]
mod tests;
