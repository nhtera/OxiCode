//! End-to-end core workflow tests for OxiCode.
//!
//! Tests the full agentic AI coding agent pipeline with real API calls:
//! - API streaming (text, tool use, multi-turn)
//! - Full agent loop (file read/write/edit, bash, glob/grep)
//! - TUI ↔ Engine bridge (event flow, permissions)
//! - Session persistence (save/load/resume)
//! - Slash commands (/help, /model, /cost)
//! - Config & provider routing
//!
//! Tests requiring API are gated behind `#[ignore]`.
//! Run with:
//!   export ANTHROPIC_AUTH_TOKEN="sk-..."
//!   export ANTHROPIC_BASE_URL="https://ezaiapi.com"
//!   export ANTHROPIC_DEFAULT_SONNET_MODEL="claude-sonnet-4.6"
//!   cargo test -p oxicode-cli --test e2e_core_workflow -- --ignored --nocapture

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use oxicode_api::{AnthropicProvider, LlmProvider, MessageRequest, StreamEvent};
use oxicode_common::{ContentBlock, Message, PermissionResponse};
use oxicode_core::turn_event::TurnEvent;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::{AppState, StateStore};
use oxicode_tools::file_state_tracker::FileStateTracker;
use oxicode_tools::tool_trait::ToolContext;
use oxicode_tui::events::{CoreEvent, UiEvent};
use tokio::sync::mpsc;

/// Serialize live API tests to avoid 429s from parallel test workers.
static LIVE_TEST_LOCK: Mutex<()> = Mutex::new(());

// ═══════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════

fn make_live_provider() -> AnthropicProvider {
    let token =
        std::env::var("ANTHROPIC_AUTH_TOKEN").expect("ANTHROPIC_AUTH_TOKEN env var required");
    let base_url = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
    AnthropicProvider::new(token).with_base_url(base_url)
}

