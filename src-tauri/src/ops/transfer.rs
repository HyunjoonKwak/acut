//! 임의 목적지 이동·복사와 공용 발행.
//!
//! 실행 전에 같은 이름과 이미 발행한 hash를 모두 보여 주고, 실제 작업은 기존
//! 볼륨 경로·sidecar·batch journal 규칙을 따른다.

use crate::db::conn::{Db, DbError, Result};
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
    folder_id: i64,
    library_id: i64,
    source_area: i32,
    volume_uuid: String,
    vol_rel: String,
    name: String,
    size: i64,
    full_hash: Option<String>,
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
            || part.contains(':')
            || part.chars().any(|c| (c as u32) < 0x20)
    }) {
        return Err(DbError::Invalid(
            "목적지 폴더 이름에 쓸 수 없는 문자가 있습니다".into(),
        ));
    }
    Ok(clean)
}

fn load(db: &Db, ids: &[i64]) -> Result<Vec<Item>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let marks = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    db.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT fi.id,fi.folder_id,fo.library_id,l.area,fo.volume_uuid,
                    fo.rel_path || CASE WHEN fo.rel_path='' THEN '' ELSE '/' END || fi.name,
                    fi.name,fi.size,fi.full_hash
             FROM files fi JOIN folders fo ON fo.id=fi.folder_id JOIN libraries l ON l.id=fo.library_id
             WHERE fi.trashed_at IS NULL AND fi.id IN ({marks}) ORDER BY fi.id"
        ))?;
        let rows = st.query_map(rusqlite::params_from_iter(ids.iter()), |r| Ok(Item {
            id:r.get(0)?,folder_id:r.get(1)?,library_id:r.get(2)?,source_area:r.get(3)?,
            volume_uuid:r.get(4)?,vol_rel:r.get(5)?,name:r.get(6)?,size:r.get(7)?,full_hash:r.get(8)?,
        }))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
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

fn hash(db: &Db, it: &Item, path: &Path) -> Result<String> {
    let h = crate::cull::hash::full(path)?;
    if it.full_hash.as_deref() != Some(&h) {
        db.write(|c| {
            c.execute(
                "UPDATE files SET full_hash=?2 WHERE id=?1",
                rusqlite::params![it.id, h],
            )
        })?;
    }
    Ok(h)
}

