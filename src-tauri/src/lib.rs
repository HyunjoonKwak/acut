// v1 코드는 src-tauri/legacy/ 로 빠졌다. core(sync·hasher)와 nas는 아직
// 안 쓰지만 3·5단계에서 붙이므로 남긴다 — 그래서 dead_code를 허용한다.
#![allow(dead_code)]

mod ai;
mod api;
mod core;
mod cull;
mod db;
mod geo;
mod media;
mod nas;
pub mod ops;
mod scan;

#[cfg(test)]
mod g2_pilot;

/// 프로세스가 시작된 순간 — 시작 시간을 재는 기준. run()의 첫 줄에서 박힌다.
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn started() -> std::time::Instant {
    *STARTED.get_or_init(std::time::Instant::now)
}

/// 웹뷰가 페이지를 읽기 시작한·끝낸 시각(ms, run() 기준) — 시작 시간을 나눠 보기 위해
pub static PAGE_MS: [std::sync::atomic::AtomicU64; 2] =
    [std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0)];

/// 네이티브 구간 표식(ms, run() 기준). 웹뷰가 페이지를 읽기 시작하기까지 2초가
/// 갔는데(실측 2026-09-01: DB 871ms → 페이지 시작 2,877ms) `db_ms` 하나로는
/// 어디서 갔는지 볼 수 없었다. 이름은 [`NATIVE_LABELS`] 와 자리를 맞춘다.
pub static NATIVE_MS: [std::sync::atomic::AtomicU64; NATIVE_LABELS.len()] =
    [const { std::sync::atomic::AtomicU64::new(0) }; NATIVE_LABELS.len()];

pub const NATIVE_LABELS: [&str; 5] = ["setup", "migrated", "db", "state", "setup_done"];

