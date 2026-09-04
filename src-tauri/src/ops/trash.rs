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
                fi.name, fi.size, fi.folder_id
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
    if !want.exists() {
        return want;
    }
    let stem = want.file_stem().map(|s| s.to_string_lossy().into_owned());
    let ext = want.extension().map(|s| s.to_string_lossy().into_owned());
    let dir = want.parent().map(Path::to_path_buf).unwrap_or_default();
    let name_for = |n: &str| match (&stem, &ext) {
        (Some(s), Some(e)) => format!("{s} ({n}).{e}"),
        (Some(s), None) => format!("{s} ({n})"),
        _ => n.to_string(),
    };
    for n in 2..10_000 {
        let p = dir.join(name_for(&n.to_string()));
        if !p.exists() {
            return p;
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
            sync_parent(to)?;
            if let Err(error) = std::fs::remove_file(from) {
                let _ = std::fs::remove_file(to);
                return Err(error);
            }
            sync_parent(from)?;
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
pub fn to_trash(db: &Db, ids: &[i64], label: &str) -> Result<Outcome> {
    let items = load(db, ids, false)?;
    if items.is_empty() {
        // 빈 배치를 남기지 않는다 — 되돌리기 목록에 «0장»이 쌓이고 사용자는 «안 된다»고 읽는다
        return Ok(Outcome {
            first_error: Some("휴지통으로 옮길 사진이 없습니다".into()),
            ..Default::default()
        });
    }
    let batch_id = super::open_batch(db, "trash", label)?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };
    // 라이브러리·마운트는 한 번만 — 파일마다 찾으면 5천 장에 수천만 행 스캔·수만 syscall (리뷰 H16)
    let (libs, mounts) = lookups(db, &items)?;
    // 옮기고 나서 비는 폴더 — (폴더 행, 디스크 경로, 라이브러리 뿌리)
    let mut touched: std::collections::BTreeMap<i64, (PathBuf, PathBuf)> =
        std::collections::BTreeMap::new();

    for it in &items {
        let lib = libs.get(&it.library_id);
        let (Some(lib_dir), Some(lib_rel), Some(mount)) = (
            lib.and_then(|l| l.dir.clone()),
            lib.map(|l| l.rel_path.clone()),
            mounts.get(&it.volume_uuid).cloned().flatten(),
        ) else {
            let _ = super::record(
                db,
                batch_id,
                "trash",
                it.id,
                &it.volume_uuid,
                &it.vol_rel,
                None,
                Err("디스크가 연결되어 있지 않습니다"),
            );
            out.failed += 1;
            out.failed_ids.push(it.id);
            out.first_error
                .get_or_insert("디스크가 연결되어 있지 않습니다".into());
            continue;
        };

        let src = mount.join(&it.vol_rel);
        let dest = free_path(trash_root(&lib_dir).join(&it.lib_rel));
        let dest_rel = dest
            .strip_prefix(&lib_dir)
            .unwrap_or(&dest)
            .to_string_lossy()
            .into_owned();

        match move_with_sidecars(&src, &dest) {
            Ok(()) => {
                // 저널 경로는 언제나 볼륨 기준이다 — 되돌릴 때 마운트만 붙이면 된다
                let to_vol_rel = crate::media::cache::rel_path(&lib_rel, &dest_rel);
                // 저널과 행 갱신은 한 트랜잭션. 파일은 이미 휴지통에 있으므로 실패하면
                // 제자리로 돌려놓고 실패로 센다 — 저널만 남고 행이 안 바뀌면 격자엔
                // 보이는데 열리지 않는 사진이 된다 (2차 리뷰 M-4)
                let (to_size, to_mtime) = super::file_stat(&dest);
                let recorded = db.transaction(|tx| {
                    tx.execute(
                        "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok,to_size,to_mtime)
                         VALUES(?1,?2,'trash',?3,?4,?3,?5,1,?6,?7)",
                        rusqlite::params![batch_id, it.id, it.volume_uuid, it.vol_rel, to_vol_rel, to_size, to_mtime],
                    )?;
                    tx.execute(
                        "UPDATE files SET trashed_at = strftime('%s','now'),
                                          trash_path = ?2, trash_batch = ?3
                         WHERE id = ?1",
                        rusqlite::params![it.id, dest_rel, batch_id],
                    )?;
                    Ok(())
                });
                match recorded {
                    Ok(()) => {
                        if let Some(dir) = src.parent() {
                            touched
                                .entry(it.folder_id)
                                .or_insert_with(|| (dir.to_path_buf(), lib_dir.clone()));
                        }
                        out.moved += 1;
                        out.bytes += it.size;
                    }
                    Err(error) => {
                        let message = match move_with_sidecars(&dest, &src) {
                            Ok(()) => error.to_string(),
                            Err(rollback) => format!(
                                "DB 갱신 실패: {error}; 파일 원위치 복구도 실패: {rollback}"
                            ),
                        };
                        let _ = super::record(
                            db,
                            batch_id,
                            "trash",
                            it.id,
                            &it.volume_uuid,
                            &it.vol_rel,
                            None,
                            Err(&message),
                        );
                        out.failed += 1;
                        out.failed_ids.push(it.id);
                        out.first_error.get_or_insert(message);
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                let _ = super::record(
                    db,
                    batch_id,
                    "trash",
                    it.id,
                    &it.volume_uuid,
                    &it.vol_rel,
                    None,
                    Err(&msg),
                );
                out.failed += 1;
                out.failed_ids.push(it.id);
                out.first_error.get_or_insert(msg);
            }
        }
    }

    super::close_batch(db, batch_id, out.moved)?;
    // 사진이 다 나간 폴더는 디스크에서 지운다 — «폴더가 똑같아서» 치운 것인데 빈 껍데기가
    // 남으면 비교 화면에 «0장»으로 다시 나오고 Finder 에도 남는다 (사용자 지적).
    // 폴더 행은 남긴다: 휴지통의 파일 행이 그 폴더를 가리키고(FK CASCADE), 되돌리면 폴더가 되살아난다
    for (folder_id, (dir, lib_dir)) in touched {
        let live: i64 = db.read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM files WHERE folder_id = ?1 AND trashed_at IS NULL",
                [folder_id],
                |r| r.get(0),
            )
        })?;
        if live == 0 {
            out.folders_removed += prune_empty_dirs(&dir, &lib_dir);
        }
    }
    Ok(out)
}

