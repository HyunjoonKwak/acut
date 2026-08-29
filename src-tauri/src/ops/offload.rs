//! 다른 디스크로 옮기기(아카이브) — 폴더 한 갈래를 통째로 다른 라이브러리로.
//!
//! 운영 SSD가 차면 오래된 연도를 아카이브 디스크로 보낸다. 옮긴 뒤에도
//! 라이브러리에 그대로 보인다 — 폴더 행이 새 라이브러리를 가리키고 썸네일도
//! 따라간다. 디스크를 빼면 그 라이브러리가 «오프라인»으로 흐려지고, 원본이
//! 필요한 일만 안 된다. 되돌리기는 같은 동작으로 원래 라이브러리를 고르면 된다.
//!
//! 물리적 이동이 먼저, DB는 그다음이다. 같은 볼륨이면 이름만 바꾸고(순간),
//! 다른 볼륨이면 파일마다 복사 → 크기·xxHash 확인 → 원본 삭제. 중간에 멈추면
//! 복사한 만큼 지우고 원본은 그대로 둔다 — 반쪽이 두 군데 남지 않게.

use crate::db::conn::{Db, Result};
use crate::db::libraries;
use crate::media::cache;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use walkdir::WalkDir;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct OffloadProgress {
    pub done: usize,
    pub total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Offloaded {
    pub folders: usize,
    pub files: usize,
    pub bytes: u64,
    /// 사본은 있는데 원본을 못 지운 파일 수 — 0이 아니면 사람이 봐야 한다
    pub undeleted: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FolderSize {
    pub folders: usize,
    pub files: usize,
    pub bytes: u64,
}

struct FolderRow {
    id: i64,
    library_id: i64,
    rel_path: String,
}

fn folder(db: &Db, id: i64) -> Result<Option<FolderRow>> {
    db.read(|c| {
        c.query_row(
            "SELECT id, library_id, rel_path FROM folders WHERE id = ?1",
            [id],
            |r| Ok(FolderRow { id: r.get(0)?, library_id: r.get(1)?, rel_path: r.get(2)? }),
        )
        .map(Some)
        .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
    })
}

/// 이 폴더와 그 아래 폴더 행들 — 같은 라이브러리 안, 볼륨 기준 경로 접두어로
fn subtree(db: &Db, f: &FolderRow) -> Result<Vec<(i64, String)>> {
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT id, rel_path FROM folders
              WHERE library_id = ?1 AND (rel_path = ?2 OR rel_path LIKE ?3 || '/%' ESCAPE '\\')
              ORDER BY rel_path",
        )?;
        // `_`·`%` 가 든 폴더 이름(«2015_여행»)이 «2015년여행»까지 끌어와 재지정하던 길 (리뷰 H14)
        let esc = crate::db::query::escape_like(&f.rel_path);
        let it = st.query_map(rusqlite::params![f.library_id, f.rel_path, esc], |r| Ok((r.get(0)?, r.get(1)?)))?;
        it.collect()
    })
}

