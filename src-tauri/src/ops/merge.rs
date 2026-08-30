//! 폴더 합치기 — 한 폴더 나무(B)를 같은 라이브러리의 다른 나무(A) 안으로.
//!
//! B 아래 사진마다 A 의 같은 자리(하위 경로 그대로)로 옮긴다. 같은 이름이 있으면 «이름 (2)».
//! 덮어쓰지 않는다. 옮기고 비는 B 폴더는 디스크에서 지운다. 저널을 남겨 ⌘Z 로 되돌린다.
//! 사용 흐름: 두 폴더 비교에서 사본을 다 뺀 뒤 «B 폴더를 A 로 합치기».

use crate::db::conn::{Db, Result};
use crate::db::libraries;
use crate::ops::trash::{free_path, move_with_sidecars, prune_empty_dirs, Outcome};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MergeProgress {
    pub done: usize,
    pub total: usize,
}

struct Item {
    id: i64,
    /// 볼륨 기준 폴더 경로
    dir: String,
    name: String,
    size: i64,
}

fn bad(msg: impl Into<String>) -> crate::db::conn::DbError {
    crate::db::conn::DbError::Invalid(msg.into())
}

fn under(root: &str, p: &str) -> bool {
    root.is_empty() || p == root || p.starts_with(&format!("{root}/"))
}

/// `src_rel` 나무를 `dst_rel` 안으로 합친다 — 둘 다 볼륨 기준 경로, 같은 라이브러리 안.
pub fn merge_tree(
    db: &Db,
    library_id: i64,
    src_rel: &str,
    dst_rel: &str,
    cancel: &AtomicBool,
    on_progress: impl Fn(&MergeProgress),
) -> Result<Outcome> {
    let lib = libraries::get(db, library_id)?.ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    let lib_dir = lib.dir.clone().ok_or_else(|| bad("디스크가 연결되어 있지 않습니다"))?;
    let mount = crate::db::volumes::find_mount(&lib.volume_uuid).ok_or_else(|| bad("디스크가 연결되어 있지 않습니다"))?;
    if src_rel == dst_rel || under(src_rel, dst_rel) || under(dst_rel, src_rel) {
        return Err(bad("두 폴더가 서로를 품고 있습니다 — 겹치지 않는 두 폴더여야 합니다"));
    }
    if !under(&lib.rel_path, src_rel) || !under(&lib.rel_path, dst_rel) || src_rel == lib.rel_path {
        return Err(bad("같은 라이브러리 안의 폴더끼리만 합칠 수 있습니다"));
    }
    if !mount.join(dst_rel).is_dir() {
        return Err(bad(format!("합쳐 넣을 폴더가 디스크에 없습니다: {dst_rel}")));
    }

    let items: Vec<Item> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.rel_path, fi.name, fi.size FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fo.volume_uuid = ?1 AND (fo.rel_path = ?2 OR substr(fo.rel_path, 1, length(?2) + 1) = ?2 || '/')
               AND fi.trashed_at IS NULL
             ORDER BY fo.rel_path, fi.name",
        )?;
        let it = st.query_map(rusqlite::params![lib.volume_uuid, src_rel], |r| {
            Ok(Item { id: r.get(0)?, dir: r.get(1)?, name: r.get(2)?, size: r.get(3)? })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let total = items.len();
    if total == 0 {
        return Ok(Outcome { first_error: Some("합칠 사진이 없습니다".into()), ..Default::default() });
    }
    let tail = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let batch_id = super::open_batch(db, "move", &format!("폴더 합치기 «{}» → «{}»", tail(src_rel), tail(dst_rel)))?;
    let mut out = Outcome { batch_id, ..Default::default() };
    let mut src_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let mut folder_cache: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    for (i, it) in items.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            out.first_error.get_or_insert("멈췄습니다 — 옮긴 것은 그대로, ⌘Z 로 되돌릴 수 있습니다".into());
            break;
        }
        let sub = it.dir.strip_prefix(src_rel).map(|s| s.trim_start_matches('/')).unwrap_or("");
        let dest_dir_rel = if sub.is_empty() { dst_rel.to_string() } else { format!("{dst_rel}/{sub}") };
        let folder_id = match folder_cache.get(&dest_dir_rel) {
            Some(&id) => id,
            None => {
                let id = ensure_folder(db, &lib.volume_uuid, library_id, &dest_dir_rel, lib.area)?;
                folder_cache.insert(dest_dir_rel.clone(), id);
                id
            }
        };
        let src_path = mount.join(&it.dir).join(&it.name);
        // 이름은 디스크에도 DB 에도 비어 있어야 한다 — 휴지통에 간 파일의 행이 그 이름을 쥐고 있으면
        // UNIQUE(folder_id, name) 에 걸려 합치기가 중간에 멈춘다 (실측 2026-08-30: 17,067장에서 멈춤)
        let dest_path = free_name(db, folder_id, free_path(mount.join(&dest_dir_rel).join(&it.name)))?;
        let new_name = dest_path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| it.name.clone());
        let from_vol_rel = format!("{}/{}", it.dir, it.name);
        let to_vol_rel = format!("{dest_dir_rel}/{new_name}");
        match move_with_sidecars(&src_path, &dest_path) {
            Ok(()) => {
                super::record(db, batch_id, "move", it.id, &lib.volume_uuid, &from_vol_rel, Some(&to_vol_rel), Ok(()))?;
                // 행 갱신이 실패해도 합치기를 통째로 멈추지 않는다 — 파일은 이미 옮겨졌으니 세어 알리고
                // 계속 간다. 남은 어긋남은 다시 스캔이 맞춘다
                let upd = db.write(|c| {
                    c.execute(
                        "UPDATE files SET folder_id = ?2, name = ?3 WHERE id = ?1",
                        rusqlite::params![it.id, folder_id, new_name],
                    )
                });
                match upd {
                    Ok(_) => {
                        out.moved += 1;
                        out.bytes += it.size;
                    }
                    Err(e) => {
                        log::warn!("합치기: 파일은 옮겼는데 행 갱신 실패 {to_vol_rel}: {e}");
                        out.failed += 1;
                        out.first_error.get_or_insert(format!("옮긴 뒤 기록 실패 — 다시 스캔으로 맞춰집니다: {e}"));
                    }
                }
                src_dirs.insert(mount.join(&it.dir));
            }
            Err(e) => {
                let msg = e.to_string();
                super::record(db, batch_id, "move", it.id, &lib.volume_uuid, &from_vol_rel, None, Err(&msg))?;
                out.failed += 1;
                out.first_error.get_or_insert(msg);
            }
        }
        if i % 25 == 0 || i + 1 == total {
            on_progress(&MergeProgress { done: i + 1, total });
        }
    }
    super::close_batch(db, batch_id, out.moved)?;

    // 비는 B 폴더는 디스크에서 지운다(깊은 것부터). 라이브러리 뿌리는 남긴다
    for d in src_dirs.iter().rev() {
        out.folders_removed += prune_empty_dirs(d, &lib_dir);
    }
    db.write(|c| {
        c.execute(
            "DELETE FROM folders WHERE library_id = ?1 AND NOT EXISTS (SELECT 1 FROM files WHERE files.folder_id = folders.id)",
            [library_id],
        )?;
        c.execute(
            "UPDATE folders SET file_count = (SELECT COUNT(*) FROM files WHERE files.folder_id = folders.id AND files.trashed_at IS NULL)
             WHERE library_id = ?1",
            [library_id],
        )
    })?;
    Ok(out)
}

