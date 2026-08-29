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
use std::path::{Path, PathBuf};

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
                fi.name, fi.size
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
fn lookups(
    db: &Db,
    items: &[Item],
) -> Result<(
    std::collections::HashMap<i64, libraries::Library>,
    std::collections::HashMap<String, Option<PathBuf>>,
)> {
    let libs = libraries::list(db)?.into_iter().map(|l| (l.id, l)).collect();
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
    for n in 2..10_000 {
        let name = match (&stem, &ext) {
            (Some(s), Some(e)) => format!("{s} ({n}).{e}"),
            (Some(s), None) => format!("{s} ({n})"),
            _ => format!("{n}"),
        };
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
    }
    want
}

/// 파일을 옮긴다. 같은 볼륨이면 rename, 아니면 복사 후 삭제.
///
/// `.acut`은 라이브러리 안에 있으니 실제로는 언제나 rename이다. 그래도
/// 폴백을 둔다 — 라이브러리가 심볼릭 링크로 다른 장치를 가리킬 수 있다.
pub fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // 다른 볼륨 — 복사한 뒤 크기가 맞는지 보고서야 원본을 지운다.
            // 디스크가 차서 반만 써진 사본을 두고 원본을 지우면 사진이 사라진다.
            let want = std::fs::metadata(from)?.len();
            let got = std::fs::copy(from, to)?;
            if got != want || std::fs::metadata(to).map(|m| m.len()).unwrap_or(0) != want {
                let _ = std::fs::remove_file(to);
                return Err(std::io::Error::other(format!(
                    "복사가 끝까지 안 됐습니다 ({got} / {want} bytes) — 디스크가 찼나요?"
                )));
            }
            copy_mtime(from, to);
            std::fs::remove_file(from)
        }
    }
}

/// 사본의 수정 시각을 원본대로 맞춘다. `fs::copy`는 내용만 옮겨 옮긴 날이 찍힌다 —
/// 촬영일 없는 파일은 이 값으로 날짜를 잡으니 어긋나면 «오늘 찍은 사진»이 된다 (리뷰 H3)
pub fn copy_mtime(from: &Path, to: &Path) {
    let Ok(m) = std::fs::metadata(from).and_then(|m| m.modified()) else { return };
    if let Ok(f) = std::fs::File::options().write(true).open(to) {
        let _ = f.set_modified(m);
    }
}

/// 제외로 판정한 것들을 휴지통으로 옮긴다.
pub fn to_trash(db: &Db, ids: &[i64], label: &str) -> Result<Outcome> {
    let items = load(db, ids, false)?;
    let batch_id = super::open_batch(db, "trash", label)?;
    let mut out = Outcome { batch_id, ..Default::default() };
    // 라이브러리·마운트는 한 번만 — 파일마다 찾으면 5천 장에 수천만 행 스캔·수만 syscall (리뷰 H16)
    let (libs, mounts) = lookups(db, &items)?;

    for it in &items {
        let lib = libs.get(&it.library_id);
        let (Some(lib_dir), Some(lib_rel), Some(mount)) = (
            lib.and_then(|l| l.dir.clone()),
            lib.map(|l| l.rel_path.clone()),
            mounts.get(&it.volume_uuid).cloned().flatten(),
        ) else {
            super::record(db, batch_id, "trash", it.id, &it.volume_uuid, &it.vol_rel, None,
                Err("디스크가 연결되어 있지 않습니다"))?;
            out.failed += 1;
            out.first_error.get_or_insert("디스크가 연결되어 있지 않습니다".into());
            continue;
        };

        let src = mount.join(&it.vol_rel);
        let dest = free_path(trash_root(&lib_dir).join(&it.lib_rel));
        let dest_rel = dest
            .strip_prefix(&lib_dir)
            .unwrap_or(&dest)
            .to_string_lossy()
            .into_owned();

        match move_file(&src, &dest) {
            Ok(()) => {
                // 저널 경로는 언제나 볼륨 기준이다 — 되돌릴 때 마운트만 붙이면 된다
                let to_vol_rel = crate::media::cache::rel_path(&lib_rel, &dest_rel);
                super::record(db, batch_id, "trash", it.id, &it.volume_uuid, &it.vol_rel,
                    Some(&to_vol_rel), Ok(()))?;
                db.write(|c| {
                    c.execute(
                        "UPDATE files SET trashed_at = strftime('%s','now'),
                                          trash_path = ?2, trash_batch = ?3
                         WHERE id = ?1",
                        rusqlite::params![it.id, dest_rel, batch_id],
                    )
                })?;
                out.moved += 1;
                out.bytes += it.size;
            }
            Err(e) => {
                let msg = e.to_string();
                super::record(db, batch_id, "trash", it.id, &it.volume_uuid, &it.vol_rel, None,
                    Err(&msg))?;
                out.failed += 1;
                out.first_error.get_or_insert(msg);
            }
        }
    }

    super::close_batch(db, batch_id, out.moved)?;
    Ok(out)
}

