#![allow(dead_code)]

mod ai;
mod api;
pub mod ops;
mod commands;
mod core;
mod cull;
mod db;
mod media;
mod nas;
mod scan;

use std::sync::Arc;
use tokio::sync::RwLock;

use core::config::AppConfig;
use db::Database;
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

            let database =
                Database::new(&app_data_dir).expect("Failed to initialize database");
            app.manage(Arc::new(database));

            // v2 — 재설계된 데이터 계층. 기존 것과 별도 파일을 쓴다.
            let v2 = db::conn::Db::open(app_data_dir.join("acut-v2.db"))
                .expect("v2 데이터베이스를 열 수 없습니다");
            app.manage(api::AppState::new(v2, app_data_dir.clone()));
            app.manage(commands::nas::NasState::default());

            // Load config
            let config = AppConfig::load(&app_data_dir.join("config.yaml"));
            app.manage(Arc::new(RwLock::new(config)));

            // Initialize watcher manager
            let watcher_manager = core::watcher::WatcherManager::new();
            app.manage(Arc::new(watcher_manager));

            // Initialize scheduler
            let scheduler = core::scheduler::SchedulerManager::new();
            app.manage(Arc::new(scheduler));

            // Initialize MCP server
            let db_arc: Arc<Database> = app.state::<Arc<Database>>().inner().clone();
            let mcp_server = core::mcp::McpServer::new(&app_data_dir, db_arc);
            app.manage(Arc::new(mcp_server));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // ── v2 (재설계) ──────────────────────────────────
            api::libraries_list,
            api::library_add,
            api::library_remove,
            api::library_stats,
            api::cache_usage,
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
            commands::scan::scan_directory,
            commands::scan::cancel_scan,
            commands::scan::process_phase1,
            commands::scan::generate_thumbnails_for,
            commands::folders::add_source_folder,
            commands::folders::get_source_folders,
            commands::folders::remove_source_folder,
            commands::folders::reset_library,
            commands::folders::update_folder_scan_time,
            commands::media::get_media_list,
            commands::media::get_media_stats,
            commands::media::get_date_groups,
            commands::media::get_folder_groups,
            commands::media::get_preview_image,
            commands::media::get_preview_video_frame,
            commands::media::get_thumbnail,
            commands::duplicate::detect_duplicates,
            commands::duplicate::get_duplicate_groups,
            commands::duplicate::open_file,
            commands::duplicate::preview_file,
            commands::duplicate::set_preferred_member,
            commands::duplicate::dismiss_duplicate_group,
            commands::duplicate::trash_duplicate_files,
            commands::duplicate::trash_group_duplicates,
            commands::fileops::list_directory,
            commands::fileops::move_files,
            commands::fileops::copy_files,
            commands::fileops::create_directory,
            commands::fileops::analyze_folder,
            commands::fileops::get_folder_tree,
            commands::bcut::detect_bcuts,
            commands::bcut::get_bcut_groups,
            commands::bcut::set_bcut_best,
            commands::bcut::dismiss_bcut_group,
            commands::bcut::trash_bcut_files,
            commands::bcut::compute_quality_scores,
            commands::fileops::trash_review_files,
            commands::fileops::scan_date_folders,
            commands::fileops::rename_date_folders,
            // Folder-level operations (A-Cut)
            commands::fileops::copy_directory,
            commands::fileops::move_paths,
            commands::fileops::rename_path,
            commands::fileops::trash_paths,
            // Comments (A-Cut inspector)
            commands::media::set_media_comment,
            commands::media::get_media_comments,
            // Workbench pipeline stats
            commands::media::get_workbench_stats,
            commands::organize::preview_organize,
            commands::organize::execute_organize,
            commands::undo::get_undo_history,
            commands::undo::undo_batch,
            // Phase 0B: Dry-run previews
            commands::duplicate::preview_trash_duplicates,
            commands::bcut::preview_trash_bcuts,
            // Phase 1A: Config
            commands::config::get_config,
            commands::config::update_config,
            commands::config::reset_config,
            // Phase 1C: Watch
            commands::watch::start_watch,
            commands::watch::stop_watch,
            commands::watch::get_watch_status,
            // Phase 1D: Schedule
            commands::schedule::get_schedules,
            commands::schedule::add_schedule,
            commands::schedule::remove_schedule,
            commands::schedule::toggle_schedule,
            commands::schedule::get_schedule_runs,
            // Phase 2A: Sync
            commands::sync::preview_sync,
            commands::sync::execute_sync,
            commands::sync::cancel_sync,
            commands::sync::get_sync_presets,
            commands::sync::save_sync_preset,
            commands::sync::delete_sync_preset,
            commands::sync::get_sync_history,
            // Phase 2C: Volume
            commands::volume::get_mounted_volumes,
            commands::volume::start_volume_monitoring,
            commands::volume::stop_volume_monitoring,
            commands::volume::eject_volume,
            // Phase 3A: MCP
            commands::mcp::start_mcp_server,
            commands::mcp::stop_mcp_server,
            commands::mcp::get_mcp_status,
            // Tags
            commands::tags::get_tags,
            commands::tags::create_tag,
            commands::tags::delete_tag,
            commands::tags::tag_media,
            commands::tags::untag_media,
            commands::tags::get_media_tags,
            // Albums
            commands::albums::get_albums,
            commands::albums::create_album,
            commands::albums::delete_album,
            commands::albums::add_media_to_album,
            commands::albums::remove_media_from_album,
            commands::albums::get_album_media,
            commands::albums::get_media_albums,
            // Search
            commands::media::search_media,
            // GPS
            commands::media::get_gps_media,
            // NAS upload
            commands::nas::nas_get_config,
            commands::nas::nas_connect,
            commands::nas::nas_disconnect,
            commands::nas::nas_status,
            commands::nas::nas_list_folders,
            commands::nas::nas_create_folder,
            commands::nas::nas_upload,
            commands::nas::nas_cancel_upload,
            commands::nas::nas_uploaded_media_ids,
            // App update
            commands::update::check_for_update,
            commands::update::download_update,
            commands::update::open_release_page,
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