/// DB 의 그 폴더 행에도 없는 이름으로 — «이름 (n).ext». 디스크 기준 빈 이름에서 시작한다
fn free_name(db: &Db, folder_id: i64, want: PathBuf) -> Result<PathBuf> {
    let taken = |name: &str| -> Result<bool> {
        db.read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM files WHERE folder_id = ?1 AND name = ?2",
                rusqlite::params![folder_id, name],
                |r| r.get::<_, i64>(0),
            )
        })
        .map(|n| n > 0)
    };
    let name = want.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if !taken(&name)? {
        return Ok(want);
    }
    let dir = want.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), Some(e.to_string())),
        None => (name.clone(), None),
    };
    for n in 2..10_000 {
        let cand = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let p = dir.join(&cand);
        if !p.exists() && !taken(&cand)? {
            return Ok(p);
        }
    }
    Ok(want)
}

/// 목적지 폴더 행 — 없으면 만든다 (organize::ensure_folder 와 같은 규칙)
fn ensure_folder(db: &Db, volume_uuid: &str, library_id: i64, vol_rel_dir: &str, area: i32) -> Result<i64> {
    let name = vol_rel_dir.rsplit('/').next().unwrap_or(vol_rel_dir).to_string();
    db.write(|c| {
        c.execute(
            "INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at)
             VALUES(?1,?2,?3,?4,?5,strftime('%s','now'))
             ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET library_id=excluded.library_id",
            rusqlite::params![volume_uuid, library_id, vol_rel_dir, name, area],
        )
    })?;
    db.read(|c| {
        c.query_row(
            "SELECT id FROM folders WHERE volume_uuid=?1 AND rel_path=?2",
            rusqlite::params![volume_uuid, vol_rel_dir],
            |r| r.get(0),
        )
    })
}