/// Finder 가 남기는 것 — 이것만 있으면 «빈 폴더»로 본다
fn is_junk_entry(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._") || name == "Thumbs.db" || name == "desktop.ini"
}

/// 빈 폴더를 지우고, 그래서 빈 위 폴더도 `stop`(라이브러리 뿌리) 바로 아래까지 올라가며 지운다.
/// 사진·다른 파일·하위 폴더가 하나라도 있으면 손대지 않는다. 지운 폴더 수를 돌려준다
pub fn prune_empty_dirs(dir: &Path, stop: &Path) -> usize {
    let mut n = 0;
    let mut cur = dir.to_path_buf();
    loop {
        if cur == stop
            || !cur.starts_with(stop)
            || cur.file_name().map(|f| f == ".acut").unwrap_or(false)
        {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&cur) else {
            break;
        };
        let mut junk = Vec::new();
        let mut other = false;
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().map(|t| t.is_file()).unwrap_or(false) && is_junk_entry(&name) {
                junk.push(e.path());
            } else {
                other = true;
                break;
            }
        }
        if other {
            break;
        }
        for j in junk {
            let _ = std::fs::remove_file(j);
        }
        if std::fs::remove_dir(&cur).is_err() {
            break;
        }
        n += 1;
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => break,
        }
    }
    n
}

