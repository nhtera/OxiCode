//! Stdio transport for MCP: spawns a subprocess and communicates via stdin/stdout JSON-RPC.
//!
//! Design: a single `request_lock` serializes all request/response pairs to prevent
//! interleaving on the shared stdin/stdout pipes (fixes C3 deadlock).
//! Timeout kills the child process to prevent zombies (fixes C2).
//! Shutdown takes stdin via Option to send real EOF (fixes H5).

use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use oxicode_common::{OxiError, OxiResult};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Default timeout for MCP requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Internal state holding both pipes — locked together to serialize request/response pairs.
struct TransportIo {
    stdin: Option<tokio::process::ChildStdin>,
    stdout_lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
}

/// Stdio-based MCP transport: spawns a child process and exchanges JSON-RPC over pipes.
pub struct StdioTransport {
    /// Single lock for both stdin and stdout — serializes all request/response pairs.
    io: Arc<Mutex<TransportIo>>,
    child: Arc<Mutex<Child>>,
    next_id: AtomicU64,
}

impl StdioTransport {
    /// Spawn an MCP server process.
    pub async fn spawn(
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

        let mut child = cmd
            .spawn()
            .map_err(|e| OxiError::Other(format!("Failed to spawn MCP server '{command}': {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| OxiError::Other("MCP server stdin not available".to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| OxiError::Other("MCP server stdout not available".to_string()))?;

        let lines = BufReader::new(stdout).lines();

        Ok(Self {
            io: Arc::new(Mutex::new(TransportIo {
                stdin: Some(stdin),
                stdout_lines: lines,
            })),
            child: Arc::new(Mutex::new(child)),
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a JSON-RPC request and wait for a matching response.
    ///
    /// The entire write-then-read cycle is serialized under one lock to prevent
    /// concurrent callers from interleaving on the shared pipes.
    pub async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> OxiResult<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);
        let request_json = serde_json::to_string(&request)
            .map_err(|e| OxiError::Other(format!("Failed to serialize request: {e}")))?;

        let child = Arc::clone(&self.child);

        // Entire request/response cycle under one lock — prevents interleaving.
        let result = tokio::time::timeout(REQUEST_TIMEOUT, async {
            let mut io = self.io.lock().await;

            // Write request.
            let stdin = io.stdin.as_mut().ok_or_else(|| {
                OxiError::Other("MCP transport stdin already closed".to_string())
            })?;
            stdin.write_all(request_json.as_bytes()).await
                .map_err(|e| OxiError::Other(format!("Failed to write to MCP server: {e}")))?;
            stdin.write_all(b"\n").await
                .map_err(|e| OxiError::Other(format!("Failed to write newline: {e}")))?;
            stdin.flush().await
                .map_err(|e| OxiError::Other(format!("Failed to flush stdin: {e}")))?;

            // Read response lines until we find one with our id.
            loop {
                match io.stdout_lines.next_line().await {
                    Ok(Some(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                            if resp.id == Some(id) {
                                return Ok(resp);
                            }
                            // Notification or mismatched id — skip.
                            tracing::debug!("Skipping MCP message with id {:?}", resp.id);
                        }
                    }
                    Ok(None) => {
                        return Err(OxiError::Other("MCP server stdout closed".to_string()));
                    }
                    Err(e) => {
                        return Err(OxiError::Other(format!("Failed to read from MCP server: {e}")));
                    }
                }
            }
        })
        .await;

        let response = match result {
            Ok(r) => r?,
            Err(_) => {
                // Timeout: kill the child to prevent zombie (C2 fix).
                tracing::warn!("MCP request '{method}' timed out, killing server");
                let mut ch = child.lock().await;
                let _ = ch.kill().await;
                return Err(OxiError::Other(format!("MCP request '{method}' timed out")));
            }
        };

        if let Some(error) = response.error {
            return Err(OxiError::Other(format!("MCP error: {error}")));
        }

        response
            .result
            .ok_or_else(|| OxiError::Other("MCP response has no result".to_string()))
    }

    /// Send a notification (no response expected).
    pub async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> OxiResult<()> {
        let notification = crate::protocol::JsonRpcNotification::new(method, params);
        let json = serde_json::to_string(&notification)
            .map_err(|e| OxiError::Other(format!("Failed to serialize notification: {e}")))?;

        let mut io = self.io.lock().await;
        let stdin = io.stdin.as_mut().ok_or_else(|| {
            OxiError::Other("MCP transport stdin already closed".to_string())
        })?;
        stdin.write_all(json.as_bytes()).await
            .map_err(|e| OxiError::Other(format!("Failed to write notification: {e}")))?;
        stdin.write_all(b"\n").await
            .map_err(|e| OxiError::Other(format!("Failed to write newline: {e}")))?;
        stdin.flush().await
            .map_err(|e| OxiError::Other(format!("Failed to flush: {e}")))?;
        Ok(())
    }

    /// Gracefully shut down the MCP server process.
    pub async fn shutdown(&self) {
        // Take stdin to send real EOF to the child process (H5 fix).
        {
            let mut io = self.io.lock().await;
            io.stdin.take(); // Drops ChildStdin → sends EOF.
        }

        let mut child = self.child.lock().await;
        let timeout = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        if timeout.is_err() {
            tracing::warn!("MCP server did not exit cleanly, killing");
            let _ = child.kill().await;
        }
    }
}
