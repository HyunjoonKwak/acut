use rusqlite::{params, Connection};

use crate::db::queries::{UndoBatch, UndoEntry};

pub fn create_batch(
    _conn: &Connection,
    batch_id: &str,
    batch_name: &str,
) -> Result<(), rusqlite::Error> {
    // Batch is implicit via batch_id in journal entries
    // This is a placeholder to validate the batch_id format
    log::info!("Creating undo batch: {} ({})", batch_id, batch_name);
    Ok(())
}

pub fn record_operation(
    conn: &Connection,
    batch_id: &str,
    batch_name: &str,
    sequence: i32,
    operation: &str,
    source_path: Option<&str>,
    target_path: Option<&str>,
    metadata_json: Option<&str>,
) -> Result<String, rusqlite::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    conn.execute(
        "INSERT INTO undo_journal
         (id, batch_id, batch_name, sequence, operation, source_path, target_path,
          metadata_json, status, executed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'executed', ?9)",
        params![id, batch_id, batch_name, sequence, operation, source_path, target_path, metadata_json, now],
    )?;

    Ok(id)
}

pub fn get_batches(conn: &Connection, limit: i64) -> Result<Vec<UndoBatch>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT batch_id, batch_name
         FROM undo_journal
         WHERE status = 'executed'
         ORDER BY executed_at DESC
         LIMIT ?1",
    )?;

    let batch_infos: Vec<(String, Option<String>)> = stmt
        .query_map(params![limit], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut batches = Vec::new();

    for (batch_id, batch_name) in batch_infos {
        let entries = get_batch_entries(conn, &batch_id)?;
        batches.push(UndoBatch {
            batch_id,
            batch_name,
            entries,
        });
    }

    Ok(batches)
}

pub fn get_batch_entries(
    conn: &Connection,
    batch_id: &str,
) -> Result<Vec<UndoEntry>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, sequence, operation, source_path, target_path, status, executed_at
         FROM undo_journal
         WHERE batch_id = ?1
         ORDER BY sequence ASC",
    )?;

    let entries = stmt
        .query_map(params![batch_id], |row| {
            Ok(UndoEntry {
                id: row.get(0)?,
                sequence: row.get(1)?,
                operation: row.get(2)?,
                source_path: row.get(3)?,
                target_path: row.get(4)?,
                status: row.get(5)?,
                executed_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(entries)
}

pub fn mark_batch_undone(conn: &Connection, batch_id: &str) -> Result<(), rusqlite::Error> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    conn.execute(
        "UPDATE undo_journal SET status = 'undone', undone_at = ?1 WHERE batch_id = ?2 AND status = 'executed'",
        params![now, batch_id],
    )?;
    Ok(())
}