/// 휴지통에서 제자리로 되돌린다. 평점·판정은 그대로 살아 있다.
pub fn restore(db: &Db, ids: &[i64]) -> Result<Outcome> {
    let items = load(db, ids, true)?;
    if items.is_empty() {
        return Ok(Outcome {
            first_error: Some("휴지통에 되돌릴 사진이 없습니다".into()),
            ..Default::default()
        });
    }
    let batch_id = super::open_batch(db, "restore", "휴지통에서 되돌리기")?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };

    let paths: std::collections::HashMap<i64, String> = db.read(|c| {
        let mut st = c.prepare("SELECT id, trash_path FROM files WHERE trashed_at IS NOT NULL")?;
        let it = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))?;
        it.collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()
    })?;
    let (libs, mounts) = lookups(db, &items)?;

    for it in &items {
        let lib = libs.get(&it.library_id);
        let (Some(lib_dir), Some(lib_rel), Some(mount), Some(tp)) = (
            lib.and_then(|l| l.dir.clone()),
            lib.map(|l| l.rel_path.clone()),
            mounts.get(&it.volume_uuid).cloned().flatten(),
            paths.get(&it.id),
        ) else {
            out.failed += 1;
            out.failed_ids.push(it.id);
            out.first_error
                .get_or_insert("되돌릴 위치를 알 수 없습니다".into());
            continue;
        };

        let src = lib_dir.join(tp);
        let dest = free_path(mount.join(&it.vol_rel));
        match move_with_sidecars(&src, &dest) {
            Ok(()) => {
                // 저널 — ⌘Z 로 «되돌리기»를 물릴 수 있게(다시 휴지통으로). 경로는 볼륨 기준
                let from_vol_rel = crate::media::cache::rel_path(&lib_rel, tp);
                let to_vol_rel = dest
                    .strip_prefix(&mount)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| it.vol_rel.clone());
                // 그새 같은 이름이 생겨 «IMG_1 (2).jpg»로 돌아왔을 수 있다 — 행도 그 이름으로.
                // 안 맞추면 다음 치우기·이름 바꾸기가 다른 사진에 걸린다 (리뷰 C5)
                let new_name = dest
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| {
                        it.vol_rel
                            .rsplit('/')
                            .next()
                            .unwrap_or(&it.vol_rel)
                            .to_string()
                    });
                // 저널과 행 갱신은 한 트랜잭션. 실패하면 파일을 휴지통 자리로 되돌린다 —
                // 디스크만 돌아오고 행이 «휴지통»이면 그 사진은 어느 화면에도 없다 (2차 리뷰 M-4)
                let (to_size, to_mtime) = super::file_stat(&dest);
                let recorded = db.transaction(|tx| {
                    tx.execute(
                        "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok,to_size,to_mtime)
                         VALUES(?1,?2,'restore',?3,?4,?3,?5,1,?6,?7)",
                        rusqlite::params![batch_id, it.id, it.volume_uuid, from_vol_rel, to_vol_rel, to_size, to_mtime],
                    )?;
                    tx.execute(
                        "UPDATE files SET trashed_at = NULL, trash_path = NULL, trash_batch = NULL,
                                name = ?2
                         WHERE id = ?1",
                        rusqlite::params![it.id, new_name],
                    )?;
                    Ok(())
                });
                match recorded {
                    Ok(()) => {
                        out.moved += 1;
                        out.bytes += it.size;
                    }
                    Err(error) => {
                        let message = match move_with_sidecars(&dest, &src) {
                            Ok(()) => error.to_string(),
                            Err(rollback) => format!(
                                "DB 갱신 실패: {error}; 파일 원위치 복구도 실패: {rollback}"
                            ),
                        };
                        out.failed += 1;
                        out.failed_ids.push(it.id);
                        out.first_error.get_or_insert(message);
                    }
                }
            }
            Err(e) => {
                out.failed += 1;
                out.failed_ids.push(it.id);
                out.first_error.get_or_insert(e.to_string());
            }
        }
    }
    super::close_batch(db, batch_id, out.moved)?;
    Ok(out)
}