/// 휴지통에서 제자리로 되돌린다. 평점·판정은 그대로 살아 있다.
pub fn restore(db: &Db, ids: &[i64]) -> Result<Outcome> {
    let items = load(db, ids, true)?;
    let batch_id = super::open_batch(db, "restore", "휴지통에서 되돌리기")?;
    let mut out = Outcome { batch_id, ..Default::default() };

    let paths: std::collections::HashMap<i64, String> = db.read(|c| {
        let mut st = c.prepare("SELECT id, trash_path FROM files WHERE trashed_at IS NOT NULL")?;
        let it = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))?;
        it.collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()
    })?;
    let (libs, mounts) = lookups(db, &items)?;

    for it in &items {
        let (Some(lib_dir), Some(mount), Some(tp)) = (
            libs.get(&it.library_id).and_then(|l| l.dir.clone()),
            mounts.get(&it.volume_uuid).cloned().flatten(),
            paths.get(&it.id),
        ) else {
            out.failed += 1;
            out.first_error.get_or_insert("되돌릴 위치를 알 수 없습니다".into());
            continue;
        };

        let src = lib_dir.join(tp);
        let dest = free_path(mount.join(&it.vol_rel));
        match move_file(&src, &dest) {
            Ok(()) => {
                // 그새 같은 이름이 생겨 «IMG_1 (2).jpg»로 돌아왔을 수 있다 — 행도 그 이름으로.
                // 안 맞추면 다음 치우기·이름 바꾸기가 다른 사진에 걸린다 (리뷰 C5)
                let new_name = dest
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| it.vol_rel.rsplit('/').next().unwrap_or(&it.vol_rel).to_string());
                db.write(|c| {
                    c.execute(
                        "UPDATE files SET trashed_at = NULL, trash_path = NULL, trash_batch = NULL,
                                name = ?2
                         WHERE id = ?1",
                        rusqlite::params![it.id, new_name],
                    )
                })?;
                out.moved += 1;
                out.bytes += it.size;
            }
            Err(e) => {
                out.failed += 1;
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
    let batch_id = super::open_batch(db, "delete", "휴지통 비우기")?;
    let mut out = Outcome { batch_id, ..Default::default() };

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
            out.first_error.get_or_insert("휴지통 밖의 경로입니다".into());
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
        db.write(|c| c.execute("DELETE FROM files WHERE id = ?1", [it.id]))?;
        out.moved += 1;
        out.bytes += it.size;
    }
    super::close_batch(db, batch_id, out.moved)?;
    Ok(out)
}

/// 정규화한 뒤에도 `root` 안에 있는가. 없는 경로는 부모까지 올라가 확인한다.
fn is_inside(path: &Path, root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let real = path
        .canonicalize()
        .or_else(|_| path.parent().map(Path::canonicalize).unwrap_or_else(|| path.canonicalize()));
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
            |r| Ok(Summary { files: r.get(0)?, bytes: r.get(1)? }),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_test;

    /// 사진 몇 장을 만들고 스캔한다. 라이브러리 폴더를 그대로 돌려준다.
    fn setup() -> (tempfile::TempDir, Db, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("2020").join("여행");
        std::fs::create_dir_all(&a).unwrap();
        for n in ["20200101_120000.jpg", "20200101_120001.jpg", "20200101_120002.jpg"] {
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

    fn alive(db: &Db) -> i64 {
        db.read(|c| {
            c.query_row("SELECT COUNT(*) FROM files WHERE trashed_at IS NULL", [], |r| r.get(0))
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
            trash_root(dir.path()).join("2020/여행/20200101_120000.jpg").is_file(),
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
            .read(|c| c.query_row("SELECT rating FROM files WHERE id=?1", [ids[0]], |r| r.get(0)))
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
        assert!(trash_root(dir.path()).join("2020/여행/20200101_120000.jpg").is_file());
        assert!(trash_root(dir.path()).join("2021/여행/20200101_120000.jpg").is_file());
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
            c.execute("UPDATE files SET culling_flag=2 WHERE id IN (?1,?2)", [ids[0], ids[1]])
        })
        .unwrap();
        assert_eq!(pending(&db, None).unwrap().len(), 2);

        to_trash(&db, &ids[..2], "시험").unwrap();
        assert!(pending(&db, None).unwrap().is_empty(), "치운 것은 대기가 아니다");
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
        assert_eq!(name, "20200101_120000 (2).jpg", "행이 실제 파일 이름을 가리킨다");
        assert!(dir.path().join("2020/여행").join(&name).is_file());
    }
}
