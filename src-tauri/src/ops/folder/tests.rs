use super::*;

fn setup() -> (tempfile::TempDir, Db, Library, Library) {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("A");
    let b = temp.path().join("B");
    std::fs::create_dir_all(a.join("부모/자식")).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    for i in 0..20 {
        std::fs::write(a.join(format!("부모/자식/{i}.jpg")), format!("photo-{i}")).unwrap();
    }
    std::fs::create_dir_all(a.join("부모/빈폴더")).unwrap();
    let db = Db::open(temp.path().join("t.db")).unwrap();
    let la = crate::db::libraries::add(&db, &a, 1).unwrap();
    let lb = crate::db::libraries::add(&db, &b, 2).unwrap();
    crate::scan::scan_folder(&db, la.id, &a, 1, |_| {}).unwrap();
    (temp, db, la, lb)
}

fn req(action: Action, source: i64, path: &str) -> Request {
    Request {
        action,
        source_library_id: source,
        source_dir: path.into(),
        destination_library_id: None,
        destination_parent: None,
        name: None,
        conflict_policy: ConflictPolicy::Skip,
    }
}

#[test]
fn create_rename_move_copy_trash_and_undo_keep_manifest() {
    let (_temp, db, la, lb) = setup();
    let mut create = req(Action::Create, la.id, "");
    create.name = Some("새 폴더".into());
    let made = execute(&db, &create, "생성").unwrap();
    assert_eq!(made.completed, 1);
    assert!(la.dir.as_ref().unwrap().join("새 폴더").is_dir());
    assert_eq!(undo(&db, made.batch_id).unwrap().moved, 1);
    assert!(!la.dir.as_ref().unwrap().join("새 폴더").exists());

    let mut rename = req(Action::Rename, la.id, "부모/자식");
    rename.destination_parent = Some("부모".into());
    rename.name = Some("이름변경".into());
    let renamed = execute(&db, &rename, "이름 변경").unwrap();
    assert_eq!(renamed.completed, 1);
    assert!(la
        .dir
        .as_ref()
        .unwrap()
        .join("부모/이름변경/3.jpg")
        .is_file());
    assert_eq!(undo(&db, renamed.batch_id).unwrap().moved, 1);
    assert!(la.dir.as_ref().unwrap().join("부모/자식/3.jpg").is_file());

    let mut copy = req(Action::Copy, la.id, "부모/자식");
    copy.destination_library_id = Some(lb.id);
    copy.destination_parent = Some("".into());
    let copied = execute(&db, &copy, "복사").unwrap();
    assert_eq!(copied.completed, 1);
    assert!(lb.dir.as_ref().unwrap().join("자식/19.jpg").is_file());
    assert_eq!(undo(&db, copied.batch_id).unwrap().moved, 1);
    assert!(!lb.dir.as_ref().unwrap().join("자식").exists());
    assert!(la.dir.as_ref().unwrap().join("부모/자식/19.jpg").is_file());

    let trashed = execute(&db, &req(Action::Trash, la.id, "부모/자식"), "폴더 휴지통").unwrap();
    assert_eq!(trashed.completed, 1);
    assert!(!la.dir.as_ref().unwrap().join("부모/자식").exists());
    assert_eq!(undo(&db, trashed.batch_id).unwrap().moved, 1);
    assert!(la.dir.as_ref().unwrap().join("부모/자식/0.jpg").is_file());

    let empty = execute(
        &db,
        &req(Action::Trash, la.id, "부모/빈폴더"),
        "빈 폴더 휴지통",
    )
    .unwrap();
    assert_eq!((empty.completed, empty.files), (1, 0));
    assert!(!la.dir.as_ref().unwrap().join("부모/빈폴더").exists());
    assert_eq!(undo(&db, empty.batch_id).unwrap().moved, 1);
    assert!(la.dir.as_ref().unwrap().join("부모/빈폴더").is_dir());
}

