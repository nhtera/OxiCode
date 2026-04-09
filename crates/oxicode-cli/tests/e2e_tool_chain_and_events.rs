//! E2E tests: multi-tool chains, grep tool, and TUI event sequence validation.
//!
//! Extends the core E2E suite with deeper tool chaining and event flow tests.
//! Tests requiring API are gated behind `#[ignore]`.
//! Run with:
//!   export ANTHROPIC_AUTH_TOKEN="sk-..."
//!   export ANTHROPIC_BASE_URL="https://ezaiapi.com"
//!   export ANTHROPIC_DEFAULT_SONNET_MODEL="claude-sonnet-4.6"
//!   cargo test -p oxicode-cli --test e2e_tool_chain_and_events -- --ignored --nocapture

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oxicode_api::{AnthropicProvider, MockLlmProvider};
use oxicode_common::{Message, PermissionResponse};
use oxicode_core::turn_event::TurnEvent;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::{AppState, StateStore};
use oxicode_tools::file_state_tracker::FileStateTracker;
use oxicode_tools::tool_trait::ToolContext;
use oxicode_tui::events::{CoreEvent, UiEvent};
use tokio::sync::mpsc;

/// Serialize live API tests to avoid 429s.
static LIVE_LOCK: Mutex<()> = Mutex::new(());

// ═══════════════════════════════════════════════════════════════════
// Shared helpers (same pattern as e2e_core_workflow.rs)
// ═══════════════════════════════════════════════════════════════════

fn live_provider() -> AnthropicProvider {
    let token = std::env::var("ANTHROPIC_AUTH_TOKEN").expect("ANTHROPIC_AUTH_TOKEN required");
    let base_url = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
    AnthropicProvider::new(token).with_base_url(base_url)
}

fn model() -> String {
    std::env::var("ANTHROPIC_DEFAULT_SONNET_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string())
}

fn tool_ctx(dir: &Path) -> ToolContext {
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

fn live_engine(dir: &Path) -> (Arc<QueryEngine>, Arc<StateStore>) {
    let provider = Arc::new(live_provider());
    let ss = Arc::new(StateStore::new(AppState::default()));
    let tools = Arc::new(oxicode_tools::default_registry());
    let perms = Arc::new(PermissionPipeline::new(PermissionMode::Bypass, vec![]));
    let tc = tool_ctx(dir);

    let engine = Arc::new(QueryEngine::new(
        provider,
        ss.clone(),
        tools,
        perms,
        tc,
        model(),
        16384,
        "You are a helpful coding assistant. Be concise. Always use tools when asked.".into(),
    ));
    (engine, ss)
}

fn mock_engine(dir: &Path, mock: MockLlmProvider) -> (Arc<QueryEngine>, Arc<StateStore>) {
    let provider = Arc::new(mock);
    let ss = Arc::new(StateStore::new(AppState::default()));
    let tools = Arc::new(oxicode_tools::default_registry());
    let perms = Arc::new(PermissionPipeline::new(PermissionMode::Bypass, vec![]));
    let tc = tool_ctx(dir);

    let engine = Arc::new(QueryEngine::new(
        provider,
        ss.clone(),
        tools,
        perms,
        tc,
        "mock-model".into(),
        16384,
        "Test assistant.".into(),
    ));
    (engine, ss)
}

fn translate(te: TurnEvent) -> CoreEvent {
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

/// Run the TUI bridge loop and collect CoreEvents.
async fn bridge(engine: Arc<QueryEngine>, ss: Arc<StateStore>, prompt: &str) -> Vec<CoreEvent> {
    let (_ui_tx, mut _ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);

    let engine_c = engine.clone();
    let ss_c = ss.clone();
    let core_tx_c = core_tx.clone();
    let prompt_owned = prompt.to_string();

    let handle = tokio::spawn(async move {
        let mut conv = Conversation::new();
        let user_msg = Message::user(&prompt_owned);
        ss_c.push_message(user_msg.clone());
        conv.push(user_msg);

        let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);
        let fwd_tx = core_tx_c.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(te) = turn_rx.recv().await {
                let ce = translate(te);
                // Auto-approve permissions.
                if let CoreEvent::PermissionAsk { reply_tx, .. } = ce {
                    let _ = reply_tx.send(PermissionResponse::AllowOnce);
                } else {
                    let _ = fwd_tx.send(ce).await;
                }
            }
        });

        let result = engine_c.execute_turn(&mut conv, Some(&turn_tx)).await;
        drop(turn_tx);
        let _ = forwarder.await;

        match result {
            Ok(_) => {
                let _ = core_tx_c.send(CoreEvent::MessageComplete).await;
            }
            Err(e) => {
                let _ = core_tx_c.send(CoreEvent::Error(e.to_string())).await;
            }
        }
    });

    let mut events = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(90), core_rx.recv()).await {
            Ok(Some(event)) => {
                let done = matches!(event, CoreEvent::MessageComplete | CoreEvent::Error(_));
                events.push(event);
                if done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {
                events.push(CoreEvent::Error("Timeout".into()));
                break;
            }
        }
    }
    let _ = handle.await;
    events
}