pub fn folder_size(db: &Db, folder_id: i64) -> Result<FolderSize> {
    let Some(f) = folder(db, folder_id)? else {
        return Ok(FolderSize { folders: 0, files: 0, bytes: 0 });
    };
    let sub = subtree(db, &f)?;
    let ids = sub.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>().join(",");
    let (files, bytes): (i64, i64) = db.read(|c| {
        c.query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(size),0) FROM files WHERE folder_id IN ({ids}) AND trashed_at IS NULL"),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    })?;
    Ok(FolderSize { folders: sub.len(), files: files as usize, bytes: bytes as u64 })
}

/// 라이브러리 루트 기준 경로 — 볼륨 기준에서 라이브러리 접두어를 뗀다
fn under_library(lib_rel: &str, vol_rel: &str) -> Option<String> {
    if lib_rel.is_empty() {
        return Some(vol_rel.to_string());
    }
    vol_rel.strip_prefix(lib_rel).and_then(|s| s.strip_prefix('/')).map(str::to_string)
}

fn bad(msg: impl Into<String>) -> crate::db::conn::DbError {
    crate::db::conn::DbError::Invalid(msg.into())
}

/// 다른 볼륨으로 — 파일마다 복사·확인, 다 되면 원본 삭제. 멈추면 사본을 지운다.
fn copy_tree(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    on_progress: &(impl Fn(&OffloadProgress) + Sync),
) -> std::io::Result<(usize, u64, usize)> {
    // 디스크가 준 이름은 NFC로 다듬는다 — macOS의 FSKit exFAT은 목록을 NFD로 주면서
    // 찾기는 NFC로만 되어(실측), 목록 그대로 지우면 «없는 파일»이 된다.
    let nfc_path = |p: &Path| -> PathBuf {
        use unicode_normalization::UnicodeNormalization;
        PathBuf::from(p.to_string_lossy().nfc().collect::<String>())
    };
    // 뿌리도 같은 꼴로 — 걸어 온 경로만 NFC면 strip_prefix가 안 맞아 첫 파일에서 죽었다
    let src = &nfc_path(src);
    let dest = &nfc_path(dest);
    let files: Vec<(PathBuf, u64)> = WalkDir::new(src)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let n = e.metadata().map(|m| m.len()).unwrap_or(0);
            (nfc_path(e.path()), n)
        })
        .collect();
    let total = files.len();
    let bytes_total: u64 = files.iter().map(|f| f.1).sum();
    let mut p = OffloadProgress { total, bytes_total, ..Default::default() };
    let mut copied: Vec<PathBuf> = Vec::new();
    let undo = |copied: &[PathBuf]| {
        for c in copied {
            let _ = std::fs::remove_file(c);
        }
        let _ = std::fs::remove_dir_all(dest);
    };
    for (path, size) in &files {
        if cancel.load(Ordering::Relaxed) {
            undo(&copied);
            return Err(std::io::Error::other("멈췄습니다 — 원본은 그대로, 복사한 것은 지웠습니다"));
        }
        let Ok(rel) = path.strip_prefix(src) else {
            undo(&copied);
            return Err(std::io::Error::other(format!(
                "경로를 셈할 수 없습니다: {} — 되돌렸습니다",
                path.display()
            )));
        };
        let to = dest.join(rel);
        // 중간에 실패하면 복사한 것을 전부 걷는다 — `?`로 빠져나가면 반쪽 사본이 남아
        // 다음 시도가 «대상에 같은 폴더가 이미 있습니다»로 막힌다 (리뷰 H2)
        if let Some(parent) = to.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                undo(&copied);
                return Err(std::io::Error::other(format!("폴더를 못 만들었습니다: {} — {e}. 되돌렸습니다", parent.display())));
            }
        }
        let n = match std::fs::copy(path, &to) {
            Ok(n) => n,
            Err(e) => {
                copied.push(to.clone());
                undo(&copied);
                return Err(std::io::Error::other(format!("복사 실패: {} — {e}. 되돌렸습니다", to.display())));
            }
        };
        let same = n == *size
            && crate::core::hasher::xxhash_file(path).is_some()
            && crate::core::hasher::xxhash_file(path) == crate::core::hasher::xxhash_file(&to);
        if !same {
            copied.push(to.clone());
            undo(&copied);
            return Err(std::io::Error::other(format!(
                "복사가 원본과 다릅니다: {} — 디스크가 찼나요? 되돌렸습니다",
                to.display()
            )));
        }
        // 수정 시각을 원본대로 — 촬영일이 없는 파일은 이 값이 날짜가 된다
        crate::ops::trash::copy_mtime(path, &to);
        copied.push(to);
        p.done += 1;
        p.bytes_done += size;
        on_progress(&p);
    }
    // 전부 확인됐다 — 이제 원본을 지운다. 하나가 안 지워져도 멈추지 않는다: 사본은
    // 이미 다 있으니 DB는 새 자리를 가리켜야 한다. 못 지운 것은 세어 알린다 (리뷰 C3)
    let mut undeleted = 0usize;
    for (path, _) in &files {
        if let Err(e) = std::fs::remove_file(path) {
            log::warn!("옮긴 뒤 원본을 못 지웠습니다 {}: {e}", path.display());
            undeleted += 1;
        }
    }
    // 빈 껍데기 폴더 — 깊은 것부터
    let mut dirs: Vec<PathBuf> = WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .map(|e| nfc_path(e.path()))
        .collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(d);
    }
    Ok((total, bytes_total, undeleted))
}

