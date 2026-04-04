//! Server mode entry point — long-running JSON-RPC service over stdin/stdout.
//!
//! `oxicode --server` starts this loop. IDE extensions connect by spawning the
//! process and communicating via line-delimited JSON-RPC on stdin/stdout.
//!
//! **Non-blocking dispatch:** `message.send` runs as a background task so the
//! main request loop can still process `tool.approve`, `message.cancel`, etc.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::server_handler::ServerHandler;
use crate::server_protocol::{error_codes, RpcId, RpcNotification, RpcRequest, RpcResponse};

/// Shared stdout writer for responses and notifications.
type StdoutWriter = Arc<tokio::sync::Mutex<tokio::io::Stdout>>;

/// Run the server loop: read JSON-RPC from stdin, dispatch, write to stdout.
pub async fn run_server(
    engine: Arc<oxicode_core::QueryEngine>,
    model: String,
) -> anyhow::Result<()> {
    let (notify_tx, mut notify_rx) = mpsc::channel::<RpcNotification>(512);
    let handler = Arc::new(ServerHandler::new(engine, model, notify_tx));

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    let stdout: StdoutWriter = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));

    // Notification writer task: drains notification channel and writes to stdout.
    let stdout_notify = stdout.clone();
    let notify_writer = tokio::spawn(async move {
        while let Some(notification) = notify_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&notification) {
                let mut out = stdout_notify.lock().await;
                let _ = out.write_all(json.as_bytes()).await;
                let _ = out.write_all(b"\n").await;
                let _ = out.flush().await;
            }
        }
    });

    // Main request loop: read line -> parse -> dispatch -> write response.
    // FIX C2: message.send is spawned as a background task so the loop stays
    // free to process tool.approve, tool.deny, and message.cancel concurrently.
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                // JSON-RPC spec: parse error response id MUST be null.
                // We use 0 as placeholder since our RpcId doesn't have a Null variant.
                let resp = RpcResponse::err(
                    RpcId::Num(0),
                    error_codes::PARSE_ERROR,
                    format!("Parse error: {e}"),
                );
                write_response(&stdout, &resp).await;
                continue;
            }
        };

        // message.send and bridge.sendMessage are long-running — spawn as background task.
        if req.method == "message.send" || req.method == "bridge.sendMessage" {
            let handler = handler.clone();
            let stdout = stdout.clone();
            tokio::spawn(async move {
                let resp = handler.handle(req).await;
                write_response(&stdout, &resp).await;
            });
            continue;
        }

        let resp = handler.handle(req).await;
        let is_shutdown = ServerHandler::is_shutdown_response(&resp);
        write_response(&stdout, &resp).await;

        if is_shutdown {
            tracing::info!("Shutdown requested, exiting server loop");
            break;
        }
    }

    // Drop handler to close notify_tx, then wait for writer to finish.
    drop(handler);
    let _ = notify_writer.await;

    Ok(())
}

/// Write a JSON-RPC response as a single line to stdout.
async fn write_response(stdout: &StdoutWriter, resp: &RpcResponse) {
    if let Ok(json) = serde_json::to_string(resp) {
        let mut out = stdout.lock().await;
        let _ = out.write_all(json.as_bytes()).await;
        let _ = out.write_all(b"\n").await;
        let _ = out.flush().await;
    }
}