// ═══════════════════════════════════════════════════════════════════
// A. Multi-Tool Chain: write → edit → read → verify
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_write_edit_read_chain() {
    let _lock = LIVE_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("chain.txt");

    let (engine, _) = live_engine(tmp.path());
    let mut conv = Conversation::new();

    // Step 1: Write the file.
    let prompt = format!(
        "Write a file at {} with content 'Hello ORIGINAL world'. Use the file_write tool.",
        file.display()
    );
    conv.push(Message::user(&prompt));
    let _ = tokio::time::timeout(
        Duration::from_secs(60),
        engine.execute_turn(&mut conv, None),
    )
    .await
    .expect("timeout")
    .expect("write turn");
    assert!(file.exists(), "File should exist after write");

    // Step 2: Read it (required before edit).
    conv.push(Message::user(&format!(
        "Read the file at {}. Use the file_read tool.",
        file.display()
    )));
    let _ = tokio::time::timeout(
        Duration::from_secs(60),
        engine.execute_turn(&mut conv, None),
    )
    .await
    .expect("timeout")
    .expect("read turn");

    // Step 3: Edit it.
    conv.push(Message::user(&format!(
        "Edit the file at {} by replacing 'ORIGINAL' with 'MODIFIED'. Use the file_edit tool.",
        file.display()
    )));
    let _ = tokio::time::timeout(
        Duration::from_secs(60),
        engine.execute_turn(&mut conv, None),
    )
    .await
    .expect("timeout")
    .expect("edit turn");

    // Step 4: Read again and verify.
    conv.push(Message::user(&format!(
        "Read the file at {} and tell me its content. Use the file_read tool.",
        file.display()
    )));
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        engine.execute_turn(&mut conv, None),
    )
    .await
    .expect("timeout")
    .expect("final read turn");

    let content = std::fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("MODIFIED"),
        "File should contain MODIFIED, got: {content}"
    );
    assert!(
        !content.contains("ORIGINAL"),
        "File should NOT contain ORIGINAL, got: {content}"
    );
    assert!(
        result.text().contains("MODIFIED"),
        "Response should mention MODIFIED, got: {}",
        result.text()
    );
}