fn model_name() -> String {
    std::env::var("ANTHROPIC_DEFAULT_SONNET_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string())
}

fn make_tool_context(dir: &Path) -> ToolContext {
    ToolContext {
        working_dir: dir.to_path_buf(),
        file_state: Arc::new(FileStateTracker::default()),
        task_manager: Arc::new(Mutex::new(oxicode_tasks::TaskManager::default())),
        task_abort_handles: Arc::new(Mutex::new(HashMap::new())),
        mcp_manager: Arc::new(oxicode_mcp::McpServerManager::new()),
        skill_executor: None,
        team_manager: Arc::new(Mutex::new(oxicode_agents::TeamManager::new())),
        bash_processes: Arc::new(Mutex::new(HashMap::new())),
    }
}

fn make_live_engine(dir: &Path) -> (Arc<QueryEngine>, Arc<StateStore>) {
    make_live_engine_with_mode(dir, PermissionMode::Bypass)
}

fn make_live_engine_with_mode(
    dir: &Path,
    mode: PermissionMode,
) -> (Arc<QueryEngine>, Arc<StateStore>) {
    let provider = Arc::new(make_live_provider());
    let state_store = Arc::new(StateStore::new(AppState::default()));
    let tool_registry = Arc::new(oxicode_tools::default_registry());
    let permission_pipeline = Arc::new(PermissionPipeline::new(mode, vec![]));
    let tool_context = make_tool_context(dir);

    let engine = Arc::new(QueryEngine::new(
        provider,
        state_store.clone(),
        tool_registry,
        permission_pipeline,
        tool_context,
        model_name(),
        16384,
        "You are a helpful coding assistant. Be concise. Always use tools when asked.".to_string(),
    ));
    (engine, state_store)
}

/// Translate TurnEvent → CoreEvent (mirrors main.rs).
fn translate_turn_event(te: TurnEvent) -> CoreEvent {
    match te {
        TurnEvent::TextDelta(t) => CoreEvent::TextDelta(t),
        TurnEvent::ThinkingDelta(t) => CoreEvent::ThinkingDelta(t),
        TurnEvent::TurnStart => CoreEvent::StreamStart,
        TurnEvent::TurnEnd => CoreEvent::StreamEnd,
        TurnEvent::ToolUseStart { id, name, input } => CoreEvent::ToolUseStart { id, name, input },
        TurnEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => CoreEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        },
        TurnEvent::PermissionAsk {
            tool_name,
            input_summary,
            prompt,
            reply_tx,
        } => CoreEvent::PermissionAsk {
            tool_name,
            input_summary,
            prompt,
            reply_tx,
        },
        TurnEvent::Error(e) => CoreEvent::Error(e),
        TurnEvent::RateLimited {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        } => CoreEvent::RateLimited {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════
// A. API Layer Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_streaming_text_response() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let provider = make_live_provider();
    let request = MessageRequest::new(
        model_name(),
        vec![Message::user("Say exactly the word 'oxicode' and nothing else.")],
    )
    .with_max_tokens(100);

    let mut stream = provider
        .stream_message(request)
        .await
        .expect("stream should start");

    let mut text = String::new();
    let mut got_stop = false;
    while let Some(event) = stream.next().await {
        match event.expect("stream event") {
            StreamEvent::TextDelta { text: t } => text.push_str(&t),
            StreamEvent::MessageStop { .. } => got_stop = true,
            StreamEvent::Error { message } => panic!("API error: {message}"),
            _ => {}
        }
    }
    let lower = text.to_lowercase();
    assert!(lower.contains("oxicode"), "Expected 'oxicode' in: {text}");
    assert!(got_stop, "Should get MessageStop");
}

#[tokio::test]
#[ignore]
async fn e2e_streaming_tool_use() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let provider = make_live_provider();

    let tool_schema = serde_json::json!({
        "name": "file_read",
        "description": "Read a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to the file" }
            },
            "required": ["file_path"]
        }
    });

    let mut request = MessageRequest::new(
        model_name(),
        vec![Message::user("Read the file /tmp/test.txt using the file_read tool.")],
    )
    .with_max_tokens(500);
    request.tools = vec![tool_schema];

    let mut stream = provider
        .stream_message(request)
        .await
        .expect("stream should start");

    let mut got_tool_start = false;
    let mut tool_name = String::new();
    while let Some(event) = stream.next().await {
        match event.expect("stream event") {
            StreamEvent::ToolUseStart { name, .. } => {
                tool_name = name;
                got_tool_start = true;
            }
            StreamEvent::Error { message } => panic!("API error: {message}"),
            _ => {}
        }
    }
    assert!(got_tool_start, "Should get ToolUseStart");
    assert_eq!(tool_name, "file_read");
}

#[tokio::test]
#[ignore]
async fn e2e_multi_turn_context() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let provider = make_live_provider();

    let messages = vec![
        Message::user("What is 5+7?"),
        {
            let mut m = Message::assistant();
            m.content.push(ContentBlock::Text {
                text: "12".to_string(),
            });
            m
        },
        Message::user("Multiply that by 3. Reply with just the number."),
    ];

    let request = MessageRequest::new(model_name(), messages).with_max_tokens(100);
    let mut stream = provider.stream_message(request).await.expect("stream");

    let mut text = String::new();
    while let Some(event) = stream.next().await {
        if let StreamEvent::TextDelta { text: t } = event.expect("event") {
            text.push_str(&t);
        }
    }
    assert!(text.contains("36"), "Expected '36', got: {text}");
}

#[tokio::test]
#[ignore]
async fn e2e_token_usage_tracking() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let provider = make_live_provider();
    let request =
        MessageRequest::new(model_name(), vec![Message::user("Say hi")]).with_max_tokens(50);

    let mut stream = provider.stream_message(request).await.expect("stream");
    let mut total_output = 0u32;
    while let Some(event) = stream.next().await {
        if let StreamEvent::UsageUpdate(usage) = event.expect("event") {
            total_output += usage.output_tokens;
        }
    }
    assert!(total_output > 0, "Output tokens should be > 0, got {total_output}");
}