/// 합치고 남은 것 — 사진이 아닌 파일(Lightroom 미리보기·txt·Thumbs.db…)은 앱이 모르니 옮기지 않는다.
/// 그래서 Finder 엔 폴더 구조가 남는다 (실측 2026-08-30: 5,928개 3.7GB). 무엇이 남았는지 세어 보여 준다
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Leftovers {
    pub files: usize,
    pub bytes: u64,
    /// 확장자별 개수, 많은 것부터
    pub kinds: Vec<(String, usize)>,
}

fn is_junk_file(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._") || name == "Thumbs.db" || name == "desktop.ini"
}

pub fn leftovers(db: &Db, library_id: i64, rel: &str) -> Result<Leftovers> {
    let lib = libraries::get(db, library_id)?.ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    let mount = crate::db::volumes::find_mount(&lib.volume_uuid).ok_or_else(|| bad("디스크가 연결되어 있지 않습니다"))?;
    let root = mount.join(rel);
    let mut out = Leftovers::default();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut kinds: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for e in walkdir::WalkDir::new(&root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if !e.file_type().is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if is_junk_file(&name) {
            continue;
        }
        out.files += 1;
        out.bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
        let ext = name.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_else(|| "(없음)".into());
        *kinds.entry(ext).or_default() += 1;
    }
    let mut v: Vec<(String, usize)> = kinds.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(8);
    out.kinds = v;
    Ok(out)
}

