use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::db::Database;

pub mod tools;

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// ---------------------------------------------------------------------------
// McpServer
// ---------------------------------------------------------------------------

pub struct McpServer {
    socket_path: PathBuf,
    running: Arc<Mutex<bool>>,
    db: Arc<Database>,
}

impl McpServer {
    /// Create a new MCP server. The Unix socket will be placed at
    /// `{app_data_dir}/smart-folder.sock`.
    pub fn new(app_data_dir: &Path, db: Arc<Database>) -> Self {
        Self {
            socket_path: app_data_dir.join("smart-folder.sock"),
            running: Arc::new(Mutex::new(false)),
            db,
        }
    }

    /// Start listening on the Unix domain socket.
    ///
    /// Spawns a background task that accepts connections and dispatches
    /// JSON-RPC requests.
    pub async fn start(&self) -> Result<(), String> {
        // Remove stale socket if it exists
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)
                .map_err(|e| format!("Failed to remove stale socket: {}", e))?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .map_err(|e| format!("Failed to bind Unix socket: {}", e))?;

        {
            let mut running = self.running.lock().await;
            *running = true;
        }

        let running = Arc::clone(&self.running);
        let socket_path = self.socket_path.clone();
        let db = Arc::clone(&self.db);

        log::info!("[MCP] server starting on {:?}", socket_path);

        tokio::spawn(async move {
            loop {
                // Check the running flag before each accept
                {
                    let r = running.lock().await;
                    if !*r {
                        break;
                    }
                }

                let accept_result = tokio::select! {
                    res = listener.accept() => Some(res),
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => None,
                };

                let (stream, _addr) = match accept_result {
                    Some(Ok(conn)) => conn,
                    Some(Err(e)) => {
                        log::error!("[MCP] accept error: {}", e);
                        continue;
                    }
                    None => continue, // timeout – re-check running flag
                };

                let running_inner = Arc::clone(&running);
                let db_inner = Arc::clone(&db);
                tokio::spawn(async move {
                    Self::handle_connection(stream, running_inner, db_inner).await;
                });
            }

            log::info!("[MCP] accept loop exited");
        });

        Ok(())
    }

    /// Stop the server and clean up the socket file.
    pub async fn stop(&self) {
        {
            let mut running = self.running.lock().await;
            *running = false;
        }

        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }

        log::info!("[MCP] server stopped");
    }

    /// Returns `true` if the server accept loop is running.
    pub async fn is_running(&self) -> bool {
        let guard = self.running.lock().await;
        *guard
    }

    /// Path to the Unix domain socket.
    pub fn socket_path(&self) -> String {
        self.socket_path.to_string_lossy().into_owned()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    async fn handle_connection(
        stream: tokio::net::UnixStream,
        running: Arc<Mutex<bool>>,
        db: Arc<Database>,
    ) {
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        while let Ok(Some(line)) = lines.next_line().await {
            {
                let r = running.lock().await;
                if !*r {
                    break;
                }
            }

            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            let response = Self::dispatch(&line, &db).await;
            let mut payload = serde_json::to_string(&response).unwrap_or_default();
            payload.push('\n');

            if writer.write_all(payload.as_bytes()).await.is_err() {
                break;
            }
        }
    }

    async fn dispatch(raw: &str, db: &Arc<Database>) -> JsonRpcResponse {
        let request: JsonRpcRequest = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                    }),
                };
            }
        };

        let id = request.id.clone();

        match request.method.as_str() {
            "initialize" => Self::handle_initialize(id),
            "tools/list" => Self::handle_tools_list(id),
            "tools/call" => Self::handle_tools_call(id, &request.params, db).await,
            _ => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                }),
            },
        }
    }

    fn handle_initialize(id: Option<Value>) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "smart-folder-mcp",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "tools": {}
                }
            })),
            error: None,
        }
    }

    fn handle_tools_list(id: Option<Value>) -> JsonRpcResponse {
        let defs = tools::get_tool_definitions();
        let list: Vec<Value> = defs
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                })
            })
            .collect();

        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({ "tools": list })),
            error: None,
        }
    }

    async fn handle_tools_call(
        id: Option<Value>,
        params: &Option<Value>,
        db: &Arc<Database>,
    ) -> JsonRpcResponse {
        let (tool_name, arguments) = match params {
            Some(p) => {
                let name = p
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = p
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                (name, args)
            }
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Missing params for tools/call".into(),
                    }),
                };
            }
        };

        match tools::handle_tool_call(&tool_name, &arguments, db).await {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: Some(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string(&value).unwrap_or_default()
                    }]
                })),
                error: None,
            },
            Err(msg) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32000,
                    message: msg,
                }),
            },
        }
    }
}