#[tokio::test]
#[ignore]
async fn e2e_custom_base_url() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let base_url = match std::env::var("ANTHROPIC_BASE_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => {
            eprintln!("Skipping: no ANTHROPIC_BASE_URL set");
            return;
        }
    };

    let token =
        std::env::var("ANTHROPIC_AUTH_TOKEN").expect("ANTHROPIC_AUTH_TOKEN required");
    let provider = AnthropicProvider::new(token).with_base_url(&base_url);
    let request =
        MessageRequest::new(model_name(), vec![Message::user("Say OK")]).with_max_tokens(20);

    let mut stream = provider.stream_message(request).await.expect("stream from custom URL");
    let mut got_text = false;
    while let Some(event) = stream.next().await {
        match event.expect("event") {
            StreamEvent::TextDelta { .. } => got_text = true,
            StreamEvent::Error { message } => panic!("Custom URL error: {message}"),
            _ => {}
        }
    }
    assert!(got_text, "Should receive text from custom URL: {base_url}");
}

// ═══════════════════════════════════════════════════════════════════
// B. Full Agent Loop Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_file_read_tool_loop() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let secret_file = tmp.path().join("secret.txt");
    std::fs::write(&secret_file, "The secret word is: OXISECRET42").unwrap();

    let (engine, _) = make_live_engine(tmp.path());
    let mut conv = Conversation::new();
    let prompt = format!(
        "Read the file at {} and tell me the secret word. Use the file_read tool.",
        secret_file.display()
    );
    conv.push(Message::user(&prompt));

    let result = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("execute_turn");

    assert!(
        result.text().contains("OXISECRET42"),
        "Response should contain secret word, got: {}",
        result.text()
    );
}

#[tokio::test]
#[ignore]
async fn e2e_file_write_tool_loop() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("created.txt");

    let (engine, _) = make_live_engine(tmp.path());
    let mut conv = Conversation::new();
    let prompt = format!(
        "Write a file at {} with content 'Hello from OxiCode'. Use the file_write tool.",
        target.display()
    );
    conv.push(Message::user(&prompt));

    let _ = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("execute_turn");

    assert!(target.exists(), "File should be created on disk");
    let content = std::fs::read_to_string(&target).unwrap();
    assert!(
        content.contains("Hello from OxiCode"),
        "File content should match, got: {content}"
    );
}

#[tokio::test]
#[ignore]
async fn e2e_file_edit_tool_loop() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("edit_me.txt");
    std::fs::write(&file, "Hello OLD world").unwrap();

    let (engine, _) = make_live_engine(tmp.path());
    let mut conv = Conversation::new();

    // First read the file (required by edit tool).
    let read_prompt = format!("Read the file at {}. Use the file_read tool.", file.display());
    conv.push(Message::user(&read_prompt));
    let _ = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("read turn");

    // Now edit it.
    let edit_prompt = format!(
        "Edit the file at {} by replacing 'OLD' with 'NEW'. Use the file_edit tool.",
        file.display()
    );
    conv.push(Message::user(&edit_prompt));
    let _ = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("edit turn");

    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("NEW"), "File should contain 'NEW', got: {content}");
    assert!(!content.contains("OLD"), "File should not contain 'OLD', got: {content}");
}

#[tokio::test]
#[ignore]
async fn e2e_bash_tool_loop() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, _) = make_live_engine(tmp.path());
    let mut conv = Conversation::new();
    conv.push(Message::user(
        "Run the bash command `echo RUSTCHECK99` and tell me the output. Use the bash tool.",
    ));

    let result = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("execute_turn");

    assert!(
        result.text().contains("RUSTCHECK99"),
        "Response should contain bash output, got: {}",
        result.text()
    );
}

