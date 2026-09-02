//! 실제 사진 **사본**으로만 실행하는 Gallery→Desk G2 파일럿.
//!
//! 운영 DB와 원본은 읽기만 한다. 환경변수로 받은 JPEG를 격리된 임시 라이브러리에
//! 복사하고, 서로 다른 실제 볼륨의 파일럿 폴더를 사용한다. 성공하면 파일럿 폴더를
//! 지우고, 실패하면 조사할 수 있도록 남긴다.

use crate::db::conn::Db;
use crate::ops::{capture_date, folder, transfer};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

fn sha256(path: &Path) -> String {
    let mut file = File::open(path).unwrap();
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    format!("{:x}", digest.finalize())
}

fn file_id(db: &Db, name: &str) -> i64 {
    db.read(|connection| {
        connection.query_row("SELECT id FROM files WHERE name=?1", [name], |row| {
            row.get(0)
        })
    })
    .unwrap()
}

fn folder_request(
    action: folder::Action,
    source_library_id: i64,
    source_dir: &str,
    destination_library_id: i64,
    name: &str,
) -> folder::Request {
    folder::Request {
        action,
        source_library_id,
        source_dir: source_dir.into(),
        destination_library_id: Some(destination_library_id),
        destination_parent: Some(String::new()),
        name: Some(name.into()),
        conflict_policy: folder::ConflictPolicy::Skip,
    }
}

