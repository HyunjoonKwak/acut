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
