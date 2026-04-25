//! Bridge WebSocket server — headless multi-session engine for IDE/cloud.
//!
//! ## Architecture
//!
//! ```text
//! run_bridge(config, pool, engine)
//!   └─ TcpListener accept loop
//!       └─ per-connection: upgrade → JWT auth → pool.admit → spawn session_task
//!           ├─ WS reader task  →  SessionCommand channel (bounded 64)
//!           └─ engine loop     →  execute_turn_with_cancel → WS sink events
//! ```
//!
//! Graceful shutdown: SIGINT/SIGTERM signal sets a shared flag; the accept loop
//! exits and each session task sends `session_end` before the connection drops.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::{accept_hdr_async, WebSocketStream};

use super::jwt::{verify_token, JwtConfig};
use super::protocol::{decode, encode, InboundMessage, OutboundMessage, BRIDGE_SUBPROTOCOL};
use super::session_pool::{SessionMetadata, SessionPool};
use crate::remote::BridgeConfig;
use oxicode_common::PermissionResponse;
use oxicode_core::{Conversation, QueryEngine, TurnEvent};

// ─── Type aliases ─────────────────────────────────────────────────────────────

type WsSink = futures::stream::SplitSink<WebSocketStream<TcpStream>, WsMessage>;
type WsStream = futures::stream::SplitStream<WebSocketStream<TcpStream>>;
type PendingPerms = Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<PermissionResponse>>>>;

// ─── Commands forwarded from WS reader to engine loop ─────────────────────────

enum SessionCommand {
    UserMessage { content: String },
    PermissionResponse { request_id: String, approved: bool },
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Run the bridge server until SIGINT/SIGTERM.
///
/// JWT secret resolution order:
/// 1. `config.jwt_secret` (set programmatically — used by tests).
/// 2. `OXICODE_BRIDGE_JWT_SECRET` environment variable.
///
/// Refuses to start if neither is set or both are empty.
/// **Never logs the secret.**
pub async fn run_bridge(
    config: BridgeConfig,
    pool: Arc<Mutex<SessionPool>>,
    engine: Arc<QueryEngine>,
) -> anyhow::Result<()> {
    // Prefer config-injected secret (enables test isolation without env mutation).
    let jwt_secret = if let Some(ref s) = config.jwt_secret {
        if s.is_empty() {
            anyhow::bail!("BridgeConfig.jwt_secret must not be empty");
        }
        s.as_bytes().to_vec()
    } else {
        load_jwt_secret()?
    };
    let jwt_secret = Arc::new(jwt_secret);

    let jwt_config = Arc::new(JwtConfig {
        iss: std::env::var("OXICODE_BRIDGE_JWT_ISS").ok(),
        aud: std::env::var("OXICODE_BRIDGE_JWT_AUD").ok(),
    });

    let addr = config.socket_addr();
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(addr = %addr, "Bridge WebSocket server listening");

    let shutdown = Arc::new(AtomicBool::new(false));
    install_shutdown_handler(shutdown.clone());

    loop {
        if shutdown.load(Ordering::SeqCst) {
            tracing::info!("Bridge: accept loop exiting (shutdown signal)");
            break;
        }

        // Poll with a short timeout so the shutdown flag is checked regularly.
        let tcp_result = tokio::time::timeout(Duration::from_millis(500), listener.accept()).await;

        let (tcp_stream, peer) = match tcp_result {
            Err(_elapsed) => continue,
            Ok(Err(e)) => {
                tracing::warn!(err = %e, "Bridge: TCP accept error");
                continue;
            }
            Ok(Ok(pair)) => pair,
        };

        tracing::debug!(peer = %peer, "Bridge: new TCP connection");

        // Clone Arcs for the per-connection task.
        let pool_c = pool.clone();
        let engine_c = engine.clone();
        let secret_c = jwt_secret.clone();
        let config_c = jwt_config.clone();
        let idle = config.idle_timeout_secs;

        tokio::spawn(async move {
            connection_task(tcp_stream, pool_c, engine_c, secret_c, config_c, idle).await;
        });
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-connection task
// ─────────────────────────────────────────────────────────────────────────────

/// Full lifecycle for one TCP connection.
#[allow(clippy::result_large_err)] // tungstenite handshake closure Err type is unavoidably large
async fn connection_task(
    tcp_stream: TcpStream,
    pool: Arc<Mutex<SessionPool>>,
    engine: Arc<QueryEngine>,
    jwt_secret: Arc<Vec<u8>>,
    jwt_config: Arc<JwtConfig>,
    idle_timeout_secs: u64,
) {
    // ── 1. WebSocket upgrade with subprotocol + bearer extraction ─────────────
    let mut bearer_from_header: Option<String> = None;

    let ws_stream = match accept_hdr_async(tcp_stream, |req: &Request, mut resp: Response| {
        if let Some(proto_val) = req.headers().get("sec-websocket-protocol") {
            let val = proto_val.to_str().unwrap_or("");
            for part in val.split(',') {
                let part = part.trim();
                if let Some(tok) = part.strip_prefix("bearer.") {
                    bearer_from_header = Some(tok.to_string());
                }
                if part == BRIDGE_SUBPROTOCOL {
                    if let Ok(v) = BRIDGE_SUBPROTOCOL.parse() {
                        resp.headers_mut().insert("sec-websocket-protocol", v);
                    }
                }
            }
        }
        Ok(resp)
    })
    .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(err = %e, "Bridge: WebSocket upgrade failed");
            return;
        }
    };

    let (raw_sink, raw_stream) = ws_stream.split();
    let sink: Arc<Mutex<WsSink>> = Arc::new(Mutex::new(raw_sink));

    // ── 2. JWT authentication ─────────────────────────────────────────────────
    let Ok((claims, stream)) = jwt_auth(
        bearer_from_header,
        raw_stream,
        sink.clone(),
        &jwt_secret,
        &jwt_config,
    )
    .await
    else {
        return; // close frame already sent by jwt_auth
    };

    // ── 3. Admit to session pool ──────────────────────────────────────────────
    let model = engine.model();
    let metadata = SessionMetadata {
        user_id: Some(claims.sub.clone()),
        model: model.clone(),
        working_dir: None,
    };
    let session_id = {
        let mut guard = pool.lock().await;
        match guard.create_session(metadata) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(user = %claims.sub, err = %e, "Bridge: session pool full");
                send_msg(
                    &sink,
                    &OutboundMessage::Error {
                        message: "Session pool full — try again later".to_string(),
                    },
                )
                .await;
                return;
            }
        }
    };

    tracing::info!(session_id = %session_id, user = %claims.sub, "Bridge: session admitted");

    // ── 4. Send session_start ─────────────────────────────────────────────────
    send_msg(
        &sink,
        &OutboundMessage::SessionStart {
            session_id: session_id.clone(),
            model,
            timestamp: Utc::now().to_rfc3339(),
            protocol: BRIDGE_SUBPROTOCOL.to_string(),
        },
    )
    .await;

    // ── 5. Run session loop ───────────────────────────────────────────────────
    session_loop(
        stream,
        sink.clone(),
        session_id.clone(),
        engine,
        pool.clone(),
        idle_timeout_secs,
    )
    .await;

    // ── 6. Release from pool + send session_end ───────────────────────────────
    pool.lock().await.remove(&session_id);
    send_msg(
        &sink,
        &OutboundMessage::SessionEnd {
            reason: "session_closed".to_string(),
        },
    )
    .await;

    tracing::info!(session_id = %session_id, "Bridge: session complete");
}