/// 같은 볼륨 이름변경은 내용을 읽지 않고 이름·크기·mtime 만 남긴다. undo 도 그 값으로 대조한다
#[test]
fn same_volume_rename_uses_a_stat_manifest_and_undo_refuses_a_changed_file() {
    let (_temp, db, la, _lb) = setup();
    let mut rename = req(Action::Rename, la.id, "부모/자식");
    rename.destination_parent = Some("부모".into());
    rename.name = Some("이름변경".into());
    let renamed = execute(&db, &rename, "이름 변경").unwrap();
    assert_eq!(renamed.completed, 1);
    assert!(
        renamed.manifest_sha256.is_none(),
        "같은 볼륨 이름변경은 내용 해시를 계산하지 않는다"
    );
    let (content, stat): (String, Option<String>) = db
        .read(|c| {
            c.query_row(
                "SELECT manifest_sha256, stat_sha256 FROM folder_journal WHERE batch_id=?1",
                [renamed.batch_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert!(content.is_empty());
    assert!(stat.is_some_and(|digest| !digest.is_empty()));

    let root = la.dir.clone().unwrap();
    std::fs::write(
        root.join("부모/이름변경/3.jpg"),
        b"edited after the rename, and longer",
    )
    .unwrap();
    let out = undo(&db, renamed.batch_id).unwrap();
    assert_eq!((out.moved, out.failed), (0, 1), "{:?}", out.first_error);
    assert!(out
        .first_error
        .as_deref()
        .unwrap_or_default()
        .contains("바뀌어"));
    assert!(
        root.join("부모/이름변경/3.jpg").is_file(),
        "바뀐 폴더는 그 자리에 둔다"
    );
}

/// 0.9.1 저널(내용 해시만, stat 없음)은 내용 해시로 대조해 그대로 되돌린다
#[test]
fn a_journal_without_stat_is_verified_by_content_hash() {
    let (_temp, db, la, _lb) = setup();
    let mut rename = req(Action::Rename, la.id, "부모/자식");
    rename.destination_parent = Some("부모".into());
    rename.name = Some("이름변경".into());
    let renamed = execute(&db, &rename, "이름 변경").unwrap();
    let root = la.dir.clone().unwrap();
    let full = manifest(&root.join("부모/이름변경")).unwrap();
    assert!(!full.sha256.is_empty());
    db.write(|c| {
        c.execute(
            "UPDATE folder_journal SET manifest_sha256=?2, stat_sha256=NULL WHERE batch_id=?1",
            rusqlite::params![renamed.batch_id, full.sha256],
        )
    })
    .unwrap();
    assert_eq!(undo(&db, renamed.batch_id).unwrap().moved, 1);
    assert!(root.join("부모/자식/3.jpg").is_file());
}

#[test]
fn cycle_root_offline_and_collision_are_blocked() {
    let (_temp, db, la, _) = setup();
    let mut cycle = req(Action::Move, la.id, "부모");
    cycle.destination_library_id = Some(la.id);
    cycle.destination_parent = Some("부모/자식".into());
    assert!(preview(&db, &cycle)
        .unwrap_err()
        .to_string()
        .contains("자기"));
    assert!(preview(&db, &req(Action::Trash, la.id, "")).is_err());
    let mut collide = req(Action::Create, la.id, "부모");
    collide.name = Some("자식".into());
    assert_eq!(preview(&db, &collide).unwrap().conflict, "name_exists");
    let missing = req(Action::Trash, 9999, "x");
    assert!(preview(&db, &missing).is_err());

    let offline_dir = la.dir.as_ref().unwrap().to_path_buf();
    let hidden = offline_dir.with_file_name("A-offline");
    std::fs::rename(&offline_dir, &hidden).unwrap();
    assert!(preview(&db, &req(Action::Trash, la.id, "부모"))
        .unwrap_err()
        .to_string()
        .contains("연결"));
}

#[test]
fn hidden_names_are_blocked_and_rename_cannot_change_libraries() {
    let (_temp, db, la, lb) = setup();
    let mut hidden = req(Action::Create, la.id, "");
    hidden.name = Some(".acut".into());
    assert!(preview(&db, &hidden).is_err());

    let mut rename = req(Action::Rename, la.id, "부모/자식");
    rename.destination_library_id = Some(lb.id);
    rename.destination_parent = Some(String::new());
    rename.name = Some("안전한이름".into());
    let plan = preview(&db, &rename).unwrap();
    assert_eq!(plan.destination, "부모/안전한이름");
    let out = execute(&db, &rename, "이름 변경").unwrap();
    assert_eq!((out.completed, out.failed), (1, 0));
    assert!(la
        .dir
        .as_ref()
        .unwrap()
        .join("부모/안전한이름/0.jpg")
        .is_file());
    assert!(!lb.dir.as_ref().unwrap().join("안전한이름").exists());
}

#[test]
fn partial_copy_failure_removes_temp_and_keeps_source() {
    let (_temp, _db, la, _) = setup();
    let source = la.dir.as_ref().unwrap().join("부모/자식");
    let target = la.dir.as_ref().unwrap().join("부분");
    let before = manifest(&source).unwrap();
    assert!(copy_tree_verified(&source, &target, 44, Some(3), &before).is_err());
    assert!(!target.exists());
    assert!(source.join("19.jpg").is_file());
    assert!(!temp_sibling(&target, 44).exists());
}

#[test]
fn staged_cross_volume_move_keeps_a_rollback_copy_until_db_commit() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("원본");
    let destination = temp.path().join("다른볼륨-역할");
    std::fs::create_dir_all(source.join("빈폴더")).unwrap();
    std::fs::write(source.join("photo.jpg"), b"pixels").unwrap();
    std::fs::write(source.join("photo.xmp"), b"sidecar").unwrap();
    let before = manifest(&source).unwrap();

    let backup = stage_move(&source, &destination, 77, true, &before).unwrap();
    let backup = backup.expect("볼륨 간 이동은 원본 쪽 rollback 백업을 둔다");
    assert_eq!(before.sha256, manifest(&destination).unwrap().sha256);
    assert!(!source.exists());
    assert!(
        backup.join("photo.xmp").is_file(),
        "sidecar도 백업에 남는다"
    );

    std::fs::rename(&backup, &source).unwrap();
    std::fs::remove_dir_all(&destination).unwrap();
    assert_eq!(before.sha256, manifest(&source).unwrap().sha256);
}

#[test]
fn cross_library_folder_move_invalidates_thumbnail_rows() {
    let (_temp, db, la, lb) = setup();
    let file_id = db
        .read(|c| {
            c.query_row(
                "SELECT fi.id FROM files fi JOIN folders fo ON fo.id=fi.folder_id
                     WHERE fo.library_id=?1 LIMIT 1",
                [la.id],
                |r| r.get::<_, i64>(0),
            )
        })
        .unwrap();
    db.write(|c| {
            c.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state) VALUES(?1,'aa/old.jpg',1,1,1)",
                [file_id],
            )
        })
        .unwrap();

    let mut move_request = req(Action::Move, la.id, "부모/자식");
    move_request.destination_library_id = Some(lb.id);
    move_request.destination_parent = Some(String::new());
    let moved = execute(&db, &move_request, "라이브러리 간 폴더 이동").unwrap();
    assert_eq!((moved.completed, moved.failed), (1, 0));
    assert_eq!(
        db.read(|c| c.query_row(
            "SELECT COUNT(*) FROM thumbs WHERE file_id=?1",
            [file_id],
            |r| r.get::<_, i64>(0),
        ))
        .unwrap(),
        0
    );
}

#[test]
fn moving_a_folder_keeps_descendant_parent_links() {
    let (_temp, db, la, lb) = setup();
    let (root_id, root_rel): (i64, String) = db
        .read(|c| {
            c.query_row(
                "SELECT id,rel_path FROM folders WHERE library_id=?1 AND name='자식'",
                [la.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    let child_id = db
        .write(|c| {
            c.execute(
                "INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,parent_id)
                 VALUES(?1,?2,?3,'손자',?4,?5)",
                rusqlite::params![
                    la.volume_uuid,
                    la.id,
                    format!("{root_rel}/손자"),
                    la.area,
                    root_id
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
        .unwrap();

    move_db_rows(&db, &la, "부모/자식", &lb, "옮긴자식").unwrap();

    let (root_parent, child_parent): (Option<i64>, Option<i64>) = db
        .read(|c| {
            Ok((
                c.query_row(
                    "SELECT parent_id FROM folders WHERE id=?1",
                    [root_id],
                    |r| r.get(0),
                )?,
                c.query_row(
                    "SELECT parent_id FROM folders WHERE id=?1",
                    [child_id],
                    |r| r.get(0),
                )?,
            ))
        })
        .unwrap();
    assert_eq!(root_parent, None, "옮긴 갈래의 뿌리만 부모가 없어진다");
    assert_eq!(
        child_parent,
        Some(root_id),
        "하위 폴더의 부모 연결은 유지된다"
    );
}

#[test]
fn nested_empty_and_large_folder_manifest_is_stable() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("큰 폴더");
    std::fs::create_dir_all(source.join("빈/더빈")).unwrap();
    for i in 0..1000 {
        std::fs::write(source.join(format!("{i}.txt")), b"x").unwrap();
    }
    std::fs::write(source.join("._0.txt"), b"AppleDouble must not travel").unwrap();
    let decomposed = "\u{1100}\u{1161}.txt";
    std::fs::write(source.join(decomposed), b"nfc path").unwrap();
    let target = temp.path().join("사본");
    let before = manifest(&source).unwrap();
    copy_tree_verified(&source, &target, 55, None, &before).unwrap();
    let after = manifest(&target).unwrap();
    assert_eq!(before.sha256, after.sha256);
    assert_eq!(before.files, 1001);
    assert!(!target.join("._0.txt").exists());
    assert!(
        before.file_hashes.contains_key("가.txt"),
        "manifest 경로는 유니코드 NFC여야 한다"
    );
    assert!(target.join("빈/더빈").is_dir());
}
