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
    empty(&db, dir.path(), &ids[1..]).unwrap();
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

/// trash_path 가 휴지통 밖의 살아 있는 사진을 가리켜도 그 파일을 옮기지 않는다
#[test]
fn restore_refuses_a_trash_path_outside_the_trash() {
    let (dir, db, ids) = setup();
    let a = dir.path().join("2020/여행");
    to_trash(&db, &ids[..1], "치우기").unwrap();
    db.write(|c| {
        c.execute(
            "UPDATE files SET trash_path='2020/여행/20200101_120001.jpg' WHERE id=?1",
            [ids[0]],
        )
    })
    .unwrap();
    let out = restore(&db, &ids[..1]).unwrap();
    assert_eq!((out.moved, out.failed), (0, 1), "{:?}", out.first_error);
    assert!(out
        .first_error
        .as_deref()
        .unwrap_or_default()
        .contains("휴지통 밖"));
    assert!(
        a.join("20200101_120001.jpg").is_file(),
        "살아 있는 사진은 그대로다"
    );
    let trashed: Option<i64> = db
        .read(|c| {
            c.query_row("SELECT trashed_at FROM files WHERE id=?1", [ids[0]], |r| {
                r.get(0)
            })
        })
        .unwrap();
    assert!(trashed.is_some());
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
    empty(&db, dir.path(), &ids[..1]).unwrap();
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

#[test]
fn free_path_uses_the_first_free_number_from_one_directory_read() {
    let d = tempfile::tempdir().unwrap();
    for name in ["a.jpg", "a (2).jpg", "A (3).JPG"] {
        std::fs::write(d.path().join(name), b"").unwrap();
    }
    assert_eq!(
        free_path(d.path().join("a.jpg")),
        d.path().join("a (4).jpg")
    );
}

#[test]
fn restore_and_empty_ignore_unrequested_trash_rows() {
    let (dir, db, ids) = setup();
    assert_eq!(to_trash(&db, &ids, "치우기").unwrap().moved, 3);
    db.write(|c| c.execute("UPDATE files SET trash_path = NULL WHERE id = ?1", [ids[2]]))
        .unwrap();

    assert_eq!(restore(&db, &ids[..1]).unwrap().moved, 1);
    assert_eq!(empty(&db, dir.path(), &ids[1..2]).unwrap().moved, 1);
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

    let out = empty(&db, dir.path(), &ids[..2]).unwrap();
    assert_eq!(out.failed, 1, "휴지통 밖은 거부한다");
    assert_eq!(out.moved, 1);
    assert!(
        dir.path().join("2020/여행/20200101_120002.jpg").is_file(),
        "밖에 있는 파일은 그대로 있어야 한다"
    );
}

#[test]
fn empty_removes_the_row_for_real() {
    let (dir, db, ids) = setup();
    let thumb = crate::media::cache::cache_root(dir.path(), 1).join("aa/thumb.jpg");
    std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
    std::fs::write(&thumb, b"thumbnail").unwrap();
    db.write(|c| {
        c.execute(
            "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state)
                 VALUES(?1,'aa/thumb.jpg',1,1,1)",
            [ids[0]],
        )
    })
    .unwrap();
    to_trash(&db, &ids[..1], "시험").unwrap();
    empty(&db, dir.path(), &ids[..1]).unwrap();
    let n: i64 = db
        .read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(n, 2, "비우면 그때 행도 사라진다");
    assert!(!thumb.exists(), "썸네일 파일도 함께 지워져야 한다");
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