// ─────────────────────────────────────────────────────────────────────────────
// JWT authentication
// ─────────────────────────────────────────────────────────────────────────────

/// Authenticate the connection. Returns `(Claims, remaining_stream)` or sends
/// WS close-code 4401 and returns `Err(())`.
async fn jwt_auth(
    bearer_from_header: Option<String>,
    mut stream: WsStream,
    sink: Arc<Mutex<WsSink>>,
    jwt_secret: &[u8],
    jwt_config: &JwtConfig,
) -> Result<(super::jwt::Claims, WsStream), ()> {
    // Prefer token extracted from subprotocol header.
    if let Some(token) = bearer_from_header {
        match verify_token(&token, jwt_secret, jwt_config) {
            Ok(claims) => return Ok((claims, stream)),
            Err(e) => {
                tracing::warn!(err = %e, "Bridge: JWT auth failed (subprotocol bearer)");
                send_close_unauthorized(sink).await;
                return Err(());
            }
        }
    }

    // Fall back: read first WS text message — expected to contain a raw JWT string.
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next()).await;
    match first {
        Ok(Some(Ok(WsMessage::Text(raw)))) => {
            match verify_token(raw.trim(), jwt_secret, jwt_config) {
                Ok(claims) => Ok((claims, stream)),
                Err(e) => {
                    tracing::warn!(err = %e, "Bridge: JWT auth failed (first message)");
                    send_close_unauthorized(sink).await;
                    Err(())
                }
            }
        }
        Ok(Some(Ok(WsMessage::Close(_)))) => {
            tracing::debug!("Bridge: client closed before auth");
            Err(())
        }
        _ => {
            tracing::warn!("Bridge: auth timeout or unexpected frame");
            send_close_unauthorized(sink).await;
            Err(())
        }
    }
}

