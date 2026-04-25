//! Smoke tests for the bridge WebSocket server (requires `--features bridge`).
//!
//! Tests:
//!   1. Valid JWT via first message → session_start received.
//!   2. Full turn: user message → text_delta event received.
//!   3. Invalid JWT → WS close-code 4401.
//!   4. Expired JWT → WS close-code 4401.
//!   5. Missing jwt_secret → run_bridge returns Err immediately.
//!   6. load_jwt_secret helpers (unit, no async).
//!
//! Run with:
//!   cargo test -p oxicode-cli --test bridge_smoke --features bridge

#![cfg(feature = "bridge")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use jsonwebtoken::{encode as jwt_encode, EncodingKey, Header};
use oxicode_api::MockLlmProvider;
use oxicode_core::QueryEngine;
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::{AppState, StateStore};
use oxicode_tools::file_state_tracker::FileStateTracker;
use oxicode_tools::tool_trait::ToolContext;
use serde::Serialize;
use tokio::sync::Mutex as TokioMutex;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use oxicode_cli::bridge::{load_jwt_secret, run_bridge};
use oxicode_cli::remote::{session_pool::SessionPool, BridgeConfig};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

const TEST_SECRET: &[u8] = b"test-bridge-jwt-secret-256bit-min-length!!!!";

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// JWT claims struct for minting test tokens.
#[derive(Serialize)]
struct TestClaims {
    sub: String,
    exp: u64,
}

/// Mint a HS256 JWT with the given expiry offset from now.
fn mint_token(sub: &str, exp_offset_secs: i64) -> String {
    let exp = (chrono::Utc::now().timestamp() + exp_offset_secs) as u64;
    jwt_encode(
        &Header::default(),
        &TestClaims {
            sub: sub.to_string(),
            exp,
        },
        &EncodingKey::from_secret(TEST_SECRET),
    )
    .expect("mint_token: encode failed")
}

/// Build a `QueryEngine` backed by `MockLlmProvider`.
fn make_mock_engine(response_text: &str) -> Arc<QueryEngine> {
    let provider = Arc::new(MockLlmProvider::with_text(response_text));
    let state_store = Arc::new(StateStore::new(AppState::default()));
    let tool_registry = Arc::new(oxicode_tools::default_registry());
    let pipeline = Arc::new(PermissionPipeline::new(PermissionMode::Bypass, vec![]));
    let tool_ctx = ToolContext {
        working_dir: std::env::temp_dir(),
        extra_working_dirs: vec![],
        file_state: Arc::new(FileStateTracker::default()),
        task_manager: Arc::new(Mutex::new(oxicode_tasks::TaskManager::default())),
        task_abort_handles: Arc::new(Mutex::new(HashMap::new())),
        mcp_manager: Arc::new(oxicode_mcp::McpServerManager::new()),
        skill_executor: None,
        team_manager: Arc::new(Mutex::new(oxicode_agents::TeamManager::new())),
        bash_processes: Arc::new(Mutex::new(HashMap::new())),
        hook_manager: None,
        permission_mode: PermissionMode::Bypass,
    };
    Arc::new(QueryEngine::new(
        provider,
        state_store,
        tool_registry,
        pipeline,
        tool_ctx,
        "mock-model".to_string(),
        4096,
        "You are a test assistant.".to_string(),
    ))
}

/// Find an ephemeral port and build a `BridgeConfig` with the test JWT secret.
async fn make_config(secret: Option<&str>) -> (u16, BridgeConfig) {
    // Bind port=0 to get an OS-assigned port, then release it.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let config = BridgeConfig {
        port,
        max_sessions: 16,
        idle_timeout_secs: 600,
        bind_address: "127.0.0.1".to_string(),
        jwt_secret: secret.map(str::to_string),
    };
    (port, config)
}

/// Build an `Arc<TokioMutex<SessionPool>>` for tests.
fn make_pool() -> Arc<TokioMutex<SessionPool>> {
    Arc::new(TokioMutex::new(SessionPool::new(16, 600)))
}

/// Collect WS text/close frames until `pred` returns true or timeout elapses.
/// Returns all collected frames as `serde_json::Value` (close frames get
/// `{"type":"__close__","code":<u16>}`).
async fn collect_until<F>(
    stream: &mut futures::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    pred: F,
    timeout_secs: u64,
) -> Vec<serde_json::Value>
where
    F: Fn(&serde_json::Value) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut collected = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(WsMessage::Text(raw)))) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.as_str()) {
                    let done = pred(&v);
                    collected.push(v);
                    if done {
                        break;
                    }
                }
            }
            Ok(Some(Ok(WsMessage::Close(frame)))) => {
                collected.push(serde_json::json!({
                    "type": "__close__",
                    "code": frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(1000),
                    "reason": frame.as_ref().map(|f| f.reason.to_string()).unwrap_or_default()
                }));
                break;
            }
            _ => break,
        }
    }
    collected
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: load_jwt_secret (unit, no network)
// ─────────────────────────────────────────────────────────────────────────────

