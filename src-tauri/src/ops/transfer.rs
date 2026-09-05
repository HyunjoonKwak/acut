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

fn copy_verified(
    from: &Path,
    to: &Path,
    expected_source_hash: Option<&str>,
) -> std::io::Result<String> {
    if to.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("목적지가 이미 있습니다: {}", to.display()),
        ));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = to.with_file_name(format!(
        ".{}.photo-desk-copy.tmp",
        to.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&temp);
    let got = std::fs::copy(from, &temp)?;
    let want = std::fs::metadata(from)?.len();
    if got != want {
        let _ = std::fs::remove_file(&temp);
        return Err(std::io::Error::other("사본 크기가 원본과 다릅니다"));
    }
    // 발행은 미리보기 직후 실행 계획을 만들며 원본을 이미 해시했다. 그 값을
    // 재사용해 같은 큰 파일을 실행 안에서 또 읽지 않는다. 일반 복사는 여기서
    // 한 번 계산한다.
    let source_hash = match expected_source_hash {
        Some(hash) => hash.to_string(),
        None => crate::cull::hash::full(from)?,
    };
    let dest_hash = crate::cull::hash::full(&temp)?;
    if source_hash != dest_hash {
        let _ = std::fs::remove_file(&temp);
        // 계획 해시와 다르면 «사본이 틀렸다»가 아니라 «원본이 그새 바뀌었다»는 뜻이다
        return Err(std::io::Error::other(if expected_source_hash.is_some() {
            "미리보기 뒤 원본 내용이 바뀌었습니다. 다시 미리보기 하세요"
        } else {
            "사본 SHA-256이 원본과 다릅니다"
        }));
    }
    copy_mtime(from, &temp);
    std::fs::File::open(&temp)?.sync_all()?;
    if to.exists() {
        let _ = std::fs::remove_file(&temp);
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("복사 중 목적지가 생겼습니다: {}", to.display()),
        ));
    }
    std::fs::rename(&temp, to)?;
    if let Some(parent) = to.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(source_hash)
}

#[derive(Debug)]
struct CopiedSidecar {
    target: PathBuf,
    sha256: String,
}

fn copy_with_sidecars(
    from: &Path,
    to: &Path,
    expected_source_hash: Option<&str>,
) -> std::io::Result<(String, Vec<CopiedSidecar>)> {
    let pairs = sidecars(from, to);
    if let Some((_, target)) = pairs.iter().find(|(_, target)| target.exists()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("목적지 사이드카가 이미 있습니다: {}", target.display()),
        ));
    }
    let hash = copy_verified(from, to, expected_source_hash)?;
    let mut copied = Vec::new();
    for (source, target) in pairs {
        match copy_verified(&source, &target, None) {
            Ok(sha256) => copied.push(CopiedSidecar { target, sha256 }),
            Err(error) => {
                for sidecar in &copied {
                    let _ = std::fs::remove_file(&sidecar.target);
                }
                let _ = std::fs::remove_file(to);
                return Err(error);
            }
        }
    }
    Ok((hash, copied))
}