/// 휴지통을 진짜로 비운다. **되돌릴 수 없다.**
///
/// 안전장치: 지우려는 경로가 그 라이브러리의 휴지통 안인지 정규화 후 다시
/// 확인한다. 심볼릭 링크나 `..`으로 밖을 가리키면 건너뛴다.
pub fn empty(db: &Db, ids: &[i64]) -> Result<Outcome> {
    let items = load(db, ids, true)?;
    if items.is_empty() {
        return Ok(Outcome {
            first_error: Some("휴지통이 비어 있습니다".into()),
            ..Default::default()
        });
    }
    let batch_id = super::open_batch(db, "delete", "휴지통 비우기")?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };

    let paths: std::collections::HashMap<i64, String> = db.read(|c| {
        let mut st = c.prepare("SELECT id, trash_path FROM files WHERE trashed_at IS NOT NULL")?;
        let it = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))?;
        it.collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()
    })?;

    let (libs, _) = lookups(db, &items)?;
    for it in &items {
        let (Some(lib_dir), Some(tp)) = (
            libs.get(&it.library_id).and_then(|l| l.dir.clone()),
            paths.get(&it.id),
        ) else {
            out.failed += 1;
            continue;
        };
        let victim = lib_dir.join(tp);
        if !is_inside(&victim, &trash_root(&lib_dir)) {
            out.failed += 1;
            out.first_error
                .get_or_insert("휴지통 밖의 경로입니다".into());
            continue;
        }
        // 이미 사라진 파일은 성공으로 친다 — 목표는 "없는 상태"다
        match std::fs::remove_file(&victim) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                out.failed += 1;
                out.first_error.get_or_insert(e.to_string());
                continue;
            }
        }
        remove_sidecars(&victim);
        db.write(|c| c.execute("DELETE FROM files WHERE id = ?1", [it.id]))?;
        out.moved += 1;
        out.bytes += it.size;
    }
    super::close_batch(db, batch_id, out.moved)?;
    // 파일 행이 하나도 안 남은 폴더 행은 이제 치운다 — 디스크의 폴더는 치울 때 이미 지웠다
    let folders: std::collections::BTreeSet<i64> = items.iter().map(|i| i.folder_id).collect();
    for f in folders {
        out.folders_removed += db.write(|c| {
            c.execute(
                "DELETE FROM folders WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM files WHERE folder_id = ?1)",
                [f],
            )
        })?;
    }
    Ok(out)
}

/// 정규화한 뒤에도 `root` 안에 있는가. 없는 경로는 부모까지 올라가 확인한다.
fn is_inside(path: &Path, root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let real = path.canonicalize().or_else(|_| {
        path.parent()
            .map(Path::canonicalize)
            .unwrap_or_else(|| path.canonicalize())
    });
    real.map(|p| p.starts_with(&root)).unwrap_or(false)
}

/// 휴지통에 든 것들의 개수와 용량.
pub fn summary(db: &Db, library_id: Option<i64>) -> Result<Summary> {
    db.read(|c| {
        c.query_row(
            "SELECT COUNT(*), COALESCE(SUM(fi.size),0)
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.trashed_at IS NOT NULL AND (?1 IS NULL OR fo.library_id = ?1)",
            [library_id],
            |r| {
                Ok(Summary {
                    files: r.get(0)?,
                    bytes: r.get(1)?,
                })
            },
        )
    })
}

/// 라이브러리 하나의 휴지통 집계 — 휴지통은 라이브러리마다 따로 있다(같은 디스크 안
/// `.acut/휴지통`). 한 라이브러리 것만 보여 주면 다른 쪽을 빠뜨린다 (2026-08-30 지적).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LibrarySummary {
    pub library_id: i64,
    pub name: String,
    pub files: i64,
    pub bytes: i64,
}