// ═══════════════════════════════════════════════════════════════════
// B. Grep Tool E2E
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_grep_tool_finds_pattern() {
    let _lock = LIVE_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();

    // Create files with specific content.
    std::fs::write(tmp.path().join("alpha.rs"), "fn alpha_secret_function() {}").unwrap();
    std::fs::write(tmp.path().join("beta.rs"), "fn beta_function() {}").unwrap();
    std::fs::write(
        tmp.path().join("gamma.txt"),
        "this has alpha_secret_function too",
    )
    .unwrap();

    let (engine, _) = live_engine(tmp.path());
    let mut conv = Conversation::new();
    let prompt = format!(
        "Use the grep tool to search for the pattern 'alpha_secret_function' in {}. \
         Tell me which files contain this pattern.",
        tmp.path().display()
    );
    conv.push(Message::user(&prompt));

    let result = tokio::time::timeout(
        Duration::from_secs(60),
        engine.execute_turn(&mut conv, None),
    )
    .await
    .expect("timeout")
    .expect("grep turn");

    let text = result.text();
    assert!(
        text.contains("alpha.rs"),
        "Should find alpha.rs, got: {text}"
    );
    assert!(
        text.contains("gamma.txt"),
        "Should find gamma.txt, got: {text}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// C. Bash → File Read Chain
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_bash_then_file_read_chain() {
    let _lock = LIVE_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("data.txt"), "CHAIN_MARKER_77").unwrap();

    let (engine, _) = live_engine(tmp.path());
    let mut conv = Conversation::new();
    let prompt = format!(
        "First use the bash tool to run `ls {}`, then read one of the files you discover \
         using the file_read tool. Tell me its content.",
        tmp.path().display()
    );
    conv.push(Message::user(&prompt));

    let result = tokio::time::timeout(
        Duration::from_secs(90),
        engine.execute_turn(&mut conv, None),
    )
    .await
    .expect("timeout")
    .expect("chain turn");

    let text = result.text();
    assert!(
        text.contains("CHAIN_MARKER_77") || text.contains("data.txt"),
        "Should find the file content or name, got: {text}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// D. TUI Event Sequence Validation (text-only response)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_event_sequence_text_only() {
    let _lock = LIVE_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, ss) = live_engine(tmp.path());

    let events = bridge(engine, ss, "Say exactly 'event-seq-ok' and nothing else.").await;

    // Validate: StreamStart → TextDelta(s) → StreamEnd → MessageComplete
    assert!(!events.is_empty(), "Should have events");
    assert!(
        matches!(&events[0], CoreEvent::StreamStart),
        "First should be StreamStart, got: {:?}",
        event_tag(&events[0])
    );

    let text_count = events
        .iter()
        .filter(|e| matches!(e, CoreEvent::TextDelta(_)))
        .count();
    assert!(text_count > 0, "Should have TextDelta events");

    let has_stream_end = events.iter().any(|e| matches!(e, CoreEvent::StreamEnd));
    assert!(has_stream_end, "Should have StreamEnd");

    assert!(
        matches!(events.last(), Some(CoreEvent::MessageComplete)),
        "Last should be MessageComplete"
    );

    // StreamEnd must come before MessageComplete.
    let end_pos = events
        .iter()
        .position(|e| matches!(e, CoreEvent::StreamEnd))
        .unwrap();
    let complete_pos = events
        .iter()
        .position(|e| matches!(e, CoreEvent::MessageComplete))
        .unwrap();
    assert!(
        end_pos < complete_pos,
        "StreamEnd must precede MessageComplete"
    );
}

// ═══════════════════════════════════════════════════════════════════
// E. TUI Event Sequence Validation (tool use response)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_event_sequence_with_tool() {
    let _lock = LIVE_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("event_test.txt"), "event-data").unwrap();

    let (engine, ss) = live_engine(tmp.path());
    let events = bridge(
        engine,
        ss,
        &format!(
            "Read the file at {} using the file_read tool.",
            tmp.path().join("event_test.txt").display()
        ),
    )
    .await;

    // Should have tool events in the sequence.
    let has_tool_start = events
        .iter()
        .any(|e| matches!(e, CoreEvent::ToolUseStart { .. }));
    let has_tool_result = events
        .iter()
        .any(|e| matches!(e, CoreEvent::ToolResult { .. }));

    assert!(has_tool_start, "Should have ToolUseStart");
    assert!(has_tool_result, "Should have ToolResult");

    // ToolUseStart must precede ToolResult.
    let ts_pos = events
        .iter()
        .position(|e| matches!(e, CoreEvent::ToolUseStart { .. }))
        .unwrap();
    let tr_pos = events
        .iter()
        .position(|e| matches!(e, CoreEvent::ToolResult { .. }))
        .unwrap();
    assert!(ts_pos < tr_pos, "ToolUseStart must precede ToolResult");

    assert!(
        matches!(events.last(), Some(CoreEvent::MessageComplete)),
        "Should end with MessageComplete"
    );
}

