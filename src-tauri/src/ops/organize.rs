//! 정리 — 고른 사진을 이벤트 폴더로 옮긴다.
//!
//! 이 앱에서 **물리적 위치가 곧 처리 단계**다. DB의 상태 값이 아니라 파일이
//! 실제로 어느 폴더에 있느냐가 그 사진의 상태다. 그래서 정리는 진짜 이동이다.
//!
//! 옮긴 뒤 `files.folder_id`를 새 폴더로 바꾼다. 재스캔을 기다리지 않는다 —
//! 옮기자마자 그리드에서 새 폴더로 보여야 손에 맞는다.

use crate::db::conn::{Db, Result};
use crate::db::libraries;
use crate::ops::trash::{free_path, move_with_sidecars, Outcome};

/// 옮길 곳. 라이브러리 안의 상대 폴더 경로다 (`2024/2024-08-27 거제통영`).
pub struct Dest {
    pub library_id: i64,
    /// 라이브러리 루트 기준. 빈 문자열이면 루트 바로 아래.
    pub rel_dir: String,
}

struct Item {
    id: i64,
    library_id: i64,
    folder_id: i64,
    volume_uuid: String,
    /// 볼륨 기준 상대경로 (파일명 포함)
    vol_rel: String,
    name: String,
}

fn load(db: &Db, ids: &[i64]) -> Result<Vec<Item>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT fi.id, fo.library_id, fo.volume_uuid,
                fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name,
                fi.name, fi.folder_id
         FROM files fi JOIN folders fo ON fo.id = fi.folder_id
         WHERE fi.id IN ({list}) AND fi.trashed_at IS NULL"
    );
    db.read(|c| {
        let mut st = c.prepare(&sql)?;
        let it = st.query_map([], |r| {
            Ok(Item {
                id: r.get(0)?,
                library_id: r.get(1)?,
                volume_uuid: r.get(2)?,
                vol_rel: r.get(3)?,
                name: r.get(4)?,
                folder_id: r.get(5)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 폴더 행을 보장하고 id를 돌려준다. 없으면 만든다.
fn ensure_folder(db: &Db, library_id: i64, vol_rel_dir: &str, area: i32) -> Result<i64> {
    let name = vol_rel_dir
        .rsplit('/')
        .next()
        .unwrap_or(vol_rel_dir)
        .to_string();
    let uuid: String = db.read(|c| {
        c.query_row(
            "SELECT volume_uuid FROM libraries WHERE id = ?1",
            [library_id],
            |r| r.get(0),
        )
    })?;
    db.write(|c| {
        c.execute(
            "INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at)
             VALUES(?1,?2,?3,?4,?5,strftime('%s','now'))
             ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET library_id=excluded.library_id",
            rusqlite::params![uuid, library_id, vol_rel_dir, name, area],
        )
    })?;
    db.read(|c| {
        c.query_row(
            "SELECT id FROM folders WHERE volume_uuid=?1 AND rel_path=?2",
            rusqlite::params![uuid, vol_rel_dir],
            |r| r.get(0),
        )
    })
}

/// 고른 사진을 목적지 폴더로 옮긴다.
///
/// 라이브러리를 넘나드는 이동도 된다 — 작업대에서 내사진으로 가는 흐름이
/// 그렇다. 볼륨이 다르면 복사 후 삭제가 되므로 느리다.
pub fn move_to(db: &Db, ids: &[i64], dest: &Dest, label: &str) -> Result<Outcome> {
    let items = load(db, ids)?;
    let Some(lib) = libraries::get(db, dest.library_id)? else {
        return Ok(Outcome {
            failed: items.len(),
            first_error: Some("등록되지 않은 라이브러리입니다".into()),
            failed_ids: items.iter().map(|it| it.id).collect(),
            ..Default::default()
        });
    };
    let Some(lib_dir) = lib.dir.clone() else {
        return Ok(Outcome {
            failed: items.len(),
            first_error: Some("디스크가 연결되어 있지 않습니다".into()),
            failed_ids: items.iter().map(|it| it.id).collect(),
            ..Default::default()
        });
    };

    let batch_id = super::open_batch(db, "move", label)?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };

    // 목적지 폴더의 **볼륨** 기준 경로. 폴더 행은 이 값으로 유일하다.
    let dest_vol_dir = crate::media::cache::rel_path(&lib.rel_path, &dest.rel_dir);
    let dest_folder = ensure_folder(db, dest.library_id, &dest_vol_dir, lib.area)?;
    let dest_dir = if dest.rel_dir.is_empty() {
        lib_dir.clone()
    } else {
        lib_dir.join(&dest.rel_dir)
    };

    for it in &items {
        let Some(mount) = crate::db::volumes::find_mount(&it.volume_uuid) else {
            out.failed += 1;
            out.failed_ids.push(it.id);
            out.first_error
                .get_or_insert("디스크가 연결되어 있지 않습니다".into());
            continue;
        };
        let src = mount.join(&it.vol_rel);
        // 이미 목적지 폴더에 있는 사진은 건드리지 않는다. `free_path`를 먼저 부르면
        // 제자리 파일이 «있는 이름»으로 보여 « (2)»로 비켜 가 버린다 (2차 리뷰 H-1)
        let want = dest_dir.join(&it.name);
        if it.folder_id == dest_folder || want == src {
            continue; // 이미 제자리다
        }
        let dest_path = free_path(want);
        let new_name = dest_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| it.name.clone());
        let occupied: bool = db.read(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE folder_id=?1 AND name=?2 AND id<>?3)",
                rusqlite::params![dest_folder, new_name, it.id],
                |r| r.get(0),
            )
        })?;
        if occupied {
            out.failed += 1;
            out.failed_ids.push(it.id);
            out.first_error.get_or_insert_with(|| {
                format!("목적지 DB에 같은 이름 기록이 있습니다: {new_name}")
            });
            continue;
        }

        match move_with_sidecars(&src, &dest_path) {
            Ok(()) => {
                let to_rel = crate::media::cache::rel_path(&dest_vol_dir, &new_name);
                let (to_size, to_mtime) = super::file_stat(&dest_path);
                let changed = db.transaction(|tx| {
                    tx.execute(
                        "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok,to_size,to_mtime)
                         VALUES(?1,?2,'move',?3,?4,?5,?6,1,?7,?8)",
                        rusqlite::params![batch_id,it.id,it.volume_uuid,it.vol_rel,lib.volume_uuid,to_rel,to_size,to_mtime],
                    )?;
                    tx.execute(
                        "UPDATE files SET folder_id = ?2, name = ?3 WHERE id = ?1",
                        rusqlite::params![it.id, dest_folder, new_name],
                    )?;
                    if it.library_id != dest.library_id {
                        tx.execute("DELETE FROM thumbs WHERE file_id=?1", [it.id])?;
                    }
                    Ok(())
                });
                match changed {
                    Ok(()) => out.moved += 1,
                    Err(error) => {
                        let rollback = move_with_sidecars(&dest_path, &src);
                        out.failed += 1;
                        out.failed_ids.push(it.id);
                        out.first_error.get_or_insert_with(|| match rollback {
                            Ok(()) => error.to_string(),
                            Err(rollback) => format!(
                                "DB 갱신 실패: {error}; 파일 원위치 복구도 실패: {rollback}"
                            ),
                        });
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                super::record(
                    db,
                    batch_id,
                    "move",
                    it.id,
                    &it.volume_uuid,
                    &it.vol_rel,
                    None,
                    Err(&msg),
                )?;
                out.failed += 1;
                out.failed_ids.push(it.id);
                out.first_error.get_or_insert(msg);
            }
        }
    }

    super::close_batch(db, batch_id, out.moved)?;
    // 떠난 라이브러리 전부와 도착 라이브러리 — 첫 파일의 것만 세면 다른 라이브러리에서
    // 온 파일의 폴더 수가 틀린 채 남는다
    let mut libs: Vec<i64> = items.iter().map(|i| i.library_id).collect();
    libs.push(dest.library_id);
    libs.sort_unstable();
    libs.dedup();
    if let Err(error) = recount(db, &libs) {
        log::warn!("정리 뒤 폴더 장수 갱신 보류: {error}");
    }
    Ok(out)
}

/// 폴더별 사진 수를 다시 센다. 사이드바 숫자가 곧바로 맞아야 한다.
fn recount(db: &Db, libs: &[i64]) -> Result<()> {
    for id in libs {
        db.write(|c| {
            c.execute(
                "UPDATE folders SET file_count =
                   (SELECT COUNT(*) FROM files
                     WHERE files.folder_id = folders.id AND files.trashed_at IS NULL)
                 WHERE library_id = ?1",
                [id],
            )
        })?;
    }
    Ok(())
}

/// 이 파일들이 지금 속한 라이브러리들 — 옮기고 나서 비는 폴더는 여기에 생긴다
pub fn libraries_of(db: &Db, ids: &[i64]) -> Result<Vec<i64>> {
    Ok(load(db, ids)?
        .into_iter()
        .map(|it| it.library_id)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect())
}

/// 선택 사진의 현재 영역. 내사진→공용 정리의 기본 동작을 복사로 고를 때 쓴다.
pub fn areas_of(db: &Db, ids: &[i64]) -> Result<Vec<i32>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    db.read(|connection| {
        let mut statement = connection.prepare(&format!(
            "SELECT DISTINCT l.area FROM files fi
             JOIN folders fo ON fo.id=fi.folder_id
             JOIN libraries l ON l.id=fo.library_id
             WHERE fi.id IN ({list}) AND fi.trashed_at IS NULL ORDER BY l.area"
        ))?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
}

pub fn should_publish(source_areas: &[i32], destination_area: i32) -> bool {
    source_areas == [1] && destination_area == 2
}

/// 옮기고 나서 빈 껍데기만 남은 폴더 행을 치운다. 디스크의 빈 폴더는 두고
/// DB에서만 감춘다 — 사용자가 밖에서 만든 폴더를 우리가 지우면 안 된다.
pub fn forget_empty_folders(db: &Db, library_id: i64) -> Result<usize> {
    db.write(|c| {
        c.execute(
            "DELETE FROM folders WHERE library_id = ?1
               AND NOT EXISTS (SELECT 1 FROM files WHERE files.folder_id = folders.id)",
            [library_id],
        )
    })
}

/// 사람이 읽는 이벤트 폴더 이름. `2024-08-27 거제통영 가족여행`
pub fn event_folder_name(date: &str, title: &str) -> String {
    let t = title.trim();
    if t.is_empty() {
        date.to_string()
    } else {
        format!("{date} {t}")
    }
}

/// 파일명에 쓸 수 없는 글자를 걸러 낸다.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        })
        .collect();
    let t = cleaned.trim().trim_matches('.').trim();
    crate::scan::nfc(t)
}

/// 목적지 폴더 경로를 만든다. 연도로 한 겹 나눈다 — NAS 공용 구조와 같다.
pub fn event_rel_dir(date: &str, title: &str) -> String {
    let year = date.get(0..4).unwrap_or("0000");
    format!("{year}/{}", sanitize(&event_folder_name(date, title)))
}

/// 영역에 맞는 모양 — 내사진(1)은 NAS Photos처럼 **평평한** 이벤트 폴더,
/// 공용(2)과 나머지는 연도/이벤트. Drive Client가 1:1로 맞추니 모양이 같아야 한다.
pub fn event_rel_dir_for(area: i32, date: &str, title: &str) -> String {
    if area == 1 {
        sanitize(&event_folder_name(date, title))
    } else {
        event_rel_dir(date, title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_test;

    /// 폴더 행의 **라이브러리 기준** 경로. rel_path는 볼륨 기준이라
    /// 임시 폴더에서는 `var/folders/…`가 앞에 붙는다.
    fn lib_rel_of(db: &Db, file_id: i64) -> String {
        db.read(|c| {
            c.query_row(
                "SELECT CASE WHEN l.rel_path='' THEN fo.rel_path
                             ELSE substr(fo.rel_path, length(l.rel_path)+2) END
                 FROM files fi JOIN folders fo ON fo.id=fi.folder_id
                 JOIN libraries l ON l.id=fo.library_id WHERE fi.id=?1",
                [file_id],
                |r| r.get(0),
            )
        })
        .unwrap()
    }

    fn lib_counts(db: &Db) -> Vec<(String, i64)> {
        db.read(|c| {
            let mut st = c.prepare(
                "SELECT CASE WHEN l.rel_path='' THEN fo.rel_path
                             ELSE substr(fo.rel_path, length(l.rel_path)+2) END,
                        fo.file_count
                 FROM folders fo JOIN libraries l ON l.id=fo.library_id
                 ORDER BY 1",
            )?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
    }

    fn setup() -> (tempfile::TempDir, Db, i64, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("작업대");
        std::fs::create_dir_all(&src).unwrap();
        for n in ["20240827_120000.jpg", "20240827_120001.jpg"] {
            std::fs::write(src.join(n), b"bytes ".repeat(20)).unwrap();
        }
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        let lib: i64 = db
            .read(|c| c.query_row("SELECT id FROM libraries", [], |r| r.get(0)))
            .unwrap();
        let ids: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare("SELECT id FROM files ORDER BY name")?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        (dir, db, lib, ids)
    }

    #[test]
    fn my_photos_are_flat_and_shared_is_by_year() {
        assert_eq!(
            event_rel_dir_for(1, "2024-08-27", "거제"),
            "2024-08-27 거제"
        );
        assert_eq!(
            event_rel_dir_for(2, "2024-08-27", "거제"),
            "2024/2024-08-27 거제"
        );
        assert_eq!(event_rel_dir_for(0, "2024-08-27", ""), "2024/2024-08-27");
    }

    #[test]
    fn only_my_photos_to_shared_uses_publication_copy() {
        assert!(should_publish(&[1], 2));
        assert!(!should_publish(&[0], 2));
        assert!(!should_publish(&[1], 1));
        assert!(!should_publish(&[0, 1], 2));
    }

    #[test]
    fn moves_files_and_repoints_the_row() {
        let (dir, db, lib, ids) = setup();
        let dest = Dest {
            library_id: lib,
            rel_dir: event_rel_dir("2024-08-27", "거제통영 가족여행"),
        };
        let out = move_to(&db, &ids, &dest, "정리").unwrap();
        assert_eq!((out.moved, out.failed), (2, 0));

        let target = dir.path().join("2024/2024-08-27 거제통영 가족여행");
        assert!(target.join("20240827_120000.jpg").is_file());
        assert!(!dir.path().join("작업대/20240827_120000.jpg").exists());

        // DB도 새 폴더를 가리켜야 한다 — 재스캔을 기다리지 않는다
        assert_eq!(lib_rel_of(&db, ids[0]), "2024/2024-08-27 거제통영 가족여행");
    }

    /// 같은 이벤트로 다시 «정리»해도 제자리 사진에 번호가 붙으면 안 된다
    #[test]
    fn organizing_into_the_folder_a_photo_already_lives_in_leaves_it_alone() {
        let (dir, db, lib, ids) = setup();
        let dest = Dest {
            library_id: lib,
            rel_dir: "2024/행사".into(),
        };
        assert_eq!(move_to(&db, &ids, &dest, "정리").unwrap().moved, 2);

        let again = move_to(&db, &ids, &dest, "정리").unwrap();
        assert_eq!(
            (again.moved, again.failed),
            (0, 0),
            "{:?}",
            again.first_error
        );
        let target = dir.path().join("2024/행사");
        assert!(target.join("20240827_120000.jpg").is_file());
        assert!(
            !target.join("20240827_120000 (2).jpg").exists(),
            "제자리 사진을 비켜 쓰면 안 된다"
        );
        let names: Vec<String> = db
            .read(|c| {
                let mut st = c.prepare("SELECT name FROM files ORDER BY name")?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect()
            })
            .unwrap();
        assert!(names.iter().all(|n| !n.contains(" (2)")), "{names:?}");
    }

    #[test]
    fn partial_failure_reports_the_ids_that_can_be_retried() {
        let (dir, db, lib, ids) = setup();
        std::fs::remove_file(dir.path().join("작업대/20240827_120000.jpg")).unwrap();
        let dest = Dest {
            library_id: lib,
            rel_dir: "2024/행사".into(),
        };
        let out = move_to(&db, &ids, &dest, "정리").unwrap();
        assert_eq!((out.moved, out.failed), (1, 1));
        assert_eq!(out.failed_ids, vec![ids[0]]);
    }

    #[test]
    fn folder_counts_are_updated() {
        let (_d, db, lib, ids) = setup();
        let dest = Dest {
            library_id: lib,
            rel_dir: "2024/행사".into(),
        };
        move_to(&db, &ids[..1], &dest, "정리").unwrap();

        let counts = lib_counts(&db);
        assert!(counts.contains(&("2024/행사".to_string(), 1)), "{counts:?}");
        assert!(counts.contains(&("작업대".to_string(), 1)), "{counts:?}");
    }

    #[test]
    fn same_name_gets_a_number_not_an_overwrite() {
        let (dir, db, lib, ids) = setup();
        // 목적지에 같은 이름을 미리 놓아 둔다
        let target = dir.path().join("2024/행사");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("20240827_120000.jpg"), b"already here").unwrap();

        let dest = Dest {
            library_id: lib,
            rel_dir: "2024/행사".into(),
        };
        move_to(&db, &ids[..1], &dest, "정리").unwrap();

        assert_eq!(
            std::fs::read(target.join("20240827_120000.jpg")).unwrap(),
            b"already here",
            "먼저 있던 파일을 덮어쓰면 안 된다"
        );
        assert!(target.join("20240827_120000 (2).jpg").is_file());
        // DB의 이름도 따라 바뀌어야 한다
        let name: String = db
            .read(|c| c.query_row("SELECT name FROM files WHERE id=?1", [ids[0]], |r| r.get(0)))
            .unwrap();
        assert_eq!(name, "20240827_120000 (2).jpg");
    }

    #[test]
    fn empty_folder_rows_are_forgotten() {
        let (_d, db, lib, ids) = setup();
        let dest = Dest {
            library_id: lib,
            rel_dir: "2024/행사".into(),
        };
        move_to(&db, &ids, &dest, "정리").unwrap();
        assert_eq!(
            forget_empty_folders(&db, lib).unwrap(),
            1,
            "빈 「작업대」 행이 사라진다"
        );
    }

    #[test]
    fn folder_names_are_built_the_way_the_nas_does() {
        assert_eq!(
            event_folder_name("2024-08-27", " 거제통영 가족여행 "),
            "2024-08-27 거제통영 가족여행"
        );
        assert_eq!(
            event_folder_name("2024-08-27", ""),
            "2024-08-27",
            "제목이 없으면 날짜만"
        );
        assert_eq!(event_rel_dir("2024-08-27", "여행"), "2024/2024-08-27 여행");
    }

    #[test]
    fn path_separators_never_leak_into_a_name() {
        // 사용자가 "2024/여행"이라고 치면 폴더가 두 겹이 되어 버린다
        assert_eq!(sanitize("가족/여행"), "가족-여행");
        assert_eq!(sanitize("a:b\\c"), "a-b-c");
        assert_eq!(sanitize("  ..점.  "), "점");
        assert_eq!(
            event_rel_dir("2024-08-27", "제주/서귀포"),
            "2024/2024-08-27 제주-서귀포"
        );
    }
}