pub fn native_mark(slot: usize) {
    NATIVE_MS[slot].store(
        started().elapsed().as_millis() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
}

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = started();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
        .on_page_load(|_webview, payload| {
            let ms = started().elapsed().as_millis() as u64;
            let slot = match payload.event() {
                tauri::webview::PageLoadEvent::Started => 0,
                tauri::webview::PageLoadEvent::Finished => 1,
            };
            PAGE_MS[slot].store(ms, std::sync::atomic::Ordering::Relaxed);
        })
        .setup(|app| {
            native_mark(0);
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
            native_mark(1);

            let v2 = db::conn::Db::open(app_data_dir.join("acut-v2.db"))
                .expect("데이터베이스를 열 수 없습니다");
            native_mark(2);
            app.manage(api::AppState::new(v2, app_data_dir.clone()));
            api::job::set_emitter(app.handle().clone());
            native_mark(3);

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

            // WAL 관리 — 대량 쓰기 뒤 수백 MB 로 남는 일지를 유휴 때 자른다.
            // 작업 스위치가 잡혀 있거나 사용자가 기다리는 중이면 건드리지 않고,
            // 64MB 아래면 할 일이 없다. 실패(읽기가 물고 있음)해도 다음 차례에 다시.
            {
                let state = app.state::<api::AppState>();
                let db = std::sync::Arc::clone(&state.db);
                let running = std::sync::Arc::clone(&state.running);
                std::thread::Builder::new()
                    .name("acut-db-maint".into())
                    .spawn(move || loop {
                        std::thread::sleep(std::time::Duration::from_secs(300));
                        if running.load(std::sync::atomic::Ordering::Acquire)
                            || api::job::waiting().load(std::sync::atomic::Ordering::Acquire)
                            || db.wal_size() < 64 * 1024 * 1024
                        {
                            continue;
                        }
                        let Some(_guard) = api::job::try_start_with(&running, "DB 정리", true) else {
                            continue;
                        };
                        match db.checkpoint_truncate() {
                            Ok(true) => log::info!("WAL 정리 — 일지를 본체에 옮기고 잘랐다"),
                            Ok(false) => log::info!("WAL 정리 미룸 — 읽기가 물고 있다"),
                            Err(e) => log::warn!("WAL 정리 실패: {e}"),
                        }
                    })
                    .expect("DB 유지보수 스레드");
            }

            // 검은 창 감시 — 메모리가 모자라 macOS가 웹뷰 프로세스를 내리면 창이
            // 검게 남는다(실측: 스왑 15GB 상태). 화면의 «살아 있음»이 20초 넘게
            // 끊기면 페이지를 다시 불러온다. 뒷단 작업은 그대로 돈다.
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let mut last_tick = chrono::Utc::now().timestamp();
                    let mut misses = 0u32;
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        let state = handle.state::<api::AppState>();
                        let now = chrono::Utc::now().timestamp();
                        // 잠자기에서 깨면 벽시계가 훌쩍 뛴다 — 그건 화면이 죽은 게 아니다.
                        // macOS 의 `Instant` 는 잠자는 동안 멈춰 있어 그걸로는 못 잡는다 —
                        // 틱 사이 벽시계 차이로 본다. 15초 넘게 뛰었으면 박동을 지금으로 맞추고 넘어간다 (리뷰 H13)
                        let slept = now - last_tick;
                        last_tick = now;
                        if slept > 15 {
                            state.last_beat.store(now, std::sync::atomic::Ordering::Relaxed);
                            misses = 0;
                            continue;
                        }
                        let last = state.last_beat.load(std::sync::atomic::Ordering::Relaxed);
                        misses = if last > 0 && now - last > 20 { misses + 1 } else { 0 };
                        // 두 번 연속 조용해야 죽은 것으로 본다 — 긴 동기 작업 한 번에 새로 부르지 않게
                        if misses >= 2 {
                            if let Some(w) = handle.get_webview_window("main") {
                                log::warn!("화면이 {}초째 조용하다 — 다시 불러온다", now - last);
                                if let Ok(url) = w.url() {
                                    let _ = w.navigate(url);
                                }
                                state.last_beat.store(now, std::sync::atomic::Ordering::Relaxed);
                                misses = 0;
                            }
                        }
                    }
                });
            }
            native_mark(4);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // ── v2 (재설계) ──────────────────────────────────
            api::libraries_list,
            api::library_add,
            api::library_remove,
            api::library_set_area,
            api::library_stats,
            api::import_preview,
            api::import_run,
            api::settings_get,
            api::settings_set,
            api::settings_remove,
            api::db_info,
            api::db_backup,
            api::db_backups,
            api::db_restore,
            api::db_backups_reveal,
            api::open_in_default_app,
            api::file_comment,
            api::file_rename,
            api::watch_set,
            api::files_by_ids,
            api::ai_status,
            api::ai_model_download,
            api::ai_embed_start,
            api::ai_similar,
            api::ai_text_search,
            api::ai_faces_start,
            api::people_list,
            api::map_overview,
            api::map_cells,
            api::folder_size,
            api::folder_offload,
            api::startup_report,
            api::heartbeat,
            api::frontend_log,
            api::video_dates_refresh,
            api::nas::nas_config,
            api::nas::nas_config_set,
            api::nas::nas_check,
            api::nas::nas_probe,
            api::nas::nas_pull_start,
            api::nas::nas_verify,
            api::nas::nas_purge_plan,
            api::nas::nas_purge_run,
            api::nas::xmp_export,
            api::backup::backup_target,
            api::backup::backup_set_target,
            api::backup::backup_plan,
            api::backup::backup_run,
            api::person_rename,
            api::person_merge,
            api::cache_usage,
            api::cache_clear,
            api::cache_migrate,
            api::trash::trash_pending,
            api::trash::trash_summary,
            api::trash::trash_by_library,
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
            api::capture_date::capture_date_audit,
            api::capture_date::capture_date_apply,
            api::transfer::transfer_preview,
            api::transfer::transfer_execute,
            api::folder::folder_operation_preview,
            api::folder::folder_operation_execute,
            api::scan_start,
            api::scan_cancel,
            api::files_page,
            api::files_facets,
            api::update::update_check,
            api::update::update_check_auto,
            api::update::update_open_page,
            api::geo::geo_stats,
            api::geo::geo_fill_start,
            api::smart::smart_list,
            api::smart::smart_save,
            api::smart::smart_delete,
            api::tags::tags_list,
            api::tags::tags_of,
            api::tags::tag_add,
            api::tags::tag_remove,
            api::tags::tag_delete,
            api::files_timeline,
            api::files_summary,
            api::files_cursor_at,
            api::files_mark,
            api::file_detail,
            api::reveal_in_finder,
            api::folders_list,
            api::cull::cull_scan,
            api::cull::cull_scan_kind,
            api::cull::cull_groups,
            api::cull::cull_members,
            api::cull::cull_set_best,
            api::cull::cull_apply,
            api::cull::cull_unapply,
            api::cull::cull_scope_counts,
            api::cull::cull_apply_all,
            api::cull::cull_apply_pair,
            api::cull::cull_folder_pairs_apply,
            api::cull::cull_folder_set_unapply,
            api::cull::cull_folder_pair_photos,
            api::cull::cull_hash_folders,
            api::folder_merge,
            api::folder_leftovers,
            api::husk_list,
            api::husk_trash,
            api::folder_merge_rest,
            api::trash::files_unmark_excluded,
            api::cull::cull_folder_sets,
            api::cull::cull_folder_set_apply,
            api::cull::cull_compare_folders,
            api::folder_by_path,
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