#[tokio::test]
#[ignore]
async fn e2e_glob_grep_tool_loop() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("alpha.rs"), "fn alpha() {}").unwrap();
    std::fs::write(tmp.path().join("beta.rs"), "fn beta() {}").unwrap();
    std::fs::write(tmp.path().join("gamma.txt"), "not rust").unwrap();

    let (engine, _) = make_live_engine(tmp.path());
    let mut conv = Conversation::new();
    let prompt = format!(
        "Use the glob tool to find all .rs files in {}. List the filenames.",
        tmp.path().display()
    );
    conv.push(Message::user(&prompt));

    let result = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("execute_turn");

    let text = result.text();
    assert!(
        text.contains("alpha.rs") || text.contains("beta.rs"),
        "Response should mention .rs files, got: {text}"
    );
}

#[tokio::test]
#[ignore]
async fn e2e_multi_tool_sequence() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.txt");
    std::fs::write(&source, "original data 12345").unwrap();

    let (engine, _) = make_live_engine(tmp.path());
    let mut conv = Conversation::new();
    let prompt = format!(
        "Do these steps using tools:\n\
         1. Read the file at {}\n\
         2. Write a new file at {} with content 'copied: ' followed by what you read\n\
         Tell me the content of the new file.",
        source.display(),
        tmp.path().join("dest.txt").display()
    );
    conv.push(Message::user(&prompt));

    let result = tokio::time::timeout(Duration::from_secs(90), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("execute_turn");

    let dest = tmp.path().join("dest.txt");
    assert!(dest.exists(), "dest.txt should be created");

    let text = result.text();
    assert!(
        text.contains("original data") || text.contains("12345"),
        "Response should mention source content, got: {text}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// C. TUI ↔ Engine Bridge Tests
// ═══════════════════════════════════════════════════════════════════

/// Helper: run the TUI bridge loop (mirrors run_tui() from main.rs).
/// Returns collected CoreEvents after sending a user prompt.
async fn run_tui_bridge(
    engine: Arc<QueryEngine>,
    state_store: Arc<StateStore>,
    prompt: &str,
    auto_permission: Option<PermissionResponse>,
) -> Vec<CoreEvent> {
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);

    let engine_clone = engine.clone();
    let state_clone = state_store.clone();
    let core_tx_clone = core_tx.clone();
    let engine_handle = tokio::spawn(async move {
        let mut conversation = Conversation::new();
        while let Some(event) = ui_rx.recv().await {
            match event {
                UiEvent::UserInput(text) => {
                    let user_msg = Message::user(&text);
                    state_clone.push_message(user_msg.clone());
                    conversation.push(user_msg);

                    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);
                    let fwd_tx = core_tx_clone.clone();
                    let forwarder = tokio::spawn(async move {
                        while let Some(te) = turn_rx.recv().await {
                            let _ = fwd_tx.send(translate_turn_event(te)).await;
                        }
                    });

                    let result = engine_clone
                        .execute_turn(&mut conversation, Some(&turn_tx))
                        .await;
                    drop(turn_tx);
                    let _ = forwarder.await;

                    match result {
                        Ok(_) => {
                            let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                        }
                        Err(e) => {
                            let _ = core_tx_clone.send(CoreEvent::Error(e.to_string())).await;
                        }
                    }
                }
                UiEvent::Quit => break,
                _ => {}
            }
        }
    });

    // Send user input.
    ui_tx
        .send(UiEvent::UserInput(prompt.to_string()))
        .await
        .unwrap();

    // Collect events, auto-respond to permissions if configured.
    let mut events = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(60), core_rx.recv()).await {
            Ok(Some(CoreEvent::PermissionAsk { reply_tx, .. })) => {
                if let Some(resp) = auto_permission {
                    let _ = reply_tx.send(resp);
                } else {
                    let _ = reply_tx.send(PermissionResponse::AllowOnce);
                }
            }
            Ok(Some(event)) => {
                let is_done = matches!(event, CoreEvent::MessageComplete | CoreEvent::Error(_));
                events.push(event);
                if is_done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {
                events.push(CoreEvent::Error("Timeout waiting for events".to_string()));
                break;
            }
        }
    }

    ui_tx.send(UiEvent::Quit).await.ok();
    let _ = engine_handle.await;
    events
}