// ═══════════════════════════════════════════════════════════════════
// F. Mock-based event sequence test (no API needed)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn event_sequence_mock_text_only() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = MockLlmProvider::with_text("Mock response OK");
    let (engine, _) = mock_engine(tmp.path(), mock);

    let mut conv = Conversation::new();
    conv.push(Message::user("test"));

    let (tx, mut rx) = mpsc::channel::<TurnEvent>(64);
    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    assert!(result.is_ok());
    let msg = result.unwrap();
    assert!(msg.text().contains("Mock response OK"));

    // Collect emitted events.
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }

    // Should have: TurnStart, TextDelta, TurnEnd
    assert!(events.iter().any(|e| matches!(e, TurnEvent::TurnStart)));
    assert!(events.iter().any(|e| matches!(e, TurnEvent::TextDelta(_))));
    assert!(events.iter().any(|e| matches!(e, TurnEvent::TurnEnd)));
}

#[tokio::test]
async fn event_sequence_mock_tool_then_text() {
    let tmp = tempfile::tempdir().unwrap();
    // Create a file so file_read tool succeeds.
    std::fs::write(tmp.path().join("test.txt"), "mock-file-content").unwrap();

    let mock = MockLlmProvider::with_tool_then_text(
        "tool_1",
        "Read",
        serde_json::json!({
            "file_path": tmp.path().join("test.txt").to_string_lossy()
        }),
        "I read the file.",
    );
    let (engine, _) = mock_engine(tmp.path(), mock);

    let mut conv = Conversation::new();
    conv.push(Message::user("read that file"));

    let (tx, mut rx) = mpsc::channel::<TurnEvent>(256);
    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    assert!(result.is_ok());

    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }

    // Tool events should be present.
    let has_tool_start = events
        .iter()
        .any(|e| matches!(e, TurnEvent::ToolUseStart { .. }));
    let has_tool_result = events
        .iter()
        .any(|e| matches!(e, TurnEvent::ToolResult { .. }));

    assert!(has_tool_start, "Should emit ToolUseStart");
    assert!(has_tool_result, "Should emit ToolResult");
}

// ═══════════════════════════════════════════════════════════════════
// G. Error Recovery: tool fails, LLM continues
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn e2e_tool_error_recovery() {
    let _lock = LIVE_LOCK.lock().expect("lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, _) = live_engine(tmp.path());
    let mut conv = Conversation::new();

    // Ask to read a nonexistent file — tool will fail, LLM should acknowledge error.
    conv.push(Message::user(
        "Read the file at /tmp/nonexistent_xyz_9999.txt using the file_read tool. \
         If the file doesn't exist, just say 'file not found'.",
    ));

    let result = tokio::time::timeout(
        Duration::from_secs(60),
        engine.execute_turn(&mut conv, None),
    )
    .await
    .expect("timeout")
    .expect("error recovery turn");

    let text = result.text().to_lowercase();
    assert!(
        text.contains("not found")
            || text.contains("doesn't exist")
            || text.contains("does not exist")
            || text.contains("error")
            || text.contains("no such file"),
        "Should acknowledge file not found, got: {text}"
    );
}

/// Event tag for debug output.
fn event_tag(e: &CoreEvent) -> &'static str {
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