/// 모든 라이브러리의 휴지통을 한눈에 — 빈 것도 0으로 나온다
pub fn summary_by_library(db: &Db) -> Result<Vec<LibrarySummary>> {
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT l.id, l.name,
                    (SELECT COUNT(*) FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                      WHERE fo.library_id = l.id AND fi.trashed_at IS NOT NULL),
                    (SELECT COALESCE(SUM(fi.size),0) FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                      WHERE fo.library_id = l.id AND fi.trashed_at IS NOT NULL)
             FROM libraries l ORDER BY l.name",
        )?;
        let it = st.query_map([], |r| {
            Ok(LibrarySummary { library_id: r.get(0)?, name: r.get(1)?, files: r.get(2)?, bytes: r.get(3)? })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 제외로 판정했지만 아직 치우지 않은 것들의 id.
pub fn pending(db: &Db, library_id: Option<i64>) -> Result<Vec<i64>> {
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.culling_flag = 2 AND fi.trashed_at IS NULL
               AND (?1 IS NULL OR fo.library_id = ?1)",
        )?;
        let it = st.query_map([library_id], |r| r.get(0))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 이 폴더들 안에서 제외 표시된 것 — 비교 화면이 «방금 표시한 것만» 치울 때
pub fn pending_in_folders(db: &Db, folder_ids: &[i64]) -> Result<Vec<i64>> {
    if folder_ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = folder_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    db.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT id FROM files WHERE culling_flag = 2 AND trashed_at IS NULL AND folder_id IN ({list})"
        ))?;
        let it = st.query_map([], |r| r.get(0))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_test;

    /// 사진 몇 장을 만들고 스캔한다. 라이브러리 폴더를 그대로 돌려준다.
    fn setup() -> (tempfile::TempDir, Db, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("2020").join("여행");
        std::fs::create_dir_all(&a).unwrap();
        for n in [
            "20200101_120000.jpg",
            "20200101_120001.jpg",
            "20200101_120002.jpg",
        ] {
            std::fs::write(a.join(n), b"photo bytes ".repeat(10)).unwrap();
        }
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 1, |_| {}).unwrap();
        let ids: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare("SELECT id FROM files ORDER BY name")?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        assert_eq!(ids.len(), 3);
        (dir, db, ids)
    }

    #[test]
    fn emptied_folders_leave_the_disk_and_come_back_on_restore() {
        let (dir, db, ids) = setup();
        let a = dir.path().join("2020/여행");
        std::fs::write(a.join(".DS_Store"), b"").unwrap(); // Finder 찌꺼기는 빈 폴더로 친다
        let out = to_trash(&db, &ids, "치우기").unwrap();
        assert_eq!(out.moved, 3);
        assert!(!a.exists(), "사진이 다 나간 폴더는 디스크에서 사라진다");
        assert!(!dir.path().join("2020").exists(), "그래서 빈 위 폴더도");
        assert!(dir.path().is_dir(), "라이브러리 뿌리는 남는다");
        assert_eq!(out.folders_removed, 2);
        let rows: i64 = db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM folders WHERE rel_path LIKE '%여행'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(rows, 1, "폴더 행은 남는다 — 휴지통 파일이 가리킨다");

        restore(&db, &ids[..1]).unwrap();
        assert!(
            a.join("20200101_120000.jpg").is_file(),
            "되돌리면 폴더가 되살아난다"
        );

        // 나머지 둘을 비우면 — 폴더엔 아직 한 장이 있으니 행은 남는다
        empty(&db, &ids[1..]).unwrap();
        let rows: i64 = db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM folders WHERE rel_path LIKE '%여행'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(rows, 1);
    }

    /// 되돌릴 이름의 행이 이미 있으면(파일은 없고 행만 남은 상태) 디스크만 돌아오면 안 된다
    #[test]
    fn restore_puts_the_file_back_in_the_trash_when_the_row_update_fails() {
        let (dir, db, ids) = setup();
        let a = dir.path().join("2020/여행");
        let out = to_trash(&db, &ids[..1], "치우기").unwrap();
        assert_eq!(out.moved, 1);
        let trash_path: String = db
            .read(|c| {
                c.query_row("SELECT trash_path FROM files WHERE id=?1", [ids[0]], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        // 그새 같은 이름이 디스크에 생겨 «(2)»로 돌아와야 하는데, 그 «(2)» 이름은
        // 파일 없이 행만 차지하고 있다 — UNIQUE(folder_id, name) 이 막는다
        std::fs::write(a.join("20200101_120000.jpg"), b"replacement").unwrap();
        db.write(|c| {
            c.execute(
                "UPDATE files SET name='20200101_120000 (2).jpg' WHERE id=?1",
                [ids[1]],
            )
        })
        .unwrap();

        let out = restore(&db, &ids[..1]).unwrap();
        assert_eq!((out.moved, out.failed), (0, 1), "{:?}", out.first_error);
        assert_eq!(out.failed_ids, vec![ids[0]]);
        assert!(
            !a.join("20200101_120000 (2).jpg").exists(),
            "디스크만 되돌아오면 안 된다"
        );
        assert!(
            dir.path().join(&trash_path).is_file(),
            "파일은 휴지통 자리로 돌아간다"
        );
        let trashed: Option<i64> = db
            .read(|c| {
                c.query_row("SELECT trashed_at FROM files WHERE id=?1", [ids[0]], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert!(trashed.is_some(), "행은 여전히 휴지통이다");
    }

    #[test]
    fn every_library_reports_its_own_trash() {
        let (_d, db, ids) = setup();
        let before = summary_by_library(&db).unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(
            (before[0].files, before[0].bytes),
            (0, 0),
            "비어도 줄은 나온다"
        );
        to_trash(&db, &ids, "휴지통으로").unwrap();
        let after = summary_by_library(&db).unwrap();
        assert_eq!(after[0].files, 3);
        assert_eq!(after[0].bytes, 3 * 120);
        restore(&db, &ids[..1]).unwrap();
        assert_eq!(summary_by_library(&db).unwrap()[0].files, 2);
    }

    #[test]
    fn a_folder_with_other_files_is_not_removed() {
        let (dir, db, ids) = setup();
        let a = dir.path().join("2020/여행");
        std::fs::write(a.join("메모.txt"), b"keep me").unwrap();
        to_trash(&db, &ids, "치우기").unwrap();
        assert!(
            a.join("메모.txt").is_file(),
            "사진이 아닌 파일이 있으면 폴더를 두어야 한다"
        );
    }

    #[test]
    fn pending_in_folders_scopes_to_the_given_folders() {
        let (_d, db, ids) = setup();
        db.write(|c| c.execute("UPDATE files SET culling_flag = 2", []))
            .unwrap();
        let fid: i64 = db
            .read(|c| {
                c.query_row("SELECT folder_id FROM files WHERE id = ?1", [ids[0]], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert_eq!(pending_in_folders(&db, &[fid]).unwrap().len(), 3);
        assert!(pending_in_folders(&db, &[fid + 1000]).unwrap().is_empty());
        assert!(pending_in_folders(&db, &[]).unwrap().is_empty());
    }

    #[test]
    fn sidecar_follows_the_photo_to_the_trash_and_back() {
        let (dir, db, ids) = setup();
        let a = dir.path().join("2020/여행");
        // 줄기 사이드카와 전체 이름 사이드카 둘 다
        std::fs::write(a.join("20200101_120000.xmp"), b"<xmp/>").unwrap();
        std::fs::write(a.join("20200101_120001.jpg.xmp"), b"<xmp/>").unwrap();
        let out = to_trash(&db, &ids[..2], "치우기").unwrap();
        assert_eq!(out.moved, 2);
        assert!(!a.join("20200101_120000.xmp").exists(), "사이드카도 떠난다");
        assert!(!a.join("20200101_120001.jpg.xmp").exists());
        let t = trash_root(dir.path());
        assert!(
            t.join("2020/여행/20200101_120000.xmp").is_file(),
            "휴지통에 같이 있다"
        );
        assert!(t.join("2020/여행/20200101_120001.jpg.xmp").is_file());

        restore(&db, &ids[..2]).unwrap();
        assert!(
            a.join("20200101_120000.xmp").is_file(),
            "되돌리면 같이 돌아온다"
        );
        assert!(a.join("20200101_120001.jpg.xmp").is_file());
        assert!(!t.join("2020/여행/20200101_120000.xmp").exists());
    }

    #[test]
    fn emptying_the_trash_removes_sidecars_too() {
        let (dir, db, ids) = setup();
        let a = dir.path().join("2020/여행");
        std::fs::write(a.join("20200101_120000.xmp"), b"<xmp/>").unwrap();
        to_trash(&db, &ids[..1], "치우기").unwrap();
        let t = trash_root(dir.path());
        assert!(t.join("2020/여행/20200101_120000.xmp").is_file());
        empty(&db, &ids[..1]).unwrap();
        assert!(
            !t.join("2020/여행/20200101_120000.xmp").exists(),
            "사진과 함께 지워진다"
        );
    }

    #[test]
    fn sidecar_pairs_are_named_after_the_destination() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("IMG_1.xmp"), b"").unwrap();
        std::fs::write(d.path().join("IMG_1.CR2.xmp"), b"").unwrap();
        let from = d.path().join("IMG_1.CR2");
        let to = d.path().join("out").join("IMG_1 (2).CR2");
        let mut got: Vec<String> = sidecars(&from, &to)
            .into_iter()
            .map(|(_, b)| b.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        got.sort();
        assert_eq!(got, ["IMG_1 (2).CR2.xmp", "IMG_1 (2).xmp"]);
        assert!(
            sidecars(&d.path().join("none.jpg"), &to).is_empty(),
            "없으면 없다"
        );
    }

    #[test]
    fn free_path_never_returns_an_existing_path() {
        let d = tempfile::tempdir().unwrap();
        let want = d.path().join("a.jpg");
        std::fs::write(&want, b"").unwrap();
        let p = free_path(want.clone());
        assert_ne!(p, want);
        assert!(!p.exists());
        assert!(p.file_name().unwrap().to_string_lossy().starts_with("a ("));
    }

    fn alive(db: &Db) -> i64 {
        db.read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM files WHERE trashed_at IS NULL",
                [],
                |r| r.get(0),
            )
        })
        .unwrap()
    }

    #[test]
    fn moves_the_file_and_keeps_the_row() {
        let (dir, db, ids) = setup();
        let src = dir.path().join("2020/여행/20200101_120000.jpg");
        assert!(src.is_file());

        let out = to_trash(&db, &ids[..1], "시험").unwrap();
        assert_eq!((out.moved, out.failed), (1, 0));
        assert!(!src.exists(), "원래 자리에서는 사라져야 한다");
        assert!(
            trash_root(dir.path())
                .join("2020/여행/20200101_120000.jpg")
                .is_file(),
            "휴지통에 폴더 구조 그대로 들어간다"
        );
        // 행은 남아 있다 — 평점·판정을 잃지 않기 위해서다
        assert_eq!(alive(&db), 2);
        let n: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(n, 3, "행을 지우지 않는다");
    }

    #[test]
    fn restore_puts_it_back_with_its_rating() {
        let (dir, db, ids) = setup();
        db.write(|c| c.execute("UPDATE files SET rating=4 WHERE id=?1", [ids[0]]))
            .unwrap();

        to_trash(&db, &ids[..1], "시험").unwrap();
        let out = restore(&db, &ids[..1]).unwrap();
        assert_eq!((out.moved, out.failed), (1, 0));

        let src = dir.path().join("2020/여행/20200101_120000.jpg");
        assert!(src.is_file(), "제자리로 돌아온다");
        assert_eq!(alive(&db), 3);

        let rating: i32 = db
            .read(|c| {
                c.query_row("SELECT rating FROM files WHERE id=?1", [ids[0]], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert_eq!(rating, 4, "판정이 살아 있어야 되돌리기가 의미 있다");
    }

    #[test]
    fn same_name_from_different_folders_does_not_overwrite() {
        let (dir, db, _) = setup();
        // 다른 폴더에 같은 이름을 하나 더 만든다
        let b = dir.path().join("2021").join("여행");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("20200101_120000.jpg"), b"different").unwrap();
        scan_test(&db, dir.path(), 1, |_| {}).ok();

        let ids: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare("SELECT id FROM files WHERE name='20200101_120000.jpg'")?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        assert_eq!(ids.len(), 2);

        let out = to_trash(&db, &ids, "시험").unwrap();
        assert_eq!(out.moved, 2, "둘 다 옮겨져야 한다");
        // 폴더 구조를 그대로 쓰므로 애초에 부딪히지 않는다
        assert!(trash_root(dir.path())
            .join("2020/여행/20200101_120000.jpg")
            .is_file());
        assert!(trash_root(dir.path())
            .join("2021/여행/20200101_120000.jpg")
            .is_file());
    }

    #[test]
    fn empty_deletes_only_inside_the_trash() {
        let (dir, db, ids) = setup();
        to_trash(&db, &ids[..2], "시험").unwrap();

        // 휴지통 밖을 가리키게 조작해 본다 — 안전장치가 막아야 한다
        db.write(|c| {
            c.execute(
                "UPDATE files SET trash_path='2020/여행/20200101_120002.jpg' WHERE id=?1",
                [ids[0]],
            )
        })
        .unwrap();

        let out = empty(&db, &ids[..2]).unwrap();
        assert_eq!(out.failed, 1, "휴지통 밖은 거부한다");
        assert_eq!(out.moved, 1);
        assert!(
            dir.path().join("2020/여행/20200101_120002.jpg").is_file(),
            "밖에 있는 파일은 그대로 있어야 한다"
        );
    }

    #[test]
    fn empty_removes_the_row_for_real() {
        let (_d, db, ids) = setup();
        to_trash(&db, &ids[..1], "시험").unwrap();
        empty(&db, &ids[..1]).unwrap();
        let n: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(n, 2, "비우면 그때 행도 사라진다");
    }

    #[test]
    fn summary_and_pending() {
        let (_d, db, ids) = setup();
        db.write(|c| {
            c.execute(
                "UPDATE files SET culling_flag=2 WHERE id IN (?1,?2)",
                [ids[0], ids[1]],
            )
        })
        .unwrap();
        assert_eq!(pending(&db, None).unwrap().len(), 2);

        to_trash(&db, &ids[..2], "시험").unwrap();
        assert!(
            pending(&db, None).unwrap().is_empty(),
            "치운 것은 대기가 아니다"
        );
        let s = summary(&db, None).unwrap();
        assert_eq!(s.files, 2);
        assert!(s.bytes > 0);
    }

    #[test]
    fn everything_is_written_to_the_journal() {
        let (_d, db, ids) = setup();
        let out = to_trash(&db, &ids, "시험").unwrap();
        let (n, batch_kind): (i64, String) = db
            .read(|c| {
                c.query_row(
                    "SELECT (SELECT COUNT(*) FROM journal WHERE batch_id=?1), kind
                     FROM batches WHERE id=?1",
                    [out.batch_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(n, 3, "옮긴 것마다 한 줄씩");
        assert_eq!(batch_kind, "trash");
    }

    #[test]
    fn trashing_twice_is_harmless() {
        let (_d, db, ids) = setup();
        to_trash(&db, &ids[..1], "시험").unwrap();
        // 이미 휴지통에 있는 것은 대상에서 빠진다
        let again = to_trash(&db, &ids[..1], "시험").unwrap();
        assert_eq!((again.moved, again.failed), (0, 0));
    }

    #[test]
    fn restore_records_the_renamed_file_when_the_slot_is_taken() {
        let (dir, db, ids) = setup();
        to_trash(&db, &ids[..1], "시험").unwrap();
        // 그새 같은 이름의 새 파일이 제자리에 생겼다
        let slot = dir.path().join("2020/여행/20200101_120000.jpg");
        std::fs::write(&slot, b"NEW").unwrap();

        let out = restore(&db, &ids[..1]).unwrap();
        assert_eq!((out.moved, out.failed), (1, 0));
        assert_eq!(std::fs::read(&slot).unwrap(), b"NEW", "새 파일은 그대로");
        let name: String = db
            .read(|c| c.query_row("SELECT name FROM files WHERE id=?1", [ids[0]], |r| r.get(0)))
            .unwrap();
        assert_eq!(
            name, "20200101_120000 (2).jpg",
            "행이 실제 파일 이름을 가리킨다"
        );
        assert!(dir.path().join("2020/여행").join(&name).is_file());
    }
}