#[tokio::test]
#[ignore]
async fn e2e_tui_bridge_text_flow() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) = make_live_engine(tmp.path());

    let events = run_tui_bridge(
        engine,
        state_store.clone(),
        "Say exactly 'bridge-ok' and nothing else.",
        None,
    )
    .await;

    // Verify event sequence: StreamStart → TextDelta(s) → StreamEnd → MessageComplete.
    assert!(events.len() >= 3, "Need at least 3 events, got {}", events.len());
    assert!(
        matches!(events[0], CoreEvent::StreamStart),
        "First event should be StreamStart, got: {:?}",
        event_name(&events[0])
    );

    let text: String = events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(!text.is_empty(), "Should have TextDelta events");

    assert!(
        matches!(events.last(), Some(CoreEvent::MessageComplete)),
        "Last event should be MessageComplete"
    );

    // Verify state store has messages.
    let state = state_store.current();
    assert!(state.messages.len() >= 2, "State should have user + assistant messages");
}

#[tokio::test]
#[ignore]
async fn e2e_tui_bridge_tool_flow() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("data.txt"), "tool-flow-data").unwrap();

    let (engine, state_store) = make_live_engine(tmp.path());

    let events = run_tui_bridge(
        engine,
        state_store,
        &format!("Read the file at {} using the file_read tool.", tmp.path().join("data.txt").display()),
        None,
    )
    .await;

    let has_tool_start = events.iter().any(|e| matches!(e, CoreEvent::ToolUseStart { .. }));
    let has_tool_result = events.iter().any(|e| matches!(e, CoreEvent::ToolResult { .. }));

    assert!(has_tool_start, "Should have ToolUseStart event");
    assert!(has_tool_result, "Should have ToolResult event");
    assert!(
        matches!(events.last(), Some(CoreEvent::MessageComplete)),
        "Should end with MessageComplete"
    );
}

#[tokio::test]
#[ignore]
async fn e2e_tui_bridge_permission_flow() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) =
        make_live_engine_with_mode(tmp.path(), PermissionMode::ApprovalOnly);

    // Auto-respond AllowOnce — tool should execute.
    let events = run_tui_bridge(
        engine,
        state_store,
        "Run the bash command `echo PERMTEST` using the bash tool.",
        Some(PermissionResponse::AllowOnce),
    )
    .await;

    let has_tool_result = events.iter().any(|e| matches!(e, CoreEvent::ToolResult { .. }));
    assert!(has_tool_result, "Tool should execute after AllowOnce");
}

#[tokio::test]
#[ignore]
async fn e2e_tui_bridge_permission_deny() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) =
        make_live_engine_with_mode(tmp.path(), PermissionMode::ApprovalOnly);

    // Auto-respond Deny — tool should be skipped.
    let events = run_tui_bridge(
        engine,
        state_store,
        "Run the bash command `echo DENIED` using the bash tool.",
        Some(PermissionResponse::Deny),
    )
    .await;

    // When denied, the tool result should indicate an error/denial.
    let tool_results: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::ToolResult { is_error, content, .. } => Some((is_error, content)),
            _ => None,
        })
        .collect();

    if !tool_results.is_empty() {
        // If there's a tool result, it should be an error.
        assert!(
            tool_results.iter().any(|(err, _)| **err),
            "Denied tool should produce error result"
        );
    }
    // Either way, the conversation should complete.
    let completed = events.iter().any(|e| {
        matches!(e, CoreEvent::MessageComplete | CoreEvent::Error(_))
    });
    assert!(completed, "Should complete or error after denial");
}