#[test]
#[ignore = "PHOTO_DESK_G2_SOURCE_JPEG와 PHOTO_DESK_G2_CROSS_VOLUME_ROOT가 필요한 실제 파일럿"]
fn real_photo_copy_round_trip() {
    let source = PathBuf::from(
        std::env::var("PHOTO_DESK_G2_SOURCE_JPEG")
            .expect("실제 JPEG 사본의 원본 경로가 필요합니다"),
    );
    let cross_base = PathBuf::from(
        std::env::var("PHOTO_DESK_G2_CROSS_VOLUME_ROOT")
            .expect("다른 물리 볼륨의 파일럿 루트가 필요합니다"),
    );
    assert!(source.is_file());
    assert!(cross_base.is_dir());

    let original_sha = sha256(&source);
    let original_size = std::fs::metadata(&source).unwrap().len();
    let same = tempfile::Builder::new()
        .prefix("photo-desk-g2-")
        .tempdir_in("/tmp")
        .unwrap();
    let nonce = format!(
        ".photo-desk-g2-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    let cross = cross_base.join(nonce);
    let mine = same.path().join("내사진");
    let shared = same.path().join("공용");
    let cross_library = cross.join("외장파일럿");
    std::fs::create_dir_all(mine.join("G2_FOLDER/중첩")).unwrap();
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::create_dir_all(&cross_library).unwrap();

    let publish_source = mine.join("G2_PUBLISH.jpg");
    let folder_source = mine.join("G2_FOLDER/중첩/G2_FOLDER.jpg");
    std::fs::copy(&source, &publish_source).unwrap();
    std::fs::copy(&source, &folder_source).unwrap();
    std::fs::write(
        mine.join("G2_FOLDER/중첩/G2_FOLDER.xmp"),
        b"<x:xmpmeta>G2 sidecar</x:xmpmeta>",
    )
    .unwrap();

    let same_device = std::fs::metadata(same.path()).unwrap().dev();
    let cross_device = std::fs::metadata(&cross).unwrap().dev();
    assert_ne!(
        same_device, cross_device,
        "실제 서로 다른 볼륨이어야 합니다"
    );

    let db = Db::open(same.path().join("g2.db")).unwrap();
    let mine_lib = crate::db::libraries::add(&db, &mine, 1).unwrap();
    let shared_lib = crate::db::libraries::add(&db, &shared, 2).unwrap();
    let cross_lib = crate::db::libraries::add(&db, &cross_library, 0).unwrap();
    crate::scan::scan_folder(&db, mine_lib.id, &mine, 1, |_| {}).unwrap();

    // 1. 실제 JPEG 사본의 수동 교정 → 재판독 → batch undo → 원본 SHA 복원.
    let publish_id = file_id(&db, "G2_PUBLISH.jpg");
    let before_meta = crate::media::exif::read(&publish_source).unwrap();
    let before_pixels = image::open(&publish_source).unwrap().to_rgb8();
    let audit = capture_date::audit(
        &db,
        &capture_date::AuditTarget {
            ids: vec![publish_id],
            library_id: None,
            rel_path: None,
            recursive: true,
        },
    )
    .unwrap();
    assert!(audit[0].existing_exif, "실제 EXIF JPEG여야 합니다");
    assert!(
        !audit[0].auto_selected,
        "유효 EXIF는 자동 교정에서 제외됩니다"
    );
    let before_taken_at = audit[0].current_at;
    let wanted = before_taken_at + 3_600;
    let capture = capture_date::apply(
        &db,
        &[capture_date::Change {
            id: publish_id,
            taken_at: wanted,
            manual: true,
        }],
        "G2 실제 JPEG 촬영일 교정",
    )
    .unwrap();
    assert_eq!((capture.corrected, capture.failed), (1, 0));
    let capture_manifest = &capture.manifest[0];
    assert_eq!(capture_manifest.before_sha256, original_sha);
    assert_ne!(capture_manifest.after_sha256, original_sha);
    assert_eq!(capture_manifest.rescan_at, wanted);
    let written_meta = crate::media::exif::read(&publish_source).unwrap();
    assert_eq!(written_meta.taken_at, Some(wanted));
    assert_eq!(written_meta.cam_make, before_meta.cam_make);
    assert_eq!(written_meta.cam_model, before_meta.cam_model);
    assert_eq!(written_meta.gps_lat, before_meta.gps_lat);
    assert_eq!(written_meta.gps_lon, before_meta.gps_lon);
    assert_eq!(
        image::open(&publish_source).unwrap().to_rgb8(),
        before_pixels
    );
    let capture_undo = capture_date::undo(&db, capture.batch_id).unwrap();
    assert_eq!((capture_undo.moved, capture_undo.failed), (1, 0));
    assert_eq!(sha256(&publish_source), original_sha);

    // 2. 내사진 → 공용 발행. 두 번째 실행은 SHA 원장에서 막히고 개인 원본은 남는다.
    let publish_request = transfer::Request {
        ids: vec![publish_id],
        destination_library_id: shared_lib.id,
        destination_dir: "G2_PUBLISHED".into(),
        mode: transfer::Mode::Copy,
        conflict_policy: transfer::ConflictPolicy::Skip,
        publish: true,
    };
    let first_publish = transfer::execute(&db, &publish_request, "G2 공용 발행").unwrap();
    let published = shared.join("G2_PUBLISHED/G2_PUBLISH.jpg");
    assert_eq!((first_publish.completed, first_publish.failed), (1, 0));
    assert_eq!(sha256(&published), original_sha);
    assert_eq!(sha256(&publish_source), original_sha);
    let second_publish = transfer::execute(&db, &publish_request, "G2 공용 재발행").unwrap();
    assert_eq!(
        (second_publish.completed, second_publish.already_published),
        (0, 1)
    );
    let publish_undo = transfer::undo_copy(&db, first_publish.batch_id).unwrap();
    assert_eq!((publish_undo.moved, publish_undo.failed), (1, 0));
    assert!(!published.exists());
    assert_eq!(sha256(&publish_source), original_sha);

    // 3. 같은 볼륨 폴더 이동·복사와 undo.
    let same_move_request = folder_request(
        folder::Action::Move,
        mine_lib.id,
        "G2_FOLDER",
        mine_lib.id,
        "G2_SAME_MOVE",
    );
    let same_move_preview = folder::preview(&db, &same_move_request).unwrap();
    assert!(!same_move_preview.cross_volume);
    let same_move = folder::execute(&db, &same_move_request, "G2 같은 볼륨 이동").unwrap();
    assert_eq!((same_move.completed, same_move.failed), (1, 0));
    let same_move_undo = folder::undo(&db, same_move.batch_id).unwrap();
    assert_eq!((same_move_undo.moved, same_move_undo.failed), (1, 0));
    assert_eq!(sha256(&folder_source), original_sha);

    let same_copy_request = folder_request(
        folder::Action::Copy,
        mine_lib.id,
        "G2_FOLDER",
        mine_lib.id,
        "G2_SAME_COPY",
    );
    let same_copy_preview = folder::preview(&db, &same_copy_request).unwrap();
    assert!(!same_copy_preview.cross_volume);
    let same_copy = folder::execute(&db, &same_copy_request, "G2 같은 볼륨 복사").unwrap();
    assert_eq!((same_copy.completed, same_copy.failed), (1, 0));
    assert_eq!(
        sha256(&mine.join("G2_SAME_COPY/중첩/G2_FOLDER.jpg")),
        original_sha
    );
    let same_copy_undo = folder::undo(&db, same_copy.batch_id).unwrap();
    assert_eq!((same_copy_undo.moved, same_copy_undo.failed), (1, 0));
    assert!(!mine.join("G2_SAME_COPY").exists());

    // 4. 실제 다른 device의 폴더 복사·이동과 undo.
    let cross_copy_request = folder_request(
        folder::Action::Copy,
        mine_lib.id,
        "G2_FOLDER",
        cross_lib.id,
        "G2_CROSS_COPY",
    );
    let cross_copy_preview = folder::preview(&db, &cross_copy_request).unwrap();
    assert!(cross_copy_preview.cross_volume);
    let cross_copy = folder::execute(&db, &cross_copy_request, "G2 cross-volume 복사").unwrap();
    assert_eq!((cross_copy.completed, cross_copy.failed), (1, 0));
    let cross_copy_path = cross_library.join("G2_CROSS_COPY/중첩/G2_FOLDER.jpg");
    assert_eq!(sha256(&cross_copy_path), original_sha);
    let cross_copy_undo = folder::undo(&db, cross_copy.batch_id).unwrap();
    assert_eq!((cross_copy_undo.moved, cross_copy_undo.failed), (1, 0));
    assert!(!cross_library.join("G2_CROSS_COPY").exists());

    let cross_move_request = folder_request(
        folder::Action::Move,
        mine_lib.id,
        "G2_FOLDER",
        cross_lib.id,
        "G2_CROSS_MOVE",
    );
    let cross_move_preview = folder::preview(&db, &cross_move_request).unwrap();
    assert!(cross_move_preview.cross_volume);
    let cross_move = folder::execute(&db, &cross_move_request, "G2 cross-volume 이동").unwrap();
    assert_eq!((cross_move.completed, cross_move.failed), (1, 0));
    let cross_move_path = cross_library.join("G2_CROSS_MOVE/중첩/G2_FOLDER.jpg");
    assert_eq!(sha256(&cross_move_path), original_sha);
    let cross_move_undo = folder::undo(&db, cross_move.batch_id).unwrap();
    assert_eq!((cross_move_undo.moved, cross_move_undo.failed), (1, 0));
    assert_eq!(sha256(&folder_source), original_sha);
    assert!(mine.join("G2_FOLDER/중첩/G2_FOLDER.xmp").is_file());

    let evidence = serde_json::json!({
        "source": { "sha256": original_sha, "bytes": original_size },
        "devices": { "same": same_device, "cross": cross_device },
        "capture_date": {
            "batch": capture.batch_id,
            "before_sha256": capture_manifest.before_sha256,
            "write_sha256": capture_manifest.after_sha256,
            "rescan_at": capture_manifest.rescan_at,
            "undo_sha256": sha256(&publish_source),
            "undo_moved": capture_undo.moved
        },
        "publish": {
            "batch": first_publish.batch_id,
            "first_completed": first_publish.completed,
            "second_completed": second_publish.completed,
            "second_already_published": second_publish.already_published,
            "undo_removed": publish_undo.moved,
            "source_retained": publish_source.is_file()
        },
        "same_volume": {
            "move_batch": same_move.batch_id,
            "move_manifest": same_move.manifest_sha256,
            "move_undo": same_move_undo.moved,
            "copy_batch": same_copy.batch_id,
            "copy_manifest": same_copy.manifest_sha256,
            "copy_undo": same_copy_undo.moved
        },
        "cross_volume": {
            "copy_batch": cross_copy.batch_id,
            "copy_manifest": cross_copy.manifest_sha256,
            "copy_undo": cross_copy_undo.moved,
            "move_batch": cross_move.batch_id,
            "move_manifest": cross_move.manifest_sha256,
            "move_undo": cross_move_undo.moved
        },
        "failures": []
    });
    assert_eq!(
        sha256(&source),
        original_sha,
        "운영 원본은 읽기 전용으로 유지"
    );
    for _ in 0..5 {
        folder::remove_tree(&cross).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !cross.exists() {
            break;
        }
    }
    assert!(!cross.exists());
    println!(
        "G2_EVIDENCE={}",
        serde_json::to_string_pretty(&evidence).unwrap()
    );
}