/// 남은 파일도 같은 자리로 — 사진이 아니라 DB 에는 안 적는다. 같은 이름은 (2). 찌꺼기(.DS_Store 등)는 지우고
/// 비는 폴더는 지운다. (옮긴 수, 실패 수, 지운 폴더 수)
pub fn merge_rest(db: &Db, library_id: i64, src_rel: &str, dst_rel: &str) -> Result<Outcome> {
    let lib = libraries::get(db, library_id)?.ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    let lib_dir = lib.dir.clone().ok_or_else(|| bad("디스크가 연결되어 있지 않습니다"))?;
    let mount = crate::db::volumes::find_mount(&lib.volume_uuid).ok_or_else(|| bad("디스크가 연결되어 있지 않습니다"))?;
    if src_rel == dst_rel || under(src_rel, dst_rel) || under(dst_rel, src_rel) || src_rel == lib.rel_path {
        return Err(bad("두 폴더가 서로를 품고 있습니다 — 겹치지 않는 두 폴더여야 합니다"));
    }
    let src = mount.join(src_rel);
    let dst = mount.join(dst_rel);
    if !src.is_dir() {
        return Ok(Outcome { first_error: Some("남은 폴더가 없습니다".into()), ..Default::default() });
    }
    let mut out = Outcome::default();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for e in walkdir::WalkDir::new(&src).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if e.file_type().is_dir() {
            dirs.push(e.path().to_path_buf());
            continue;
        }
        if !e.file_type().is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if is_junk_file(&name) {
            let _ = std::fs::remove_file(e.path());
            continue;
        }
        let Ok(rel) = e.path().strip_prefix(&src) else { continue };
        let dest = free_path(dst.join(rel));
        match crate::ops::trash::move_file(e.path(), &dest) {
            Ok(()) => {
                out.moved += 1;
                out.bytes += e.metadata().map(|m| m.len() as i64).unwrap_or(0);
            }
            Err(err) => {
                out.failed += 1;
                out.first_error.get_or_insert(format!("{}: {err}", e.path().display()));
            }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        out.folders_removed += prune_empty_dirs(&d, &lib_dir);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::undo;
    use crate::scan::scan_test;

    fn setup() -> (tempfile::TempDir, Db, i64, String, String, String) {
        let dir = tempfile::tempdir().unwrap();
        for (d, files) in [
            ("B/2016", vec![("a.jpg", "AAAA"), ("only-b.jpg", "BBBB")]),
            ("B/2016/x", vec![("deep.jpg", "DDDD")]),
            ("A/2016", vec![("a.jpg", "other content")]),
            ("A/2017", vec![("c.jpg", "CCCC")]),
        ] {
            let p = dir.path().join(d);
            std::fs::create_dir_all(&p).unwrap();
            for (n, body) in files {
                std::fs::write(p.join(n), body.as_bytes().repeat(30)).unwrap();
            }
        }
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        let (lib, lib_rel): (i64, String) = db
            .read(|c| c.query_row("SELECT id, rel_path FROM libraries", [], |r| Ok((r.get(0)?, r.get(1)?))))
            .unwrap();
        let j = |s: &str| if lib_rel.is_empty() { s.to_string() } else { format!("{lib_rel}/{s}") };
        (dir, db, lib, j("B"), j("A"), lib_rel)
    }

    #[test]
    fn merging_moves_photos_into_matching_folders_and_renames_collisions() {
        let (dir, db, lib, b, a, _) = setup();
        let out = merge_tree(&db, lib, &b, &a, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!((out.moved, out.failed), (3, 0), "{out:?}");
        assert!(dir.path().join("A/2016/a (2).jpg").is_file(), "같은 이름은 (2) — 덮어쓰지 않는다");
        assert!(dir.path().join("A/2016/a.jpg").is_file(), "A 의 원래 사진은 그대로");
        assert!(dir.path().join("A/2016/only-b.jpg").is_file());
        assert!(dir.path().join("A/2016/x/deep.jpg").is_file(), "하위 폴더도 같은 자리로");
        assert!(!dir.path().join("B").exists(), "비어 버린 B 는 디스크에서 사라진다");
        let b_rows: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM folders WHERE rel_path LIKE '%/B%' OR rel_path = 'B'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(b_rows, 0, "B 폴더 행도 없다");
        let named: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files WHERE name = 'a (2).jpg'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(named, 1, "DB 도 새 이름을 안다");

        // ⌘Z — 전부 B 로 돌아온다
        let u = undo::undo(&db, out.batch_id).unwrap();
        assert_eq!((u.moved, u.failed), (3, 0), "{u:?}");
        assert!(dir.path().join("B/2016/a.jpg").is_file());
        assert!(dir.path().join("B/2016/x/deep.jpg").is_file());
        assert!(!dir.path().join("A/2016/a (2).jpg").exists());
    }

    /// 휴지통에 간 파일의 행이 목적지에서 같은 이름을 쥐고 있어도 멈추지 않는다
    #[test]
    fn a_trashed_row_holding_the_name_does_not_abort_the_merge() {
        let (dir, db, lib, b, a, _) = setup();
        // A/2016/only-b.jpg 라는 «휴지통 행»을 만든다 — 디스크엔 없고 DB 에만 있다
        let a2016: i64 = db
            .read(|c| c.query_row("SELECT id FROM folders WHERE rel_path LIKE '%A/2016'", [], |r| r.get(0)))
            .unwrap();
        // 기존 행(A/2017/c.jpg)을 «A/2016 의 only-b.jpg 휴지통 행»으로 바꿔 둔다 — NOT NULL 열을 다 채울 필요 없이
        db.write(|c| {
            c.execute(
                "UPDATE files SET folder_id = ?1, name = 'only-b.jpg', trashed_at = 1, trash_path = 'x' WHERE name = 'c.jpg'",
                [a2016],
            )
        })
        .unwrap();
        let out = merge_tree(&db, lib, &b, &a, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!((out.moved, out.failed), (3, 0), "{out:?}");
        assert!(dir.path().join("A/2016/only-b (2).jpg").is_file(), "DB 에서도 빈 이름으로: {:?}", std::fs::read_dir(dir.path().join("A/2016")).unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>());
    }

    /// 사진이 아닌 파일이 남아 폴더가 남았을 때 — «남은 파일도 옮기기»가 마저 치운다
    #[test]
    fn leftovers_are_counted_and_merge_rest_moves_them_too() {
        let (dir, db, lib, b, a, _) = setup();
        std::fs::write(dir.path().join("B/2016/노트.txt"), b"memo").unwrap();
        std::fs::write(dir.path().join("B/2016/x/Thumbs.db"), b"junk").unwrap();
        merge_tree(&db, lib, &b, &a, &AtomicBool::new(false), |_| {}).unwrap();
        assert!(dir.path().join("B/2016/노트.txt").is_file(), "사진이 아닌 것은 안 옮겨져 폴더가 남는다");
        let l = leftovers(&db, lib, &b).unwrap();
        assert_eq!(l.files, 1, "{l:?}");
        assert_eq!(l.kinds[0].0, "txt");
        let r = merge_rest(&db, lib, &b, &a).unwrap();
        assert_eq!((r.moved, r.failed), (1, 0), "{r:?}");
        assert!(dir.path().join("A/2016/노트.txt").is_file());
        assert!(!dir.path().join("B").exists(), "찌꺼기(Thumbs.db)는 지우고 빈 폴더도 지운다");
        assert_eq!(leftovers(&db, lib, &b).unwrap().files, 0);
    }

    #[test]
    fn overlapping_or_foreign_roots_are_refused() {
        let (_d, db, lib, b, a, lib_rel) = setup();
        assert!(merge_tree(&db, lib, &b, &b, &AtomicBool::new(false), |_| {}).is_err());
        let inner = format!("{b}/2016");
        assert!(merge_tree(&db, lib, &b, &inner, &AtomicBool::new(false), |_| {}).is_err(), "품는 관계");
        assert!(merge_tree(&db, lib, &lib_rel, &a, &AtomicBool::new(false), |_| {}).is_err(), "라이브러리 뿌리째는 안 된다");
    }
}