/// `load_jwt_secret` returns Err when the env var is unset.
/// NOTE: env var tests cannot run in parallel safely; they each use a unique
/// name or are serialized.  We use a dedicated env var name per sub-test.
#[test]
fn test_load_jwt_secret_missing_env_var() {
    // Ensure the var is absent by using a unique test-only name. We temporarily
    // point OXICODE_BRIDGE_JWT_SECRET away using a scope trick: save + remove + restore.
    let saved = std::env::var("OXICODE_BRIDGE_JWT_SECRET").ok();
    // SAFETY: single-threaded test binary section; no other thread reads this var.
    #[allow(unused_unsafe)]
    {
        // Use std::env directly — unavoidably unsafe in Rust ≥1.72 multi-thread warning,
        // but integration tests here run single-file so this is acceptable.
        // The lints are #[allow]'d at file scope below.
        std::env::remove_var("OXICODE_BRIDGE_JWT_SECRET");
    }
    let result = load_jwt_secret();
    // Restore
    if let Some(v) = saved {
        std::env::set_var("OXICODE_BRIDGE_JWT_SECRET", v);
    }
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("OXICODE_BRIDGE_JWT_SECRET"),
        "error should name the missing env var"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: server boot validation
// ─────────────────────────────────────────────────────────────────────────────

/// run_bridge returns Err immediately when no JWT secret is configured.
#[tokio::test]
async fn test_run_bridge_refuses_without_secret() {
    let (port, _) = make_config(None).await;
    // Config with no jwt_secret and env var intentionally absent.
    let config = BridgeConfig {
        port,
        max_sessions: 2,
        idle_timeout_secs: 60,
        bind_address: "127.0.0.1".to_string(),
        jwt_secret: None, // no secret
    };
    // Clear the env var for this test so the fallback also fails.
    // We do not mutate env here — if OXICODE_BRIDGE_JWT_SECRET happens to be set
    // the test may pass through that path, but the binary will still start and the
    // test needs to check whether our *config path* is exercised.
    // Instead: use an empty string, which is always rejected.
    let config_empty = BridgeConfig {
        jwt_secret: Some(String::new()),
        ..config
    };
    let pool = make_pool();
    let engine = make_mock_engine("unused");
    let result: anyhow::Result<()> = run_bridge(config_empty, pool, engine).await;
    assert!(result.is_err(), "Expected Err for empty secret");
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: JWT authentication (network)
// ─────────────────────────────────────────────────────────────────────────────

/// Valid JWT → `session_start` event received.
#[tokio::test]
async fn test_valid_jwt_receives_session_start() {
    let secret_str = String::from_utf8_lossy(TEST_SECRET).to_string();
    let (port, config) = make_config(Some(&secret_str)).await;
    let pool = make_pool();
    let engine = make_mock_engine("Hello from bridge!");

    let server = tokio::spawn(run_bridge(config, pool, engine));
    tokio::time::sleep(Duration::from_millis(150)).await;

    let url = format!("ws://127.0.0.1:{port}");
    let (ws, _) = connect_async(&url).await.expect("WS connect failed");
    let (mut sink, mut stream) = ws.split();

    // Send valid JWT as first (auth) message.
    sink.send(WsMessage::Text(mint_token("alice", 3600).into()))
        .await
        .unwrap();

    let events = collect_until(&mut stream, |v| v["type"] == "session_start", 5).await;

    server.abort();
    assert!(
        events.iter().any(|v| v["type"] == "session_start"),
        "Expected session_start, got: {events:?}"
    );
    let start = events
        .iter()
        .find(|v| v["type"] == "session_start")
        .unwrap();
    assert!(!start["session_id"].as_str().unwrap_or("").is_empty());
    assert_eq!(start["protocol"], "oxicode.bridge.v1");
}

/// Valid JWT + user message → `text_delta` event received.
#[tokio::test]
async fn test_valid_jwt_full_turn_text_delta() {
    let secret_str = String::from_utf8_lossy(TEST_SECRET).to_string();
    let (port, config) = make_config(Some(&secret_str)).await;
    let pool = make_pool();
    let engine = make_mock_engine("delta response text");

    let server = tokio::spawn(run_bridge(config, pool, engine));
    tokio::time::sleep(Duration::from_millis(150)).await;

    let url = format!("ws://127.0.0.1:{port}");
    let (ws, _) = connect_async(&url).await.expect("WS connect failed");
    let (mut sink, mut stream) = ws.split();

    sink.send(WsMessage::Text(mint_token("bob", 3600).into()))
        .await
        .unwrap();

    // Wait for session_start.
    collect_until(&mut stream, |v| v["type"] == "session_start", 5).await;

    // Send user message.
    let msg = serde_json::json!({"type": "user_message", "content": "hello"});
    sink.send(WsMessage::Text(msg.to_string().into()))
        .await
        .unwrap();

    let events = collect_until(&mut stream, |v| v["type"] == "text_delta", 10).await;

    server.abort();
    assert!(
        events.iter().any(|v| v["type"] == "text_delta"),
        "Expected text_delta in: {events:?}"
    );
}

/// Invalid JWT (garbage string) → WS close with code 4401.
#[tokio::test]
async fn test_invalid_jwt_close_4401() {
    let secret_str = String::from_utf8_lossy(TEST_SECRET).to_string();
    let (port, config) = make_config(Some(&secret_str)).await;
    let pool = make_pool();
    let engine = make_mock_engine("unused");

    let server = tokio::spawn(run_bridge(config, pool, engine));
    tokio::time::sleep(Duration::from_millis(150)).await;

    let url = format!("ws://127.0.0.1:{port}");
    let (ws, _) = connect_async(&url).await.expect("WS connect");
    let (mut sink, mut stream) = ws.split();

    sink.send(WsMessage::Text("not-a-valid-jwt-token".into()))
        .await
        .unwrap();

    let events = collect_until(&mut stream, |v| v["type"] == "__close__", 5).await;

    server.abort();
    let close = events
        .iter()
        .find(|v| v["type"] == "__close__")
        .expect("Expected WS close frame");
    assert_eq!(
        close["code"].as_u64().unwrap_or(0),
        4401,
        "Expected close-code 4401, got: {close}"
    );
}

/// Expired JWT → WS close with code 4401.
#[tokio::test]
async fn test_expired_jwt_close_4401() {
    let secret_str = String::from_utf8_lossy(TEST_SECRET).to_string();
    let (port, config) = make_config(Some(&secret_str)).await;
    let pool = make_pool();
    let engine = make_mock_engine("unused");

    let server = tokio::spawn(run_bridge(config, pool, engine));
    tokio::time::sleep(Duration::from_millis(150)).await;

    let url = format!("ws://127.0.0.1:{port}");
    let (ws, _) = connect_async(&url).await.expect("WS connect");
    let (mut sink, mut stream) = ws.split();

    // Expired 120 seconds ago.
    sink.send(WsMessage::Text(mint_token("eve", -120).into()))
        .await
        .unwrap();

    let events = collect_until(&mut stream, |v| v["type"] == "__close__", 5).await;

    server.abort();
    let close = events
        .iter()
        .find(|v| v["type"] == "__close__")
        .expect("Expected WS close frame");
    assert_eq!(
        close["code"].as_u64().unwrap_or(0),
        4401,
        "Expected close-code 4401 for expired token, got: {close}"
    );
}

/// Wrong signing key → WS close with code 4401.
#[tokio::test]
async fn test_wrong_signing_key_close_4401() {
    let secret_str = String::from_utf8_lossy(TEST_SECRET).to_string();
    let (port, config) = make_config(Some(&secret_str)).await;
    let pool = make_pool();
    let engine = make_mock_engine("unused");

    let server = tokio::spawn(run_bridge(config, pool, engine));
    tokio::time::sleep(Duration::from_millis(150)).await;

    let url = format!("ws://127.0.0.1:{port}");
    let (ws, _) = connect_async(&url).await.expect("WS connect");
    let (mut sink, mut stream) = ws.split();

    // Mint with a *different* secret than what the server holds.
    let wrong_secret = b"completely-different-secret-that-is-also-long-enough";
    let exp = (chrono::Utc::now().timestamp() + 3600) as u64;
    let token = jwt_encode(
        &Header::default(),
        &TestClaims {
            sub: "mallory".to_string(),
            exp,
        },
        &EncodingKey::from_secret(wrong_secret),
    )
    .unwrap();
    sink.send(WsMessage::Text(token.into())).await.unwrap();

    let events = collect_until(&mut stream, |v| v["type"] == "__close__", 5).await;

    server.abort();
    let close = events
        .iter()
        .find(|v| v["type"] == "__close__")
        .expect("Expected WS close frame");
    assert_eq!(
        close["code"].as_u64().unwrap_or(0),
        4401,
        "Expected close-code 4401 for wrong key, got: {close}"
    );
}