/// Send WS close frame with library close-code 4401 (Unauthorized).
async fn send_close_unauthorized(sink: Arc<Mutex<WsSink>>) {
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;
    let frame = CloseFrame {
        code: CloseCode::Library(4401),
        reason: "Unauthorized: invalid or missing JWT".into(),
    };
    let _ = sink.lock().await.send(WsMessage::Close(Some(frame))).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Session loop: wire WS ↔ QueryEngine
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)] // session loop is inherently long: reader + engine + teardown
async fn session_loop(
    mut stream: WsStream,
    sink: Arc<Mutex<WsSink>>,
    session_id: String,
    engine: Arc<QueryEngine>,
    pool: Arc<Mutex<SessionPool>>,
    idle_timeout_secs: u64,
) {
    // Bounded channel: WS reader → engine loop (64 slots).
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<SessionCommand>(64);

    // Cancel flag shared between reader and engine loop.
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Pending permission requests: request_id → reply channel.
    let pending_perms: PendingPerms = Arc::new(Mutex::new(HashMap::new()));

    // Conversation persists for the lifetime of the session.
    let conversation: Arc<Mutex<Conversation>> = Arc::new(Mutex::new(Conversation::new()));

    let idle = Duration::from_secs(idle_timeout_secs);

    // ── WS reader task ────────────────────────────────────────────────────────
    let cancel_r = cancel_flag.clone();
    let _pending_r = pending_perms.clone();
    let sid_r = session_id.clone();
    let cmd_tx_r = cmd_tx.clone();

    let reader = tokio::spawn(async move {
        let mut last_active = tokio::time::Instant::now();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                Err(_) => {
                    if last_active.elapsed() >= idle {
                        tracing::info!(session_id = %sid_r, "Bridge: idle timeout");
                        break;
                    }
                }
                Ok(None) => {
                    tracing::debug!(session_id = %sid_r, "Bridge: WS stream ended");
                    break;
                }
                Ok(Some(Err(e))) => {
                    tracing::warn!(session_id = %sid_r, err = %e, "Bridge: WS read error");
                    break;
                }
                Ok(Some(Ok(WsMessage::Text(raw)))) => {
                    last_active = tokio::time::Instant::now();
                    match decode(raw.as_str()) {
                        Ok(InboundMessage::UserMessage { content, .. }) => {
                            if cmd_tx_r
                                .try_send(SessionCommand::UserMessage { content })
                                .is_err()
                            {
                                tracing::warn!(
                                    session_id = %sid_r,
                                    "Bridge: command channel full, dropping message"
                                );
                            }
                        }
                        Ok(InboundMessage::Cancel) => {
                            cancel_r.store(true, Ordering::SeqCst);
                        }
                        Ok(InboundMessage::PermissionResponse {
                            request_id,
                            approved,
                        }) => {
                            let cmd = SessionCommand::PermissionResponse {
                                request_id,
                                approved,
                            };
                            let _ = cmd_tx_r.try_send(cmd);
                        }
                        Ok(InboundMessage::SlashCommand { command }) => {
                            // Slash commands are logged but not yet dispatched.
                            tracing::debug!(
                                session_id = %sid_r,
                                %command,
                                "Bridge: slash command (not yet implemented)"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(session_id = %sid_r, err = %e, "Bridge: decode error");
                        }
                    }
                }
                Ok(Some(Ok(WsMessage::Close(_)))) => break,
                Ok(Some(Ok(_))) => {} // ping/pong/binary — ignore
            }
        }
    });

    // ── Engine loop ───────────────────────────────────────────────────────────
    let sink_e = sink.clone();
    let pending_e = pending_perms.clone();
    let cancel_e = cancel_flag.clone();
    let conv_e = conversation.clone();
    let pool_e = pool.clone();
    let sid_e = session_id.clone();

    let engine_task = tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                SessionCommand::PermissionResponse {
                    request_id,
                    approved,
                } => {
                    let mut guard = pending_e.lock().await;
                    if let Some(tx) = guard.remove(&request_id) {
                        let resp = if approved {
                            PermissionResponse::AllowOnce
                        } else {
                            PermissionResponse::Deny
                        };
                        let _ = tx.send(resp);
                    }
                }
                SessionCommand::UserMessage { content } => {
                    let user_msg = oxicode_common::Message::user(&content);
                    conv_e.lock().await.push(user_msg);

                    // Touch session activity.
                    pool_e.lock().await.get_mut(&sid_e);

                    // Per-turn event channel.
                    let (event_tx, event_rx) = mpsc::channel::<TurnEvent>(128);

                    // Spawn event forwarder.
                    let fwd = tokio::spawn(forward_events(
                        event_rx,
                        sink_e.clone(),
                        pending_e.clone(),
                        sid_e.clone(),
                    ));

                    // Execute turn.
                    let result = {
                        let mut conv = conv_e.lock().await;
                        engine
                            .execute_turn_with_cancel(&mut conv, Some(&event_tx), Some(&cancel_e))
                            .await
                    };

                    // Drop sender so forwarder drains.
                    drop(event_tx);
                    let _ = fwd.await;

                    match result {
                        Ok(msg) => conv_e.lock().await.push(msg),
                        Err(e) => {
                            send_msg(
                                &sink_e,
                                &OutboundMessage::Error {
                                    message: e.to_string(),
                                },
                            )
                            .await;
                        }
                    }
                }
            }
        }
    });

    // Wait for reader (idle / WS close), then stop engine.
    let _ = reader.await;
    engine_task.abort();
    let _ = engine_task.await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Event forwarder: TurnEvent channel → OutboundMessage → WS sink