/// 폴더(와 그 아래)를 `dest_library`로 옮긴다. 라이브러리 루트 기준 자리는 그대로.
pub fn move_folder(
    db: &Db,
    cache_base: &Path,
    folder_id: i64,
    dest_library: i64,
    cancel: &AtomicBool,
    on_progress: impl Fn(&OffloadProgress) + Sync,
) -> Result<Offloaded> {
    let f = folder(db, folder_id)?.ok_or_else(|| bad("없는 폴더입니다"))?;
    let src_lib = libraries::get(db, f.library_id)?.ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    let dst_lib = libraries::get(db, dest_library)?.ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    if src_lib.id == dst_lib.id {
        return Err(bad("같은 라이브러리입니다"));
    }
    let (Some(src_root), Some(dst_root)) = (src_lib.dir.clone(), dst_lib.dir.clone()) else {
        return Err(bad("디스크가 연결되어 있지 않습니다"));
    };
    let sub = under_library(&src_lib.rel_path, &f.rel_path)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad("라이브러리 자체는 옮길 수 없습니다 — 안의 폴더를 고르세요"))?;
    let src_dir = src_root.join(&sub);
    let dst_dir = dst_root.join(&sub);
    if !src_dir.is_dir() {
        return Err(bad(format!("폴더가 없습니다: {}", src_dir.display())));
    }
    if dst_dir.exists() {
        return Err(bad(format!("대상에 같은 폴더가 이미 있습니다: {}", dst_dir.display())));
    }
    let rows = subtree(db, &f)?;
    // 대상 라이브러리에 같은 자리의 폴더 행이 있으면 섞이지 않게 거절한다
    let new_rel = |vol_rel: &str| -> String {
        let s = under_library(&src_lib.rel_path, vol_rel).unwrap_or_default();
        cache::rel_path(&dst_lib.rel_path, &s)
    };
    for (_, rel) in &rows {
        let target = new_rel(rel);
        let taken: i64 = db.read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM folders WHERE library_id = ?1 AND rel_path = ?2",
                rusqlite::params![dst_lib.id, target],
                |r| r.get(0),
            )
        })?;
        if taken > 0 {
            return Err(bad(format!("대상 라이브러리에 「{target}」 폴더 행이 이미 있습니다. 그쪽을 먼저 다시 스캔하거나 정리하세요.")));
        }
    }
    let ids = rows.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>().join(",");
    let thumbs: Vec<String> = db.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT t.rel_path FROM thumbs t JOIN files fi ON fi.id = t.file_id
              WHERE fi.folder_id IN ({ids}) AND t.state = 1 AND t.rel_path IS NOT NULL"
        ))?;
        let it = st.query_map([], |r| r.get(0))?;
        it.collect()
    })?;
    let (files, bytes): (i64, i64) = db.read(|c| {
        c.query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(size),0) FROM files WHERE folder_id IN ({ids})"),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    })?;

    // 1) 물리적 이동 — 같은 볼륨이면 이름만
    if let Some(parent) = dst_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| bad(e.to_string()))?;
    }
    let mut undeleted = 0usize;
    if src_lib.volume_uuid == dst_lib.volume_uuid {
        std::fs::rename(&src_dir, &dst_dir).map_err(|e| bad(format!("옮기기 실패: {e}")))?;
        on_progress(&OffloadProgress { done: files as usize, total: files as usize, bytes_done: bytes as u64, bytes_total: bytes as u64 });
    } else {
        undeleted = copy_tree(&src_dir, &dst_dir, cancel, &on_progress)
            .map_err(|e| bad(e.to_string()))?
            .2;
    }

    // 2) DB — 폴더 행이 새 라이브러리를 가리킨다
    let batch = super::open_batch(db, "offload", &format!("{} → {}", sub, dst_lib.name))?;
    db.transaction(|tx| {
        for (id, rel) in &rows {
            tx.execute(
                "UPDATE folders SET volume_uuid = ?2, library_id = ?3, rel_path = ?4, area = ?5,
                        parent_id = CASE WHEN id = ?6 THEN NULL ELSE parent_id END
                  WHERE id = ?1",
                rusqlite::params![id, dst_lib.volume_uuid, dst_lib.id, new_rel(rel), dst_lib.area, f.id],
            )?;
        }
        Ok(())
    })?;
    super::close_batch(db, batch, files as usize)?;

    // 3) 썸네일 — 라이브러리별 캐시 폴더 사이로
    let (from, to) = (cache::cache_root(cache_base, src_lib.id), cache::cache_root(cache_base, dst_lib.id));
    for rel in &thumbs {
        let (a, b) = (from.join(rel), to.join(rel));
        if let Some(p) = b.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if std::fs::rename(&a, &b).is_err() {
            let _ = std::fs::copy(&a, &b).and_then(|_| std::fs::remove_file(&a));
        }
    }
    Ok(Offloaded { folders: rows.len(), files: files as usize, bytes: bytes as u64, undeleted })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_library_strips_the_library_prefix() {
        assert_eq!(under_library("", "2015/여행"), Some("2015/여행".into()));
        assert_eq!(under_library("사진", "사진/2015/여행"), Some("2015/여행".into()));
        assert_eq!(under_library("사진", "사진"), None);
        assert_eq!(under_library("사진", "다른/2015"), None);
    }

    #[test]
    fn copy_tree_moves_every_file_and_leaves_no_source() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("a/2015"), d.path().join("b/2015"));
        std::fs::create_dir_all(src.join("여행")).unwrap();
        std::fs::write(src.join("x.jpg"), b"xx").unwrap();
        std::fs::write(src.join("여행/y.jpg"), b"yyyy").unwrap();
        let cancel = AtomicBool::new(false);
        let (n, bytes, _) = copy_tree(&src, &dst, &cancel, &|_| {}).unwrap();
        assert_eq!((n, bytes), (2, 6));
        assert_eq!(std::fs::read(dst.join("여행/y.jpg")).unwrap(), b"yyyy");
        assert!(!src.exists());
    }

    /// 두 라이브러리(같은 볼륨의 임시 폴더) 사이로 폴더 한 갈래를 옮긴다 —
    /// 디스크의 파일, 폴더 행, 썸네일이 다 따라가야 한다.
    #[test]
    fn move_folder_repoints_rows_and_moves_files_and_thumbs() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(d.path().join("t.db")).unwrap();
        let (a, b, cache) = (d.path().join("A"), d.path().join("B"), d.path().join("cache"));
        std::fs::create_dir_all(a.join("2015/여행")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("2015/x.jpg"), b"x").unwrap();
        std::fs::write(a.join("2015/여행/y.jpg"), b"yy").unwrap();
        let la = libraries::add(&db, &a, 1).unwrap();
        let lb = libraries::add(&db, &b, 3).unwrap();
        // 폴더·파일·썸네일 행을 손으로 심는다 (스캔 없이)
        let (fid_2015, fid_trip) = db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO folders(volume_uuid, library_id, rel_path, name, area, file_count)
                     VALUES(?1, ?2, ?3, '2015', 1, 1)",
                    rusqlite::params![la.volume_uuid, la.id, cache::rel_path(&la.rel_path, "2015")],
                )?;
                let f1 = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO folders(volume_uuid, library_id, rel_path, parent_id, name, area, file_count)
                     VALUES(?1, ?2, ?3, ?4, '여행', 1, 1)",
                    rusqlite::params![la.volume_uuid, la.id, cache::rel_path(&la.rel_path, "2015/여행"), f1],
                )?;
                let f2 = tx.last_insert_rowid();
                for (id, folder, name) in [(1, f1, "x.jpg"), (2, f2, "y.jpg")] {
                    tx.execute(
                        "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                         VALUES(?1,?2,?3,1,0,0,0,0)",
                        rusqlite::params![id, folder, name],
                    )?;
                    tx.execute(
                        "INSERT INTO thumbs(file_id, rel_path, src_size, src_mtime, state) VALUES(?1, ?2, 1, 0, 1)",
                        rusqlite::params![id, format!("ab/{id}.jpg")],
                    )?;
                }
                Ok((f1, f2))
            })
            .unwrap();
        let from = cache::cache_root(&cache, la.id).join("ab");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("1.jpg"), b"t1").unwrap();
        std::fs::write(from.join("2.jpg"), b"t2").unwrap();

        let cancel = AtomicBool::new(false);
        let o = move_folder(&db, &cache, fid_2015, lb.id, &cancel, |_| {}).unwrap();
        assert_eq!((o.folders, o.files), (2, 2));
        // 디스크
        assert!(b.join("2015/여행/y.jpg").exists());
        assert!(!a.join("2015").exists());
        // 폴더 행 — 새 라이브러리, 새 볼륨 기준 경로, 뿌리의 parent는 없음
        let (lib, rel, parent): (i64, String, Option<i64>) = db
            .read(|c| {
                c.query_row(
                    "SELECT library_id, rel_path, parent_id FROM folders WHERE id = ?1",
                    [fid_2015],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!((lib, rel, parent), (lb.id, cache::rel_path(&lb.rel_path, "2015"), None));
        let (lib2, rel2): (i64, String) = db
            .read(|c| c.query_row("SELECT library_id, rel_path FROM folders WHERE id = ?1", [fid_trip], |r| Ok((r.get(0)?, r.get(1)?))))
            .unwrap();
        assert_eq!((lib2, rel2), (lb.id, cache::rel_path(&lb.rel_path, "2015/여행")));
        // 썸네일
        assert!(cache::cache_root(&cache, lb.id).join("ab/2.jpg").exists());
        assert!(!from.join("1.jpg").exists());
        // 같은 자리로 다시 옮기면 대상에 폴더가 이미 있다고 거절… 아니, 원래 자리는 비었으니 된다 (되돌리기)
        let back = move_folder(&db, &cache, fid_2015, la.id, &cancel, |_| {}).unwrap();
        assert_eq!(back.files, 2);
        assert!(a.join("2015/x.jpg").exists());
    }

    #[test]
    fn a_cancelled_copy_leaves_the_source_and_removes_the_partial_copy() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("a/2015"), d.path().join("b/2015"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("x.jpg"), b"xx").unwrap();
        let cancel = AtomicBool::new(true);
        assert!(copy_tree(&src, &dst, &cancel, &|_| {}).is_err());
        assert!(src.join("x.jpg").exists());
        assert!(!dst.exists());
    }
}
