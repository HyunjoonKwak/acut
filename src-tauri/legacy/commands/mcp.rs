use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::core::mcp::McpServer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStatus {
    pub running: bool,
    pub socket_path: String,
}

#[tauri::command]
pub async fn start_mcp_server(
    mcp: State<'_, Arc<McpServer>>,
) -> Result<(), String> {
    mcp.start().await
}

#[tauri::command]
pub async fn stop_mcp_server(
    mcp: State<'_, Arc<McpServer>>,
) -> Result<(), String> {
    mcp.stop().await;
    Ok(())
}

#[tauri::command]
pub async fn get_mcp_status(
    mcp: State<'_, Arc<McpServer>>,
) -> Result<McpStatus, String> {
    Ok(McpStatus {
        running: mcp.is_running().await,
        socket_path: mcp.socket_path(),
    })
}