fn ensure_folder(db: &Db, lib: &crate::db::libraries::Library, rel: &str) -> Result<i64> {
    let vol_rel = crate::media::cache::rel_path(&lib.rel_path, rel);
    let name = rel.rsplit('/').next().unwrap_or(&lib.name);
    // 겹쳐 등록된 다른 라이브러리의 행은 빼앗지 않는다 — organize::ensure_folder 와 같은 규칙
    db.write(|c|c.execute("INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at) VALUES(?1,?2,?3,?4,?5,strftime('%s','now')) ON CONFLICT(volume_uuid,rel_path) DO NOTHING",rusqlite::params![lib.volume_uuid,lib.id,vol_rel,name,lib.area]))?;
    let (id, owner): (i64, i64) = db.read(|c| {
        c.query_row(
            "SELECT id, library_id FROM folders WHERE volume_uuid=?1 AND rel_path=?2",
            rusqlite::params![lib.volume_uuid, vol_rel],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    })?;
    if owner != lib.id {
        return Err(DbError::Invalid(
            "목적지 폴더가 이미 다른 라이브러리에 속해 있습니다".into(),
        ));
    }
    Ok(id)
}

pub(crate) fn clone_row(db: &Db, source: i64, folder: i64, name: &str, hash: &str) -> Result<i64> {
    db.transaction(|tx|{
        tx.execute(
            "INSERT INTO files(folder_id,name,ext,size,kind,taken_at,taken_at_source,created_at,modified_at,
             quick_hash,full_hash,image_hash,phash,psig,width,height,orientation,duration_ms,culling_flag,rating,favorite,comment,
             cam_make,cam_model,lens,iso,aperture,shutter,focal_mm,gps_lat,gps_lon,gps_alt,geo_name,geo_country,geo_admin1,geo_admin2,
             sharpness,exposure,embedding,inode,scanned_at)
             SELECT ?2,?3,ext,size,kind,taken_at,taken_at_source,created_at,modified_at,NULL,?4,image_hash,phash,psig,width,height,orientation,duration_ms,
             culling_flag,rating,favorite,comment,cam_make,cam_model,lens,iso,aperture,shutter,focal_mm,gps_lat,gps_lon,gps_alt,geo_name,geo_country,
             geo_admin1,geo_admin2,sharpness,exposure,embedding,NULL,strftime('%s','now') FROM files WHERE id=?1",
            rusqlite::params![source,folder,name,hash],
        )?;
        let id=tx.last_insert_rowid();
        tx.execute("INSERT OR IGNORE INTO file_tags(file_id,tag_id) SELECT ?2,tag_id FROM file_tags WHERE file_id=?1",rusqlite::params![source,id])?;
        tx.execute("INSERT OR IGNORE INTO capture_date_overrides(file_id,taken_at) SELECT ?2,taken_at FROM capture_date_overrides WHERE file_id=?1",rusqlite::params![source,id])?;
        Ok(id)
    })
}