// ─────────────────────────────────────────────────────────────────────────────

async fn forward_events(
    mut rx: mpsc::Receiver<TurnEvent>,
    sink: Arc<Mutex<WsSink>>,
    pending_perms: PendingPerms,
    session_id: String,
) {
    while let Some(event) = rx.recv().await {
        let out = map_turn_event(event, &pending_perms, &session_id).await;
        if let Some(msg) = out {
            send_msg(&sink, &msg).await;
        }
    }
}

/// Map a `TurnEvent` to an `OutboundMessage`. Returns `None` for internal-only events.
async fn map_turn_event(
    event: TurnEvent,
    pending_perms: &PendingPerms,
    _session_id: &str,
) -> Option<OutboundMessage> {
    match event {
        TurnEvent::TextDelta(text) => Some(OutboundMessage::TextDelta { text }),
        TurnEvent::ToolUseStart { id, name, input } => Some(OutboundMessage::ToolUse {
            tool_use_id: id,
            tool_name: name,
            input,
        }),
        TurnEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => Some(OutboundMessage::ToolResult {
            tool_use_id,
            content,
            is_error,
        }),
        TurnEvent::PermissionAsk {
            tool_name,
            input_summary,
            prompt,
            reply_tx,
        } => {
            let request_id = uuid::Uuid::new_v4().to_string();
            pending_perms
                .lock()
                .await
                .insert(request_id.clone(), reply_tx);
            Some(OutboundMessage::PermissionAsk {
                request_id,
                tool_name,
                input_summary,
                prompt,
            })
        }
        TurnEvent::Error(msg) => Some(OutboundMessage::Error { message: msg }),
        // TurnEnd signals end of streaming; emit a zero-usage marker.
        TurnEvent::TurnEnd => Some(OutboundMessage::Usage {
            input_tokens: 0,
            output_tokens: 0,
        }),
        // TurnStart / ThinkingDelta / Retrying / RateLimited / HookProgress / HookMessage
        // are internal; not forwarded to the wire client.
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Serialize and send one `OutboundMessage` to the WS sink.
async fn send_msg(sink: &Arc<Mutex<WsSink>>, msg: &OutboundMessage) {
    if let Ok(encoded) = encode(msg) {
        let _ = sink.lock().await.send(WsMessage::Text(encoded)).await;
    }
}

/// Load JWT secret from environment. Never logs the value.
pub fn load_jwt_secret() -> anyhow::Result<Vec<u8>> {
    let secret = std::env::var("OXICODE_BRIDGE_JWT_SECRET").map_err(|_| {
        anyhow::anyhow!(
            "OXICODE_BRIDGE_JWT_SECRET is not set. \
             Bridge mode requires a JWT secret to authenticate clients."
        )
    })?;
    if secret.is_empty() {
        anyhow::bail!("OXICODE_BRIDGE_JWT_SECRET must not be empty");
    }
    Ok(secret.into_bytes())
}

/// Install a SIGINT/SIGTERM handler that sets `shutdown_flag`.
fn install_shutdown_handler(shutdown_flag: Arc<AtomicBool>) {
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Bridge: shutdown signal received");
        shutdown_flag.store(true, Ordering::SeqCst);
    });
}
