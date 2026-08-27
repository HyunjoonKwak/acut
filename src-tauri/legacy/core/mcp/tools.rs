use std::sync::Arc;

use rusqlite::params;
use serde_json::{json, Value};

use crate::db::Database;

/// Describes a single tool that can be invoked through the MCP protocol.
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Return the list of all tools exposed by the Smart Folder MCP server.
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "scan".into(),
            description: "Trigger a directory scan to index media files".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "directory": {
                        "type": "string",
                        "description": "Absolute path to the directory to scan"
                    }
                },
                "required": ["directory"]
            }),
        },
        ToolDefinition {
            name: "get_stats".into(),
            description: "Get statistics about the indexed media library".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "detect_duplicates".into(),
            description: "Run duplicate detection across the indexed media library".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "organize".into(),
            description: "Organize files into a target directory using the specified strategy".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target_dir": {
                        "type": "string",
                        "description": "Absolute path to the target directory"
                    },
                    "strategy": {
                        "type": "string",
                        "description": "Organization strategy (e.g. 'date', 'type', 'event')"
                    }
                },
                "required": ["target_dir", "strategy"]
            }),
        },
        ToolDefinition {
            name: "sync".into(),
            description: "Synchronize files from a source directory to a target directory".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "Absolute path to the source directory"
                    },
                    "target": {
                        "type": "string",
                        "description": "Absolute path to the target directory"
                    }
                },
                "required": ["source", "target"]
            }),
        },
        ToolDefinition {
            name: "get_media_list".into(),
            description: "List indexed media files with pagination".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "number",
                        "description": "Maximum number of items to return"
                    },
                    "offset": {
                        "type": "number",
                        "description": "Number of items to skip"
                    }
                },
                "required": ["limit", "offset"]
            }),
        },
    ]
}

/// Dispatch a tool call by name, using the shared database for real queries.
pub async fn handle_tool_call(name: &str, params: &Value, db: &Arc<Database>) -> Result<Value, String> {
    match name {
        "scan" => {
            let directory = params
                .get("directory")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            log::info!("[MCP:tools] scan requested for directory: {}", directory);
            Ok(json!({
                "status": "ok",
                "message": format!("Use Tauri command scan_directory for scanning. Requested path: '{}'", directory)
            }))
        }
        "get_stats" => {
            log::info!("[MCP:tools] get_stats requested");
            let stats = db.with_conn(|conn| {
                let total: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM media_files",
                    [],
                    |row| row.get(0),
                )?;
                let size: i64 = conn.query_row(
                    "SELECT COALESCE(SUM(file_size), 0) FROM media_files",
                    [],
                    |row| row.get(0),
                )?;
                let image_count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM media_files WHERE media_type = 'image'",
                    [],
                    |row| row.get(0),
                )?;
                let video_count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM media_files WHERE media_type = 'video'",
                    [],
                    |row| row.get(0),
                )?;
                Ok(json!({
                    "total": total,
                    "total_size": size,
                    "image_count": image_count,
                    "video_count": video_count,
                }))
            }).map_err(|e| format!("DB error: {}", e))?;
            Ok(json!({ "status": "ok", "stats": stats }))
        }
        "detect_duplicates" => {
            log::info!("[MCP:tools] detect_duplicates requested");
            Ok(json!({
                "status": "ok",
                "message": "Use Tauri command detect_duplicates"
            }))
        }
        "organize" => {
            let target_dir = params
                .get("target_dir")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let strategy = params
                .get("strategy")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            log::info!(
                "[MCP:tools] organize requested - target: {}, strategy: {}",
                target_dir,
                strategy
            );
            Ok(json!({
                "status": "ok",
                "message": "Use Tauri command preview_organize/execute_organize"
            }))
        }
        "sync" => {
            let source = params
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target = params
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            log::info!(
                "[MCP:tools] sync requested - source: {}, target: {}",
                source,
                target
            );
            Ok(json!({
                "status": "ok",
                "message": "Use Tauri command preview_sync/execute_sync"
            }))
        }
        "get_media_list" => {
            let limit = params
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(20);
            let offset = params
                .get("offset")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            log::info!(
                "[MCP:tools] get_media_list requested - limit: {}, offset: {}",
                limit,
                offset
            );

            let files = db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, file_path, file_name, file_size, media_type, scan_phase
                     FROM media_files ORDER BY modified_at DESC LIMIT ?1 OFFSET ?2"
                )?;
                let rows = stmt.query_map(params![limit, offset], |row| {
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "file_path": row.get::<_, String>(1)?,
                        "file_name": row.get::<_, String>(2)?,
                        "file_size": row.get::<_, i64>(3)?,
                        "media_type": row.get::<_, String>(4)?,
                        "scan_phase": row.get::<_, i32>(5)?,
                    }))
                })?.collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }).map_err(|e| format!("DB error: {}", e))?;

            Ok(json!({ "status": "ok", "files": files }))
        }
        _ => Err(format!("Unknown tool: {}", name)),
    }
}
