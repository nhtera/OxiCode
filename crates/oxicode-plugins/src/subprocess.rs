//! Plugin subprocess runner: spawns a plugin process and communicates via JSON-RPC over stdio.
//!
//! Reuses the same stdin/stdout JSON-RPC pattern as the MCP stdio transport.
//! A single `io` lock serializes all request/response pairs to prevent interleaving.

use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use oxicode_common::{OxiError, OxiResult};

/// Default timeout for plugin requests.
const PLUGIN_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// JSON-RPC 2.0 request sent to a plugin.
#[derive(Debug, Serialize)]
struct PluginRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response from a plugin.
#[derive(Debug, Deserialize)]
struct PluginResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<PluginRpcError>,
}

/// JSON-RPC error from a plugin.
#[derive(Debug, Deserialize)]
pub struct PluginRpcError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for PluginRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "plugin RPC error {}: {}", self.code, self.message)
    }
}

/// Internal IO state — locked together to serialize request/response pairs.
struct SubprocessIo {
    stdin: Option<tokio::process::ChildStdin>,
    stdout_lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
}

/// A running plugin subprocess communicating via JSON-RPC over stdio.
pub struct PluginSubprocess {
    io: Arc<Mutex<SubprocessIo>>,
    child: Arc<Mutex<Child>>,
    next_id: AtomicU64,
    plugin_name: String,
}

impl PluginSubprocess {
    /// Spawn a plugin subprocess.
    pub async fn spawn(
        plugin_name: &str,
        command: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> OxiResult<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (k, v) in env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().map_err(|e| {
            OxiError::Other(format!("Failed to spawn plugin '{plugin_name}' ({command}): {e}"))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            OxiError::Other(format!("Plugin '{plugin_name}' stdin not available"))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            OxiError::Other(format!("Plugin '{plugin_name}' stdout not available"))
        })?;

        Ok(Self {
            io: Arc::new(Mutex::new(SubprocessIo {
                stdin: Some(stdin),
                stdout_lines: BufReader::new(stdout).lines(),
            })),
            child: Arc::new(Mutex::new(child)),
            next_id: AtomicU64::new(1),
            plugin_name: plugin_name.to_string(),
        })
    }

    /// Send a JSON-RPC request and wait for a matching response.
    pub async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> OxiResult<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = PluginRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };
        let req_json = serde_json::to_string(&req)
            .map_err(|e| OxiError::Other(format!("Failed to serialize plugin request: {e}")))?;

        let child = Arc::clone(&self.child);
        let name = &self.plugin_name;

        let result = tokio::time::timeout(PLUGIN_REQUEST_TIMEOUT, async {
            let mut io = self.io.lock().await;

            // Write request.
            let stdin = io.stdin.as_mut().ok_or_else(|| {
                OxiError::Other(format!("Plugin '{name}' stdin already closed"))
            })?;
            stdin.write_all(req_json.as_bytes()).await
                .map_err(|e| OxiError::Other(format!("Plugin '{name}' write failed: {e}")))?;
            stdin.write_all(b"\n").await
                .map_err(|e| OxiError::Other(format!("Plugin '{name}' write newline failed: {e}")))?;
            stdin.flush().await
                .map_err(|e| OxiError::Other(format!("Plugin '{name}' flush failed: {e}")))?;

            // Read response lines until matching id.
            loop {
                match io.stdout_lines.next_line().await {
                    Ok(Some(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(resp) = serde_json::from_str::<PluginResponse>(trimmed) {
                            if resp.id == Some(id) {
                                return Ok(resp);
                            }
                            tracing::debug!("Plugin '{name}': skipping response with id {:?}", resp.id);
                        }
                    }
                    Ok(None) => {
                        return Err(OxiError::Other(format!("Plugin '{name}' stdout closed")));
                    }
                    Err(e) => {
                        return Err(OxiError::Other(format!("Plugin '{name}' read failed: {e}")));
                    }
                }
            }
        })
        .await;

        let response = match result {
            Ok(r) => r?,
            Err(_) => {
                tracing::warn!("Plugin '{}' request '{method}' timed out, killing", self.plugin_name);
                let mut ch = child.lock().await;
                let _ = ch.kill().await;
                return Err(OxiError::Other(
                    format!("Plugin '{}' request '{method}' timed out", self.plugin_name),
                ));
            }
        };

        if let Some(error) = response.error {
            return Err(OxiError::Other(format!("Plugin '{}': {error}", self.plugin_name)));
        }

        response
            .result
            .ok_or_else(|| OxiError::Other(format!("Plugin '{}' returned no result", self.plugin_name)))
    }

    /// Call a tool on the plugin.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> OxiResult<serde_json::Value> {
        self.request(
            "tool/call",
            Some(serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            })),
        )
        .await
    }

    /// Dispatch a hook event to the plugin.
    pub async fn dispatch_hook(
        &self,
        event: &str,
        data: serde_json::Value,
    ) -> OxiResult<serde_json::Value> {
        self.request(
            "hook/dispatch",
            Some(serde_json::json!({
                "event": event,
                "data": data,
            })),
        )
        .await
    }

    /// Check if the subprocess is still running.
    pub async fn is_alive(&self) -> bool {
        let mut child = self.child.lock().await;
        matches!(child.try_wait(), Ok(None))
    }

    /// Gracefully shut down the plugin subprocess.
    pub async fn shutdown(&self) {
        // Send EOF by dropping stdin.
        {
            let mut io = self.io.lock().await;
            io.stdin.take();
        }

        let mut child = self.child.lock().await;
        let timeout = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        if timeout.is_err() {
            tracing::warn!("Plugin '{}' did not exit cleanly, killing", self.plugin_name);
            let _ = child.kill().await;
        }
    }
}