#[tokio::test]
#[ignore]
async fn e2e_tui_bridge_error_recovery() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();

    // Use a provider with an invalid token to force an error.
    let provider = Arc::new(AnthropicProvider::new("invalid-token-xxx".to_string()).with_base_url(
        std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
    ));
    let state_store = Arc::new(StateStore::new(AppState::default()));
    let tool_registry = Arc::new(oxicode_tools::default_registry());
    let pipeline = Arc::new(PermissionPipeline::new(PermissionMode::Bypass, vec![]));
    let tool_context = make_tool_context(tmp.path());

    let engine = Arc::new(QueryEngine::new(
        provider,
        state_store.clone(),
        tool_registry,
        pipeline,
        tool_context,
        model_name(),
        16384,
        "Test".to_string(),
    ));

    let events = run_tui_bridge(engine, state_store, "Say hello", None).await;

    let has_error = events
        .iter()
        .any(|e| matches!(e, CoreEvent::Error(_)));
    assert!(has_error, "Invalid token should produce CoreEvent::Error");
}

/// Helper to name events for debug output.
fn event_name(e: &CoreEvent) -> &'static str {
    match e {
        CoreEvent::TextDelta(_) => "TextDelta",
        CoreEvent::StreamStart => "StreamStart",
        CoreEvent::StreamEnd => "StreamEnd",
        CoreEvent::Error(_) => "Error",
        CoreEvent::MessageComplete => "MessageComplete",
        CoreEvent::ToolUseStart { .. } => "ToolUseStart",
        CoreEvent::ToolResult { .. } => "ToolResult",
        CoreEvent::PermissionAsk { .. } => "PermissionAsk",
        CoreEvent::RateLimited { .. } => "RateLimited",
        CoreEvent::ThinkingDelta(_) => "ThinkingDelta",
    }
}

// ═══════════════════════════════════════════════════════════════════
// D. Session Persistence Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_session_save_and_resume() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let session_dir = tempfile::tempdir().unwrap();

    let (engine, _) = make_live_engine(tmp.path());
    let mut conv = Conversation::new();
    conv.push(Message::user("Say exactly 'session-test-ok'."));

    let result = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("execute_turn");

    // Build session with messages.
    let mut session = oxicode_session::Session::new(model_name());
    session.push_message(Message::user("Say exactly 'session-test-ok'."));
    session.push_message(result);

    // Save to temp dir.
    let path = oxicode_session::save_session(&session, Some(session_dir.path()))
        .expect("save should succeed");
    assert!(path.exists(), "Session file should exist");

    // Load it back.
    let loaded = oxicode_session::load_session(&session.id, Some(session_dir.path()))
        .expect("load should succeed");
    assert_eq!(loaded.messages.len(), session.messages.len());
    assert_eq!(loaded.model, session.model);
    assert_eq!(loaded.id, session.id);
}

#[tokio::test]
#[ignore]
async fn e2e_session_multi_turn_resume() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let session_dir = tempfile::tempdir().unwrap();

    let (engine, _) = make_live_engine(tmp.path());

    // Turn 1.
    let mut conv = Conversation::new();
    conv.push(Message::user("Remember this: my favorite number is 42."));
    let _r1 = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("turn 1");

    // Turn 2.
    conv.push(Message::user("What is my favorite number? Reply with just the number."));
    let r2 = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("turn 2");

    // Save session.
    let mut session = oxicode_session::Session::new(model_name());
    for msg in conv.api_messages() {
        session.push_message(msg.clone());
    }
    oxicode_session::save_session(&session, Some(session_dir.path())).expect("save");

    // Load and verify.
    let loaded = oxicode_session::load_session(&session.id, Some(session_dir.path())).expect("load");
    assert!(loaded.messages.len() >= 4, "Should have 4+ messages (2 user + 2 assistant)");

    // Verify the model remembered "42".
    assert!(
        r2.text().contains("42"),
        "Turn 2 should mention 42, got: {}",
        r2.text()
    );
}

#[tokio::test]
#[ignore]
async fn e2e_session_state_tracks_usage() {
    let _lock = LIVE_TEST_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) = make_live_engine(tmp.path());

    let mut conv = Conversation::new();
    conv.push(Message::user("Say hello"));
    let _ = tokio::time::timeout(Duration::from_secs(60), engine.execute_turn(&mut conv, None))
        .await
        .expect("timeout")
        .expect("execute_turn");

    let state = state_store.current();
    assert!(
        state.total_usage.input_tokens > 0 || state.total_usage.output_tokens > 0,
        "State should track usage. Input: {}, Output: {}",
        state.total_usage.input_tokens,
        state.total_usage.output_tokens
    );
}