pub fn execute(db: &Db, request: &Request, label: &str) -> Result<TransferOutcome> {
    let plan = preview(db, request)?;
    let items = load(db, &request.ids)?;
    if items.is_empty() {
        return Ok(TransferOutcome {
            first_error: Some("옮기거나 복사할 사진이 없습니다".into()),
            ..Default::default()
        });
    }
    let planned = plan
        .items
        .iter()
        .map(|item| (item.id, item))
        .collect::<std::collections::HashMap<_, _>>();
    if planned.len() != items.len() || items.iter().any(|item| !planned.contains_key(&item.id)) {
        return Err(DbError::Invalid(
            "실행 계획과 현재 사진 목록이 달라졌습니다. 다시 미리보기 하세요".into(),
        ));
    }
    let (lib, rel, dir) = dest(db, request)?;
    // 목적지 폴더는 실제로 옮길 사진이 있을 때 만든다 — 전부 건너뛰는 실행이 빈 폴더를
    // 디스크에 남기면 안 된다
    let mut folder: Option<i64> = None;
    let kind = if plan.publish {
        "publish"
    } else if request.mode == Mode::Copy {
        "copy"
    } else {
        "move"
    };
    let batch = super::open_batch(db, kind, label)?;
    let mut out = TransferOutcome {
        batch_id: batch,
        ..Default::default()
    };
    for it in &items {
        let p = planned
            .get(&it.id)
            .expect("위에서 실행 계획과 사진 ID를 모두 대조했다");
        if p.action == "skip" {
            out.skipped += 1;
            if p.conflict == "already_published" {
                out.already_published += 1;
            }
            continue;
        }
        let result = (|| -> Result<()> {
            let source = mount_path(it)?;
            let target = dir.join(&p.planned_name);
            if source == target {
                return Ok(());
            }
            if target.exists() {
                return Err(DbError::Invalid(format!(
                    "실행 직전 같은 이름이 생겼습니다: {}",
                    p.planned_name
                )));
            }
            let folder = match folder {
                Some(folder) => folder,
                None => {
                    std::fs::create_dir_all(&dir)
                        .io_context("전송 목적지 폴더를 만들다가 실패했습니다")?;
                    let made = ensure_folder(db, &lib, &rel)?;
                    folder = Some(made);
                    made
                }
            };
            let dest_vol_rel = crate::media::cache::rel_path(
                &crate::media::cache::rel_path(&lib.rel_path, &rel),
                &p.planned_name,
            );
            match request.mode {
                Mode::Move => {
                    let occupied: bool = db.read(|c| {
                        c.query_row(
                            "SELECT EXISTS(SELECT 1 FROM files WHERE folder_id=?1 AND name=?2 AND id<>?3)",
                            rusqlite::params![folder,p.planned_name,it.id],
                            |r| r.get(0),
                        )
                    })?;
                    if occupied {
                        return Err(DbError::Invalid(format!(
                            "목적지 DB에 같은 이름 기록이 있습니다: {}",
                            p.planned_name
                        )));
                    }
                    move_with_sidecars(&source, &target)
                        .io_context("파일과 사이드카를 목적지로 옮기다가 실패했습니다")?;
                    let (to_size, to_mtime) = super::file_stat(&target);
                    let changed = db.transaction(|tx| {
                        tx.execute(
                            "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok,to_size,to_mtime)
                             VALUES(?1,?2,'move',?3,?4,?5,?6,1,?7,?8)",
                            rusqlite::params![batch,it.id,it.volume_uuid,it.vol_rel,lib.volume_uuid,dest_vol_rel,to_size,to_mtime],
                        )?;
                        tx.execute(
                            "UPDATE files SET folder_id=?2,name=?3 WHERE id=?1",
                            rusqlite::params![it.id, folder, p.planned_name],
                        )?;
                        // 썸네일 캐시는 라이브러리별 루트에 있다. 다른 라이브러리로
                        // 옮긴 뒤 예전 완료 행을 남기면 통계는 완료인데 UI는 빈 그림이
                        // 된다. 행을 지워 다음 썸네일 생성에서 정확히 다시 만들게 한다.
                        if it.library_id != lib.id {
                            tx.execute("DELETE FROM thumbs WHERE file_id=?1", [it.id])?;
                        }
                        Ok(())
                    });
                    if let Err(error) = changed {
                        return match move_with_sidecars(&target, &source) {
                            Ok(()) => Err(error),
                            Err(rollback) => Err(DbError::Invalid(format!(
                                "DB 갱신 실패: {error}; 파일 원위치 복구도 실패: {rollback}"
                            ))),
                        };
                    }
                }
                Mode::Copy => {
                    // 계획 해시를 넘기면 copy_verified 가 사본과 대조해 원본 변경을 잡는다
                    let (full, copied_sidecars) =
                        copy_with_sidecars(&source, &target, p.source_sha256.as_deref())
                            .io_context("파일과 사이드카를 목적지로 복사하다가 실패했습니다")?;
                    let new_id = match clone_row(db, it.id, folder, &p.planned_name, &full) {
                        Ok(id) => id,
                        Err(e) => {
                            let _ = std::fs::remove_file(&target);
                            for sidecar in &copied_sidecars {
                                let _ = std::fs::remove_file(&sidecar.target);
                            }
                            return Err(e);
                        }
                    };
                    let destination_vol_dir = crate::media::cache::rel_path(&lib.rel_path, &rel);
                    let recorded = db.transaction(|tx| {
                        tx.execute(
                            "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok)
                             VALUES(?1,?2,'copy',?3,?4,?5,?6,1)",
                            rusqlite::params![batch,new_id,it.volume_uuid,it.vol_rel,lib.volume_uuid,dest_vol_rel],
                        )?;
                        tx.execute(
                            "INSERT INTO copy_manifest(batch_id,file_id,seq,to_vol,to_path,sha256,is_main)
                             VALUES(?1,?2,0,?3,?4,?5,1)",
                            rusqlite::params![batch,new_id,lib.volume_uuid,dest_vol_rel,full],
                        )?;
                        for (index, sidecar) in copied_sidecars.iter().enumerate() {
                            let name = sidecar
                                .target
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            let path = crate::media::cache::rel_path(&destination_vol_dir, &name);
                            tx.execute(
                                "INSERT INTO copy_manifest(batch_id,file_id,seq,to_vol,to_path,sha256,is_main)
                                 VALUES(?1,?2,?3,?4,?5,?6,0)",
                                rusqlite::params![batch,new_id,index as i64+1,lib.volume_uuid,path,sidecar.sha256],
                            )?;
                        }
                        if plan.publish {
                            let dest_path=if rel.is_empty(){p.planned_name.clone()}else{format!("{rel}/{}",p.planned_name)};
                            tx.execute("INSERT INTO publication_ledger(source_file_id,source_sha256,destination_library_id,destination_path,destination_sha256,batch_id) VALUES(?1,?2,?3,?4,?2,?5) ON CONFLICT(source_sha256,destination_library_id,destination_path) DO UPDATE SET destination_sha256=excluded.destination_sha256,batch_id=excluded.batch_id,created_at=strftime('%s','now')",rusqlite::params![it.id,full,lib.id,dest_path,batch])?;
                        }
                        Ok(())
                    });
                    if let Err(e) = recorded {
                        let _ = db.write(|c| c.execute("DELETE FROM files WHERE id=?1", [new_id]));
                        let _ = std::fs::remove_file(&target);
                        for sidecar in &copied_sidecars {
                            let _ = std::fs::remove_file(&sidecar.target);
                        }
                        return Err(e);
                    }
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                out.completed += 1;
                out.bytes += it.size;
            }
            Err(e) => {
                out.failed += 1;
                out.failed_ids.push(it.id);
                out.first_error.get_or_insert(e.to_string());
                let msg = e.to_string();
                let _ = super::record(
                    db,
                    batch,
                    kind,
                    it.id,
                    &it.volume_uuid,
                    &it.vol_rel,
                    None,
                    Err(&msg),
                );
            }
        }
    }
    super::close_batch(db, batch, out.completed)?;
    let affected = items
        .iter()
        .map(|i| i.library_id)
        .chain(std::iter::once(lib.id))
        .collect::<std::collections::BTreeSet<_>>();
    for id in &affected {
        if let Err(error) = db.write(|c|c.execute("UPDATE folders SET file_count=(SELECT COUNT(*) FROM files WHERE files.folder_id=folders.id AND files.trashed_at IS NULL) WHERE library_id=?1",[id])) {
            log::warn!("이동·복사 뒤 폴더 장수 갱신 보류: {error}");
        }
    }
    for id in affected {
        if let Err(error) = crate::ops::organize::forget_empty_folders(db, id) {
            log::warn!("이동·복사 뒤 빈 폴더 정리 보류: {error}");
        }
    }
    Ok(out)
}

pub fn undo_copy(db: &Db, batch_id: i64) -> Result<Outcome> {
    #[derive(Debug)]
    struct CopyRow {
        id: i64,
        volume: String,
        path: String,
        expected: Option<String>,
    }
    #[derive(Debug)]
    struct Artifact {
        volume: String,
        path: String,
        sha256: String,
    }
    let rows: Vec<CopyRow> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT j.file_id,COALESCE(j.to_vol,j.from_vol),j.to_path,fi.full_hash
             FROM journal j LEFT JOIN files fi ON fi.id=j.file_id
             WHERE j.batch_id=?1 AND j.ok=1 AND j.file_id IS NOT NULL
               AND j.to_path IS NOT NULL ORDER BY j.id DESC",
        )?;
        let rows = st.query_map([batch_id], |r| {
            Ok(CopyRow {
                id: r.get(0)?,
                volume: r.get(1)?,
                path: r.get(2)?,
                expected: r.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };
    for row in rows {
        let result = (|| -> Result<()> {
            let mount = crate::db::volumes::find_mount(&row.volume)
                .ok_or_else(|| DbError::Invalid("사본 디스크가 연결되어 있지 않습니다".into()))?;
            let main = mount.join(&row.path);
            let mut artifacts: Vec<Artifact> = db.read(|c| {
                let mut st = c.prepare(
                    "SELECT to_vol,to_path,sha256 FROM copy_manifest
                     WHERE batch_id=?1 AND file_id=?2 ORDER BY seq",
                )?;
                let rows = st.query_map(rusqlite::params![batch_id, row.id], |r| {
                    Ok(Artifact {
                        volume: r.get(0)?,
                        path: r.get(1)?,
                        sha256: r.get(2)?,
                    })
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })?;
            if artifacts.is_empty() {
                let expected = row.expected.clone().ok_or_else(|| {
                    DbError::Invalid("사본의 원래 SHA-256 기록이 없어 지우지 않았습니다".into())
                })?;
                if !crate::ops::trash::sidecars(&main, &main).is_empty() {
                    return Err(DbError::Invalid(
                        "0.9.0 사본의 사이드카 경로 기록이 없어 안전하게 되돌릴 수 없습니다".into(),
                    ));
                }
                artifacts.push(Artifact {
                    volume: row.volume.clone(),
                    path: row.path.clone(),
                    sha256: expected,
                });
            }
            let mut paths = Vec::with_capacity(artifacts.len());
            for artifact in &artifacts {
                let artifact_mount =
                    crate::db::volumes::find_mount(&artifact.volume).ok_or_else(|| {
                        DbError::Invalid("사본 디스크가 연결되어 있지 않습니다".into())
                    })?;
                let path = artifact_mount.join(&artifact.path);
                if !path.is_file() {
                    return Err(DbError::Invalid(format!(
                        "되돌릴 사본이 없습니다: {}",
                        artifact.path
                    )));
                }
                if crate::cull::hash::full(&path)
                    .io_context("되돌릴 사본의 해시를 읽다가 실패했습니다")?
                    != artifact.sha256
                {
                    return Err(DbError::Invalid(format!(
                        "사본 내용이 작업 뒤 바뀌어 지우지 않았습니다: {}",
                        artifact.path
                    )));
                }
                paths.push(path);
            }

            // 영구 삭제 전에 같은 폴더 안의 숨은 이름으로 원자적으로 치워 둔다.
            // DB 갱신이 실패하면 전부 제자리로 돌릴 수 있다.
            let mut staged = Vec::with_capacity(paths.len());
            for path in paths {
                let temp = free_path(path.with_file_name(format!(
                    ".{}.photo-desk-undo-{batch_id}.tmp",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )));
                if let Err(error) = std::fs::rename(&path, &temp) {
                    for (original, staged_path) in staged.iter().rev() {
                        let _ = std::fs::rename(staged_path, original);
                    }
                    return Err(DbError::Invalid(format!(
                        "되돌릴 사본을 임시 위치로 옮기다가 실패했습니다: {error}"
                    )));
                }
                staged.push((path, temp));
            }
            let changed = db.transaction(|tx| {
                tx.execute("DELETE FROM files WHERE id=?1", [row.id])?;
                tx.execute(
                    "UPDATE journal SET ok=0 WHERE batch_id=?1 AND file_id=?2 AND ok=1",
                    rusqlite::params![batch_id, row.id],
                )?;
                Ok(())
            });
            if let Err(error) = changed {
                for (original, staged_path) in staged.iter().rev() {
                    let _ = std::fs::rename(staged_path, original);
                }
                return Err(error);
            }
            for (_, staged_path) in staged {
                if let Err(error) = std::fs::remove_file(&staged_path) {
                    log::warn!(
                        "되돌린 사본 임시 파일을 지우지 못했습니다 {}: {error}",
                        staged_path.display()
                    );
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => out.moved += 1,
            Err(e) => {
                out.failed += 1;
                out.failed_ids.push(row.id);
                out.first_error.get_or_insert(e.to_string());
            }
        }
    }
    if rows_is_empty(db, batch_id)? {
        db.write(|c| {
            c.execute(
                "DELETE FROM publication_ledger WHERE batch_id=?1",
                [batch_id],
            )
        })?;
        crate::ops::undo::mark_undone(db, batch_id)?;
    }
    Ok(out)
}

fn rows_is_empty(db: &Db, batch: i64) -> Result<bool> {
    db.read(|c| {
        c.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM journal WHERE batch_id=?1 AND ok=1)",
            [batch],
            |r| r.get(0),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_test;

    fn setup() -> (tempfile::TempDir, Db, i64, i64, Vec<i64>) {
        let d = tempfile::tempdir().unwrap();
        let mine = d.path().join("내사진");
        let shared = d.path().join("공용");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(mine.join("a.jpg"), b"photo a").unwrap();
        std::fs::write(mine.join("a.xmp"), b"sidecar").unwrap();
        std::fs::write(mine.join("b.jpg"), b"photo b").unwrap();
        let db = Db::open(d.path().join("t.db")).unwrap();
        scan_test(&db, &mine, 1, |_| {}).unwrap();
        scan_test(&db, &shared, 2, |_| {}).unwrap();
        let libs: Vec<(i64, i32)> = crate::db::libraries::list(&db)
            .unwrap()
            .into_iter()
            .map(|l| (l.id, l.area))
            .collect();
        let mine_id = libs.iter().find(|x| x.1 == 1).unwrap().0;
        let shared_id = libs.iter().find(|x| x.1 == 2).unwrap().0;
        let ids = db
            .read(|c| {
                let mut s = c.prepare("SELECT id FROM files ORDER BY name")?;
                let r = s.query_map([], |r| r.get(0))?;
                r.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        (d, db, mine_id, shared_id, ids)
    }

    #[test]
    fn publish_defaults_to_copy_keeps_original_and_second_run_is_deduplicated() {
        let (d, db, _mine, shared, ids) = setup();
        let req = Request {
            ids: ids[..1].to_vec(),
            destination_library_id: shared,
            destination_dir: "가족".into(),
            mode: Mode::Copy,
            conflict_policy: ConflictPolicy::Skip,
            publish: true,
        };
        let first = execute(&db, &req, "공용 발행").unwrap();
        assert_eq!((first.completed, first.failed), (1, 0));
        assert!(d.path().join("내사진/a.jpg").is_file());
        assert!(d.path().join("공용/가족/a.jpg").is_file());
        assert!(d.path().join("공용/가족/a.xmp").is_file());
        let second = execute(&db, &req, "공용 발행").unwrap();
        assert_eq!((second.completed, second.already_published), (0, 1));
        let u = undo_copy(&db, first.batch_id).unwrap();
        assert_eq!(u.moved, 1);
        assert!(d.path().join("내사진/a.jpg").is_file());
        assert!(!d.path().join("공용/가족/a.jpg").exists());
    }

    #[test]
    fn collision_is_previewed_and_rename_never_overwrites() {
        let (d, db, _mine, shared, ids) = setup();
        std::fs::create_dir_all(d.path().join("공용/가족")).unwrap();
        std::fs::write(d.path().join("공용/가족/a.jpg"), b"existing").unwrap();
        let mut req = Request {
            ids: ids[..1].to_vec(),
            destination_library_id: shared,
            destination_dir: "가족".into(),
            mode: Mode::Copy,
            conflict_policy: ConflictPolicy::Skip,
            publish: false,
        };
        let p = preview(&db, &req).unwrap();
        assert_eq!(
            (p.items[0].conflict.as_str(), p.items[0].action.as_str()),
            ("name_exists", "skip")
        );
        req.conflict_policy = ConflictPolicy::Rename;
        let p = preview(&db, &req).unwrap();
        assert_eq!(p.items[0].action, "rename");
        let out = execute(&db, &req, "복사").unwrap();
        assert_eq!(out.completed, 1);
        assert_eq!(
            std::fs::read(d.path().join("공용/가족/a.jpg")).unwrap(),
            b"existing"
        );
    }

    #[test]
    fn partial_failure_records_success_and_undo_only_removes_the_copy() {
        let (d, db, _mine, shared, ids) = setup();
        std::fs::remove_file(d.path().join("내사진/a.jpg")).unwrap();
        let req = Request {
            ids: ids.clone(),
            destination_library_id: shared,
            destination_dir: "부분".into(),
            mode: Mode::Copy,
            conflict_policy: ConflictPolicy::Rename,
            publish: false,
        };
        let out = execute(&db, &req, "부분 복사").unwrap();
        assert_eq!((out.completed, out.failed), (1, 1));
        assert!(d.path().join("내사진/b.jpg").is_file());
        assert!(d.path().join("공용/부분/b.jpg").is_file());
        assert_eq!(undo_copy(&db, out.batch_id).unwrap().moved, 1);
        assert!(d.path().join("내사진/b.jpg").is_file());
        assert!(!d.path().join("공용/부분/b.jpg").exists());
    }

    #[test]
    fn move_uses_the_shared_batch_journal_and_standard_undo() {
        let (d, db, _mine, shared, ids) = setup();
        db.write(|c| {
            c.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state) VALUES(?1,'aa/old.jpg',1,1,1)",
                [ids[0]],
            )
        })
        .unwrap();
        let req = Request {
            ids: ids[..1].to_vec(),
            destination_library_id: shared,
            destination_dir: "이동".into(),
            mode: Mode::Move,
            conflict_policy: ConflictPolicy::Rename,
            publish: false,
        };
        let out = execute(&db, &req, "임의 이동").unwrap();
        assert_eq!((out.completed, out.failed), (1, 0));
        assert!(!d.path().join("내사진/a.jpg").exists());
        assert!(d.path().join("공용/이동/a.jpg").is_file());
        assert!(d.path().join("공용/이동/a.xmp").is_file());
        assert_eq!(
            db.read(|c| c.query_row(
                "SELECT COUNT(*) FROM thumbs WHERE file_id=?1",
                [ids[0]],
                |r| r.get::<_, i64>(0),
            ))
            .unwrap(),
            0,
            "라이브러리별 캐시 루트가 바뀌면 기존 썸네일은 재생성 대기가 된다"
        );

        let undo = crate::ops::undo::undo(&db, out.batch_id).unwrap();
        assert_eq!((undo.moved, undo.failed), (1, 0));
        assert!(d.path().join("내사진/a.jpg").is_file());
        assert!(d.path().join("내사진/a.xmp").is_file());
    }

    #[test]
    fn undo_refuses_a_copy_changed_after_the_operation() {
        let (d, db, _mine, shared, ids) = setup();
        let req = Request {
            ids: ids[..1].to_vec(),
            destination_library_id: shared,
            destination_dir: "변경".into(),
            mode: Mode::Copy,
            conflict_policy: ConflictPolicy::Skip,
            publish: false,
        };
        let out = execute(&db, &req, "복사").unwrap();
        let target = d.path().join("공용/변경/a.jpg");
        std::fs::write(&target, b"edited after copy").unwrap();
        let undone = undo_copy(&db, out.batch_id).unwrap();
        assert_eq!((undone.moved, undone.failed), (0, 1));
        assert_eq!(std::fs::read(&target).unwrap(), b"edited after copy");
        assert!(d.path().join("공용/변경/a.xmp").is_file());
    }

    #[test]
    fn copy_sidecar_collision_never_overwrites_or_deletes_the_existing_file() {
        let (d, db, _mine, shared, ids) = setup();
        std::fs::create_dir_all(d.path().join("공용/충돌")).unwrap();
        let existing = d.path().join("공용/충돌/a.xmp");
        std::fs::write(&existing, b"someone else's metadata").unwrap();
        let req = Request {
            ids: ids[..1].to_vec(),
            destination_library_id: shared,
            destination_dir: "충돌".into(),
            mode: Mode::Copy,
            conflict_policy: ConflictPolicy::Skip,
            publish: false,
        };
        let out = execute(&db, &req, "사이드카 충돌").unwrap();
        assert_eq!((out.completed, out.failed), (0, 1));
        assert!(!d.path().join("공용/충돌/a.jpg").exists());
        assert_eq!(
            std::fs::read(&existing).unwrap(),
            b"someone else's metadata"
        );
    }

    #[test]
    fn undo_refuses_a_changed_copied_sidecar_and_keeps_the_whole_copy() {
        let (d, db, _mine, shared, ids) = setup();
        let req = Request {
            ids: ids[..1].to_vec(),
            destination_library_id: shared,
            destination_dir: "sidecar".into(),
            mode: Mode::Copy,
            conflict_policy: ConflictPolicy::Skip,
            publish: false,
        };
        let out = execute(&db, &req, "사이드카 복사").unwrap();
        let sidecar = d.path().join("공용/sidecar/a.xmp");
        std::fs::write(&sidecar, b"edited metadata").unwrap();
        let undone = undo_copy(&db, out.batch_id).unwrap();
        assert_eq!((undone.moved, undone.failed), (0, 1));
        assert!(d.path().join("공용/sidecar/a.jpg").is_file());
        assert_eq!(std::fs::read(&sidecar).unwrap(), b"edited metadata");
    }

    #[test]
    fn preview_reserves_names_within_the_same_batch() {
        let d = tempfile::tempdir().unwrap();
        let mine = d.path().join("내사진");
        let shared = d.path().join("공용");
        std::fs::create_dir_all(mine.join("one")).unwrap();
        std::fs::create_dir_all(mine.join("two")).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(mine.join("one/a.jpg"), b"first").unwrap();
        std::fs::write(mine.join("two/a.jpg"), b"second").unwrap();
        let db = Db::open(d.path().join("t.db")).unwrap();
        scan_test(&db, &mine, 1, |_| {}).unwrap();
        scan_test(&db, &shared, 2, |_| {}).unwrap();
        let libs = crate::db::libraries::list(&db).unwrap();
        let shared_id = libs.iter().find(|library| library.area == 2).unwrap().id;
        let ids = db
            .read(|c| {
                let mut statement = c.prepare("SELECT id FROM files ORDER BY id")?;
                let rows = statement
                    .query_map([], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<i64>>>()?;
                Ok(rows)
            })
            .unwrap();
        let plan = preview(
            &db,
            &Request {
                ids,
                destination_library_id: shared_id,
                destination_dir: String::new(),
                mode: Mode::Copy,
                conflict_policy: ConflictPolicy::Rename,
                publish: false,
            },
        )
        .unwrap();
        assert_eq!(plan.items[0].planned_name, "a.jpg");
        assert_eq!(plan.items[1].planned_name, "a (2).jpg");
        assert_eq!(plan.items[1].conflict, "batch_name_exists");
    }

    #[test]
    fn a_db_name_conflict_is_found_before_a_move_touches_the_file() {
        let (d, db, _mine, shared, ids) = setup();
        let destination = crate::db::libraries::get(&db, shared).unwrap().unwrap();
        std::fs::create_dir_all(d.path().join("공용/blocked")).unwrap();
        let folder = ensure_folder(&db, &destination, "blocked").unwrap();
        clone_row(&db, ids[0], folder, "a.jpg", "ghost").unwrap();
        let out = execute(
            &db,
            &Request {
                ids: ids[..1].to_vec(),
                destination_library_id: shared,
                destination_dir: "blocked".into(),
                mode: Mode::Move,
                conflict_policy: ConflictPolicy::Skip,
                publish: false,
            },
            "이동",
        )
        .unwrap();
        assert_eq!((out.completed, out.failed), (0, 1));
        assert!(d.path().join("내사진/a.jpg").is_file());
        assert!(!d.path().join("공용/blocked/a.jpg").exists());
    }

    #[test]
    fn a_destination_symlink_cannot_escape_the_library() {
        use std::os::unix::fs::symlink;

        let (d, db, _mine, shared, ids) = setup();
        let outside = d.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, d.path().join("공용/escape")).unwrap();
        let error = preview(
            &db,
            &Request {
                ids: ids[..1].to_vec(),
                destination_library_id: shared,
                destination_dir: "escape".into(),
                mode: Mode::Copy,
                conflict_policy: ConflictPolicy::Skip,
                publish: false,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("밖"));
    }
}
