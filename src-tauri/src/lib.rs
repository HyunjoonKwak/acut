// v1 코드는 src-tauri/legacy/ 로 빠졌다. core(sync·hasher)와 nas는 아직
// 안 쓰지만 3·5단계에서 붙이므로 남긴다 — 그래서 dead_code를 허용한다.
#![allow(dead_code)]

mod api;
mod core;
mod cull;
mod db;
mod media;
mod nas;
pub mod ops;
mod scan;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .register_asynchronous_uri_scheme_protocol("thumb", |ctx, req, responder| {
            api::thumb_protocol::handle(ctx.app_handle(), req, responder);
        })
        .register_asynchronous_uri_scheme_protocol("video", |ctx, req, responder| {
            // 원본을 Range로 잘라 보낸다. 파일이 커서 반드시 별도 스레드로.
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || api::video_protocol::handle(&app, req, responder));
        })
        .register_asynchronous_uri_scheme_protocol("photo", |ctx, req, responder| {
            // 미리보기를 그 자리에서 만들 수 있으므로 블로킹이다. 별도 스레드로.
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || api::photo_protocol::handle(&app, req, responder));
        })
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Initialize database
            let app_data_dir = app
                .handle()
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            // One-time migration from the legacy bundle identifier
            migrate_legacy_app_data(&app_data_dir);

            let v2 = db::conn::Db::open(app_data_dir.join("acut-v2.db"))
                .expect("데이터베이스를 열 수 없습니다");
            app.manage(api::AppState::new(v2, app_data_dir.clone()));

            // 하루에 한 벌 — 판정·평점·태그가 든 파일이 하나뿐이라 잃으면 끝이다.
            // 별도 스레드에서. 8만 장 DB라도 몇 초지만 첫 화면을 막을 이유가 없다.
            {
                let state = app.state::<api::AppState>();
                let db = std::sync::Arc::clone(&state.db);
                let dir = app_data_dir.join("backups");
                std::thread::spawn(move || {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    match db::backup::make_if_stale(&db, &dir, now) {
                        Ok(Some(b)) => log::info!("자동 백업 {}", b.name),
                        Ok(None) => {}
                        Err(e) => log::warn!("자동 백업 실패: {e}"),
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // ── v2 (재설계) ──────────────────────────────────
            api::libraries_list,
            api::library_add,
            api::library_remove,
            api::library_stats,
            api::import_preview,
            api::import_run,
            api::settings_get,
            api::settings_set,
            api::settings_remove,
            api::db_backup,
            api::db_backups,
            api::db_restore,
            api::db_backups_reveal,
            api::open_in_default_app,
            api::cache_usage,
            api::cache_clear,
            api::cache_migrate,
            api::trash::trash_pending,
            api::trash::trash_summary,
            api::trash::trash_apply,
            api::trash::trash_files,
            api::trash::trash_restore,
            api::trash::trash_empty,
            api::organize::organize_suggest,
            api::organize::organize_date,
            api::organize::organize_preview,
            api::organize::organize_move,
            api::organize::batches_recent,
            api::organize::batch_undo,
            api::volumes_list,
            api::scan_start,
            api::scan_cancel,
            api::files_page,
            api::files_summary,
            api::files_facets,
            api::smart::smart_list,
            api::smart::smart_save,
            api::smart::smart_delete,
            api::tags::tags_list,
            api::tags::tags_of,
            api::tags::tag_add,
            api::tags::tag_remove,
            api::tags::tag_delete,
            api::files_timeline,
            api::files_cursor_at,
            api::files_mark,
            api::file_detail,
            api::reveal_in_finder,
            api::folders_list,
            api::cull::cull_scan,
            api::cull::cull_groups,
            api::cull::cull_members,
            api::cull::cull_set_best,
            api::cull::cull_apply,
            api::cull::cull_skip,
            api::cull::cull_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// One-time migration of app data from the legacy bundle identifier
/// (`com.smartcategory.media`) to the current one (`com.acut.media`).
/// Runs before DB init: if the new data dir has no database yet and the
/// legacy dir exists, move everything over (copy as fallback).
fn migrate_legacy_app_data(new_dir: &std::path::Path) {
    const LEGACY_ID: &str = "com.smartcategory.media";

    if new_dir.join("smart_category.db").exists() {
        return;
    }
    let Some(parent) = new_dir.parent() else {
        return;
    };
    let old_dir = parent.join(LEGACY_ID);
    if !old_dir.is_dir() || old_dir == new_dir {
        return;
    }

    let is_empty = match std::fs::read_dir(new_dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => true, // new dir does not exist yet
    };
    if !is_empty {
        return;
    }

    let _ = std::fs::remove_dir(new_dir);
    match std::fs::rename(&old_dir, new_dir) {
        Ok(()) => {
            log::info!(
                "Migrated legacy app data: {} -> {}",
                old_dir.display(),
                new_dir.display()
            );
        }
        Err(e) => {
            log::warn!("Legacy data rename failed ({e}); copying instead");
            if let Err(e) = copy_dir_recursive(&old_dir, new_dir) {
                log::error!("Legacy data copy failed: {e}");
            }
        }
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