fn dest(db: &Db, request: &Request) -> Result<(crate::db::libraries::Library, String, PathBuf)> {
    let rel = validate_rel(&request.destination_dir)?;
    let lib = crate::db::libraries::get(db, request.destination_library_id)?
        .ok_or_else(|| DbError::Invalid("등록되지 않은 목적지 라이브러리입니다".into()))?;
    let dir = lib
        .dir
        .clone()
        .ok_or_else(|| DbError::Invalid("목적지 디스크가 연결되어 있지 않습니다".into()))?;
    let full = if rel.is_empty() { dir } else { dir.join(&rel) };
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
        let source_hash = if publish {
            Some(hash(db, &it, &source_path)?)
        } else {
            None
        };
        let existing_publication = match source_hash.as_deref() {
            Some(h) => published_path(db, h, lib.id)?,
            None => None,
        };
        let wanted = dir.join(&it.name);
        let (conflict, action, planned) = if let Some(path) = existing_publication {
            let full = lib.dir.as_ref().unwrap().join(&path);
            let valid = full.is_file()
                && source_hash
                    .as_deref()
                    .is_some_and(|h| crate::cull::hash::full(&full).ok().as_deref() == Some(h));
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

fn copy_verified(from: &Path, to: &Path) -> std::io::Result<String> {
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
    let source_hash = crate::cull::hash::full(from)?;
    let dest_hash = crate::cull::hash::full(&temp)?;
    if source_hash != dest_hash {
        let _ = std::fs::remove_file(&temp);
        return Err(std::io::Error::other("사본 SHA-256이 원본과 다릅니다"));
    }
    copy_mtime(from, &temp);
    std::fs::rename(&temp, to)?;
    Ok(source_hash)
}

fn copy_with_sidecars(from: &Path, to: &Path) -> std::io::Result<String> {
    let pairs = sidecars(from, to);
    let hash = copy_verified(from, to)?;
    for (src, want) in pairs {
        let target = free_path(want);
        if let Err(e) = copy_verified(&src, &target) {
            log::warn!(
                "사이드카 복사 실패 {} → {}: {e}",
                src.display(),
                target.display()
            );
        }
    }
    Ok(hash)
}

fn ensure_folder(db: &Db, lib: &crate::db::libraries::Library, rel: &str) -> Result<i64> {
    let vol_rel = crate::media::cache::rel_path(&lib.rel_path, rel);
    let name = rel.rsplit('/').next().unwrap_or(&lib.name);
    db.write(|c|c.execute("INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at) VALUES(?1,?2,?3,?4,?5,strftime('%s','now')) ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET library_id=excluded.library_id",rusqlite::params![lib.volume_uuid,lib.id,vol_rel,name,lib.area]))?;
    db.read(|c| {
        c.query_row(
            "SELECT id FROM folders WHERE volume_uuid=?1 AND rel_path=?2",
            rusqlite::params![lib.volume_uuid, vol_rel],
            |r| r.get(0),
        )
    })
}

fn clone_row(db: &Db, source: i64, folder: i64, name: &str, hash: &str) -> Result<i64> {
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
    let (lib, rel, dir) = dest(db, request)?;
    std::fs::create_dir_all(&dir)?;
    let folder = ensure_folder(db, &lib, &rel)?;
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
    for (it, p) in items.iter().zip(&plan.items) {
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
            let dest_vol_rel = crate::media::cache::rel_path(
                &crate::media::cache::rel_path(&lib.rel_path, &rel),
                &p.planned_name,
            );
            match request.mode {
                Mode::Move => {
                    move_with_sidecars(&source, &target)?;
                    super::record_to(
                        db,
                        batch,
                        "move",
                        it.id,
                        &it.volume_uuid,
                        &it.vol_rel,
                        &lib.volume_uuid,
                        Some(&dest_vol_rel),
                        Ok(()),
                    )?;
                    db.write(|c| {
                        c.execute(
                            "UPDATE files SET folder_id=?2,name=?3 WHERE id=?1",
                            rusqlite::params![it.id, folder, p.planned_name],
                        )
                    })?;
                }
                Mode::Copy => {
                    let full = copy_with_sidecars(&source, &target)?;
                    if p.source_sha256.as_deref().is_some_and(|h| h != full) {
                        let _ = std::fs::remove_file(&target);
                        return Err(DbError::Invalid(
                            "미리보기 뒤 원본 내용이 바뀌었습니다".into(),
                        ));
                    }
                    let new_id = match clone_row(db, it.id, folder, &p.planned_name, &full) {
                        Ok(id) => id,
                        Err(e) => {
                            let _ = std::fs::remove_file(&target);
                            crate::ops::trash::remove_sidecars(&target);
                            return Err(e);
                        }
                    };
                    let recorded = db.transaction(|tx| {
                        tx.execute(
                            "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok)
                             VALUES(?1,?2,'copy',?3,?4,?5,?6,1)",
                            rusqlite::params![batch,new_id,it.volume_uuid,it.vol_rel,lib.volume_uuid,dest_vol_rel],
                        )?;
                        if plan.publish {
                            let dest_path=if rel.is_empty(){p.planned_name.clone()}else{format!("{rel}/{}",p.planned_name)};
                            tx.execute("INSERT INTO publication_ledger(source_file_id,source_sha256,destination_library_id,destination_path,destination_sha256,batch_id) VALUES(?1,?2,?3,?4,?2,?5) ON CONFLICT(source_sha256,destination_library_id,destination_path) DO UPDATE SET destination_sha256=excluded.destination_sha256,batch_id=excluded.batch_id,created_at=strftime('%s','now')",rusqlite::params![it.id,full,lib.id,dest_path,batch])?;
                        }
                        Ok(())
                    });
                    if let Err(e) = recorded {
                        let _ = db.write(|c| c.execute("DELETE FROM files WHERE id=?1", [new_id]));
                        let _ = std::fs::remove_file(&target);
                        crate::ops::trash::remove_sidecars(&target);
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
        db.write(|c|c.execute("UPDATE folders SET file_count=(SELECT COUNT(*) FROM files WHERE files.folder_id=folders.id AND files.trashed_at IS NULL) WHERE library_id=?1",[id]))?;
    }
    for id in affected {
        crate::ops::organize::forget_empty_folders(db, id)?;
    }
    Ok(out)
}

pub fn undo_copy(db: &Db, batch_id: i64) -> Result<Outcome> {
    let rows:Vec<(i64,String,String)>=db.read(|c|{let mut st=c.prepare("SELECT file_id,COALESCE(to_vol,from_vol),to_path FROM journal WHERE batch_id=?1 AND ok=1 AND file_id IS NOT NULL AND to_path IS NOT NULL ORDER BY id DESC")?;let rows=st.query_map([batch_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;rows.collect::<rusqlite::Result<Vec<_>>>()})?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };
    for (id, vol, rel) in rows {
        let result = (|| -> Result<()> {
            let mount = crate::db::volumes::find_mount(&vol)
                .ok_or_else(|| DbError::Invalid("사본 디스크가 연결되어 있지 않습니다".into()))?;
            let path = mount.join(&rel);
            if path.exists() {
                std::fs::remove_file(&path)?;
                crate::ops::trash::remove_sidecars(&path);
            }
            db.write(|c| c.execute("DELETE FROM files WHERE id=?1", [id]))?;
            Ok(())
        })();
        match result {
            Ok(()) => out.moved += 1,
            Err(e) => {
                out.failed += 1;
                out.failed_ids.push(id);
                out.first_error.get_or_insert(e.to_string());
            }
        }
    }
    if out.moved > 0 || rows_is_empty(db, batch_id)? {
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

        let undo = crate::ops::undo::undo(&db, out.batch_id).unwrap();
        assert_eq!((undo.moved, undo.failed), (1, 0));
        assert!(d.path().join("내사진/a.jpg").is_file());
        assert!(d.path().join("내사진/a.xmp").is_file());
    }
}