// ═══════════════════════════════════════════════════════════════════
// E. CLI Binary Tests (slash commands tested via binary invocation)
// ═══════════════════════════════════════════════════════════════════

/// Get the path to the compiled oxicode binary.
fn binary_path() -> String {
    let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("debug")
        .join("oxicode");
    target_dir.to_string_lossy().to_string()
}

#[test]
fn e2e_cli_version_output() {
    // Build first.
    let status = std::process::Command::new("cargo")
        .args(["build", "-p", "oxicode-cli"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("build");
    assert!(status.success());

    let output = std::process::Command::new(&binary_path())
        .arg("--version")
        .output()
        .expect("run --version");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("oxicode"), "Version should contain 'oxicode', got: {stdout}");
}

#[test]
fn e2e_cli_help_lists_flags() {
    let output = std::process::Command::new(&binary_path())
        .arg("--help")
        .output()
        .expect("run --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--model"), "Help should list --model");
    assert!(stdout.contains("--prompt"), "Help should list --prompt");
    assert!(stdout.contains("--output"), "Help should list --output");
}

#[tokio::test]
#[ignore]
async fn e2e_cli_single_prompt_returns_text() {
    let output = std::process::Command::new(&binary_path())
        .args(["-p", "Say exactly 'cli-e2e-ok' and nothing else", "--no-onboard"])
        .envs(std::env::vars().filter(|(k, _)| {
            k.starts_with("ANTHROPIC_") || k == "PATH" || k == "HOME"
        }))
        .output()
        .expect("run single prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(!stdout.is_empty(), "Should produce output.\nstderr: {stderr}");
}

// ═══════════════════════════════════════════════════════════════════
// F. Context & System Prompt Tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn e2e_system_prompt_includes_sections() {
    let prompt = oxicode_core::system_prompt::assemble_system_prompt(
        Some("Global rules here"),
        Some("Project rules here"),
        Some("Skills available"),
        None,
    );
    assert!(prompt.contains("OxiCode"), "System prompt should contain 'OxiCode'");
    assert!(prompt.contains("Global"), "Should contain global section");
    assert!(prompt.contains("Project"), "Should contain project section");
    assert!(prompt.contains("Skills"), "Should contain skills section");
}

#[test]
fn e2e_provider_router_resolves_models() {
    // Only test if env vars are set.
    if std::env::var("ANTHROPIC_AUTH_TOKEN").is_err() {
        eprintln!("Skipping: no ANTHROPIC_AUTH_TOKEN");
        return;
    }

    let router = oxicode_api::ProviderRouter::from_env();
    let model = model_name();
    let resolved = router.resolve(&model);
    assert!(
        resolved.is_ok(),
        "Router should resolve model '{}': {:?}",
        model,
        resolved.err()
    );
    let resolved = resolved.unwrap();
    assert!(
        !resolved.model.is_empty(),
        "Resolved model name should not be empty"
    );
}

// ═══════════════════════════════════════════════════════════════════
// G. Config Tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn e2e_config_loads_defaults() {
    // Load with no config dir override — should return defaults without panicking.
    let settings = oxicode_config::load_settings(None);
    assert!(!settings.model.is_empty(), "Default model should be set");
    assert!(settings.max_tokens > 0, "Default max_tokens should be > 0");
}

#[test]
fn e2e_config_permission_modes_parse() {
    assert!(matches!(
        PermissionMode::parse("default"),
        PermissionMode::Default
    ));
    assert!(matches!(
        PermissionMode::parse("bypass"),
        PermissionMode::Bypass
    ));
    assert!(matches!(
        PermissionMode::parse("approval_only"),
        PermissionMode::ApprovalOnly
    ));
    // Unknown should fall back to default.
    assert!(matches!(
        PermissionMode::parse("unknown"),
        PermissionMode::Default
    ));
}
