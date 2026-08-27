use rusqlite::Connection;

pub fn run(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS media_files (
            id TEXT PRIMARY KEY,
            file_path TEXT NOT NULL UNIQUE,
            original_path TEXT NOT NULL,
            file_name TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            mime_type TEXT,
            sha256_hash TEXT,
            quick_hash TEXT,
            phash BLOB,
            width INTEGER,
            height INTEGER,
            media_type TEXT NOT NULL DEFAULT 'image',
            created_at TEXT NOT NULL,
            modified_at TEXT NOT NULL,
            scanned_at TEXT NOT NULL,
            source_type TEXT NOT NULL DEFAULT 'local',
            thumbnail TEXT,
            scan_phase INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS media_exif (
            media_id TEXT PRIMARY KEY REFERENCES media_files(id) ON DELETE CASCADE,
            date_taken TEXT,
            camera_make TEXT,
            camera_model TEXT,
            lens_model TEXT,
            focal_length REAL,
            aperture REAL,
            iso INTEGER,
            gps_latitude REAL,
            gps_longitude REAL,
            gps_altitude REAL,
            orientation INTEGER
        );

        CREATE TABLE IF NOT EXISTS tags (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            category TEXT NOT NULL DEFAULT 'user',
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS media_tags (
            media_id TEXT NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
            tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            confidence REAL DEFAULT 1.0,
            source TEXT NOT NULL DEFAULT 'user',
            PRIMARY KEY (media_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS albums (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            cover_media_id TEXT REFERENCES media_files(id) ON DELETE SET NULL,
            auto_generated INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS album_media (
            album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
            media_id TEXT NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
            sort_order INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (album_id, media_id)
        );

        CREATE TABLE IF NOT EXISTS duplicate_groups (
            id TEXT PRIMARY KEY,
            match_type TEXT NOT NULL,
            similarity_score REAL,
            status TEXT NOT NULL DEFAULT 'pending'
        );

        CREATE TABLE IF NOT EXISTS duplicate_members (
            group_id TEXT NOT NULL REFERENCES duplicate_groups(id) ON DELETE CASCADE,
            media_id TEXT NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
            is_preferred INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (group_id, media_id)
        );

        CREATE TABLE IF NOT EXISTS classification_rules (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            conditions_json TEXT NOT NULL,
            action_type TEXT NOT NULL,
            action_value TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS source_folders (
            id TEXT PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            added_at TEXT NOT NULL,
            last_scanned_at TEXT
        );

        CREATE TABLE IF NOT EXISTS undo_journal (
            id TEXT PRIMARY KEY,
            batch_id TEXT NOT NULL,
            batch_name TEXT,
            sequence INTEGER NOT NULL,
            operation TEXT NOT NULL,
            source_path TEXT,
            target_path TEXT,
            metadata_json TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            executed_at TEXT,
            undone_at TEXT
        );

        CREATE TABLE IF NOT EXISTS bcut_groups (
            id TEXT PRIMARY KEY,
            group_reason TEXT NOT NULL DEFAULT 'time',
            member_count INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'pending'
        );

        CREATE TABLE IF NOT EXISTS bcut_members (
            group_id TEXT NOT NULL REFERENCES bcut_groups(id) ON DELETE CASCADE,
            media_id TEXT NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
            quality_score REAL NOT NULL DEFAULT 0,
            sharpness_score REAL NOT NULL DEFAULT 0,
            exposure_score REAL NOT NULL DEFAULT 0,
            is_best INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (group_id, media_id)
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS nas_uploads (
            id TEXT PRIMARY KEY,
            media_id TEXT,
            sha256 TEXT,
            source_path TEXT NOT NULL,
            remote_path TEXT NOT NULL,
            uploaded_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_nas_uploads_sha256 ON nas_uploads(sha256);
        CREATE INDEX IF NOT EXISTS idx_nas_uploads_media ON nas_uploads(media_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_nas_uploads_unique ON nas_uploads(media_id, remote_path);

        CREATE INDEX IF NOT EXISTS idx_media_sha256 ON media_files(sha256_hash);
        CREATE INDEX IF NOT EXISTS idx_media_phash ON media_files(phash);
        CREATE INDEX IF NOT EXISTS idx_media_quick_hash ON media_files(quick_hash);
        CREATE INDEX IF NOT EXISTS idx_media_file_size ON media_files(file_size);
        CREATE INDEX IF NOT EXISTS idx_media_path ON media_files(file_path);
        CREATE INDEX IF NOT EXISTS idx_media_incremental ON media_files(file_path, modified_at, file_size);
        CREATE INDEX IF NOT EXISTS idx_media_scan_phase ON media_files(scan_phase);
        CREATE INDEX IF NOT EXISTS idx_exif_date ON media_exif(date_taken);
        CREATE INDEX IF NOT EXISTS idx_exif_gps ON media_exif(gps_latitude, gps_longitude);
        CREATE INDEX IF NOT EXISTS idx_undo_batch ON undo_journal(batch_id);
        CREATE INDEX IF NOT EXISTS idx_media_type ON media_files(media_type);

        -- Watch activity log (Phase 1C)
        CREATE TABLE IF NOT EXISTS watch_activity_log (
            id TEXT PRIMARY KEY,
            folder_path TEXT NOT NULL,
            event_type TEXT NOT NULL,
            file_path TEXT,
            detected_at TEXT NOT NULL,
            processed INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_watch_activity ON watch_activity_log(detected_at DESC);

        -- Schedules (Phase 1D)
        CREATE TABLE IF NOT EXISTS schedules (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            cron_expression TEXT NOT NULL,
            task_type TEXT NOT NULL,
            task_params_json TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            last_run_at TEXT,
            next_run_at TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS schedule_runs (
            id TEXT PRIMARY KEY,
            schedule_id TEXT NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            status TEXT NOT NULL DEFAULT 'running',
            result_json TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_schedule_runs ON schedule_runs(schedule_id, started_at DESC);

        -- Sync history (Phase 2A)
        CREATE TABLE IF NOT EXISTS sync_history (
            id TEXT PRIMARY KEY,
            preset_id TEXT,
            source_dir TEXT NOT NULL,
            target_dir TEXT NOT NULL,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            files_copied INTEGER NOT NULL DEFAULT 0,
            files_updated INTEGER NOT NULL DEFAULT 0,
            files_skipped INTEGER NOT NULL DEFAULT 0,
            bytes_transferred INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'running',
            error_message TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_sync_history ON sync_history(started_at DESC);

        CREATE TABLE IF NOT EXISTS sync_file_checksums (
            id TEXT PRIMARY KEY,
            sync_history_id TEXT,
            file_path TEXT NOT NULL,
            xxhash64 TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            modified_at TEXT NOT NULL,
            synced_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sync_checksum_path ON sync_file_checksums(file_path);

        -- Known devices (Phase 2C)
        CREATE TABLE IF NOT EXISTS known_devices (
            uuid TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            last_mount_point TEXT,
            associated_source_folder TEXT,
            first_seen_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL
        );

        -- Migration: add columns if missing (for existing DBs)
        -- SQLite doesn't support IF NOT EXISTS for ALTER TABLE, so we ignore errors
        ",
    )?;

    // Safe column additions for existing databases
    let _ = conn.execute_batch("ALTER TABLE media_files ADD COLUMN scan_phase INTEGER NOT NULL DEFAULT 0");
    let _ = conn.execute_batch("ALTER TABLE media_files ADD COLUMN thumbnail TEXT");
    // User comment on a photo/video (A-Cut inspector)
    let _ = conn.execute_batch("ALTER TABLE media_files ADD COLUMN comment TEXT");
    // Remove old column data if migrating
    let _ = conn.execute_batch("UPDATE media_files SET thumbnail = NULL WHERE thumbnail IS NULL AND scan_phase = 0");

    // Migrate legacy inline base64 thumbnails to the file-based cache:
    // clear them and reset scan_phase so Phase 1 regenerates thumbnail files.
    // Cache paths always end in '.jpg'; the base64 alphabet has no '.'
    // (a leading-'/' check would fail — base64 JPEG starts with "/9j/").
    // Idempotent: already-migrated rows end in '.jpg' and are untouched.
    let _ = conn.execute_batch(
        "UPDATE media_files SET thumbnail = NULL, scan_phase = 0
         WHERE thumbnail IS NOT NULL AND thumbnail NOT LIKE '%.jpg'",
    );

    Ok(())
}
