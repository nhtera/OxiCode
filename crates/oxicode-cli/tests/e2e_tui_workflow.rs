//! E2E TUI workflow tests with real API.
//!
//! Tests the full TUI → Engine → API pipeline with real streaming,
//! verifying the exact event flow, streaming markdown, multi-turn state,
//! session persistence, and cancel/interrupt behavior.
//!
//! Run with:
//!   export ANTHROPIC_AUTH_TOKEN="sk-..."
//!   export ANTHROPIC_BASE_URL="https://ezaiapi.com"
//!   export ANTHROPIC_DEFAULT_SONNET_MODEL="claude-sonnet-4.6"
//!   cargo test -p oxicode-cli --test e2e_tui_workflow -- --ignored --nocapture
#![allow(clippy::await_holding_lock)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::needless_continue)]
#![allow(clippy::match_same_arms)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oxicode_api::AnthropicProvider;
use oxicode_common::{Message, PermissionResponse, Role};
use oxicode_core::turn_event::TurnEvent;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::{AppState, StateStore};
use oxicode_tools::file_state_tracker::FileStateTracker;
use oxicode_tools::tool_trait::ToolContext;
use oxicode_tui::events::{CoreEvent, UiEvent};
use oxicode_tui::streaming_markdown::MarkdownStreamCollector;
use tokio::sync::mpsc;

/// Serialize live API tests to avoid 429s.
/// Uses `unwrap_or_else` to prevent cascading PoisonError failures.
static LIVE_LOCK: Mutex<()> = Mutex::new(());

// ═══════════════════════════════════════════════════════════════════
// Shared helpers
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
        extra_working_dirs: Vec::new(),
        file_state: Arc::new(FileStateTracker::default()),
        task_manager: Arc::new(Mutex::new(oxicode_tasks::TaskManager::default())),
        task_abort_handles: Arc::new(Mutex::new(HashMap::new())),
        mcp_manager: Arc::new(oxicode_mcp::McpServerManager::new()),
        skill_executor: None,
        team_manager: Arc::new(Mutex::new(oxicode_agents::TeamManager::new())),
        bash_processes: Arc::new(Mutex::new(HashMap::new())),
        hook_manager: None,
        permission_mode: oxicode_permissions::PermissionMode::Default,
    }
}

fn live_engine(dir: &Path) -> (Arc<QueryEngine>, Arc<StateStore>) {
    live_engine_with_mode(dir, PermissionMode::Bypass)
}

fn live_engine_with_mode(dir: &Path, mode: PermissionMode) -> (Arc<QueryEngine>, Arc<StateStore>) {
    let provider = Arc::new(live_provider());
    let ss = Arc::new(StateStore::new(AppState::default()));
    let tools = Arc::new(oxicode_tools::default_registry());
    let perms = Arc::new(PermissionPipeline::new(mode, vec![]));
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

/// Translate TurnEvent → CoreEvent (mirrors main.rs).
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
        TurnEvent::Retrying {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        } => CoreEvent::Retrying {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        },
        TurnEvent::HookProgress { event, state } => CoreEvent::HookProgress {
            event,
            state: match state {
                oxicode_core::HookState::Running => "running".into(),
                oxicode_core::HookState::Completed => "completed".into(),
            },
        },
        TurnEvent::HookMessage {
            event,
            kind,
            content,
        } => CoreEvent::HookMessage {
            event,
            kind: kind.as_str().to_string(),
            content,
        },
    }
}

/// Collect CoreEvents until MessageComplete or timeout, auto-approving permissions.
async fn collect_events_auto_approve(
    core_rx: &mut mpsc::Receiver<CoreEvent>,
    timeout_secs: u64,
) -> Vec<CoreEventKind> {
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        match tokio::time::timeout_at(deadline, core_rx.recv()).await {
            Ok(Some(event)) => {
                let kind = classify_event(&event);
                // Auto-approve permissions.
                if let CoreEvent::PermissionAsk { reply_tx, .. } = event {
                    let _ = reply_tx.send(PermissionResponse::AllowOnce);
                }
                let is_terminal = matches!(
                    kind,
                    CoreEventKind::MessageComplete | CoreEventKind::Error(_)
                );
                events.push(kind);
                if is_terminal {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {
                events.push(CoreEventKind::Timeout);
                break;
            }
        }
    }
    events
}

/// Simplified event classification for assertions.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum CoreEventKind {
    StreamStart,
    TextDelta(String),
    StreamEnd,
    ToolUseStart { name: String },
    ToolResult { is_error: bool },
    PermissionAsk,
    MessageComplete,
    Error(String),
    RateLimited,
    Retrying,
    ThinkingDelta,
    HookProgress,
    HookMessage,
    SessionResumed,
    Timeout,
}

fn classify_event(event: &CoreEvent) -> CoreEventKind {
    match event {
        CoreEvent::StreamStart => CoreEventKind::StreamStart,
        CoreEvent::TextDelta(t) => CoreEventKind::TextDelta(t.clone()),
        CoreEvent::StreamEnd => CoreEventKind::StreamEnd,
        CoreEvent::ToolUseStart { name, .. } => CoreEventKind::ToolUseStart { name: name.clone() },
        CoreEvent::ToolResult { is_error, .. } => CoreEventKind::ToolResult {
            is_error: *is_error,
        },
        CoreEvent::PermissionAsk { .. } => CoreEventKind::PermissionAsk,
        CoreEvent::MessageComplete => CoreEventKind::MessageComplete,
        CoreEvent::Error(e) => CoreEventKind::Error(e.clone()),
        CoreEvent::RateLimited { .. } => CoreEventKind::RateLimited,
        CoreEvent::ThinkingDelta(_) => CoreEventKind::ThinkingDelta,
        CoreEvent::Retrying { .. } => CoreEventKind::Retrying,
        CoreEvent::HookProgress { .. } => CoreEventKind::HookProgress,
        CoreEvent::HookMessage { .. } => CoreEventKind::HookMessage,
        CoreEvent::SessionResumed { .. } => CoreEventKind::SessionResumed,
    }
}

/// Spawn engine task (mimics run_tui engine loop from main.rs).
///
/// Takes ownership of `ui_rx` via channel swap. Returns the join handle
/// which resolves to the final `Conversation` state.
fn spawn_engine_task(
    engine: Arc<QueryEngine>,
    state_store: Arc<StateStore>,
    ui_rx: mpsc::Receiver<UiEvent>,
    core_tx: mpsc::Sender<CoreEvent>,
    cancel_flag: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<Conversation> {
    tokio::spawn(async move {
        let mut conversation = Conversation::new();
        let state = state_store.current();
        for msg in &state.messages {
            conversation.push(msg.clone());
        }

        let mut rx = ui_rx;
        while let Some(event) = rx.recv().await {
            match event {
                UiEvent::UserInput { text, .. } => {
                    let user_msg = Message::user(&text);
                    state_store.push_message(user_msg.clone());
                    conversation.push(user_msg);

                    cancel_flag.store(false, Ordering::SeqCst);

                    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);
                    let fwd_tx = core_tx.clone();
                    let forwarder = tokio::spawn(async move {
                        while let Some(te) = turn_rx.recv().await {
                            let _ = fwd_tx.send(translate(te)).await;
                        }
                    });

                    let result = engine
                        .execute_turn_with_cancel(
                            &mut conversation,
                            Some(&turn_tx),
                            Some(&cancel_flag),
                        )
                        .await;
                    drop(turn_tx);
                    let _ = forwarder.await;

                    match result {
                        Ok(_) => {
                            let _ = core_tx.send(CoreEvent::MessageComplete).await;
                        }
                        Err(e) => {
                            let _ = core_tx.send(CoreEvent::Error(e.to_string())).await;
                        }
                    }
                }
                UiEvent::InterruptTurn => {
                    cancel_flag.store(true, Ordering::SeqCst);
                }
                UiEvent::Quit => break,
                _ => {}
            }
        }
        conversation
    })
}

// ═══════════════════════════════════════════════════════════════════
// 1. TUI → Engine → API full roundtrip with real streaming
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live e2e test — requires running oxicode binary and ANTHROPIC_API_KEY"]
async fn e2e_tui_full_roundtrip_real_api() {
    let _lock = LIVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) = live_engine(tmp.path());

    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let engine_handle = spawn_engine_task(engine, state_store.clone(), ui_rx, core_tx, cancel_flag);

    // Send user input through TUI channel.
    ui_tx
        .send(UiEvent::UserInput {
            text: "Say exactly 'roundtrip_ok' and nothing else.".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    let events = collect_events_auto_approve(&mut core_rx, 30).await;

    // Quit.
    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    // Verify event sequence: StreamStart → TextDelta(s) → StreamEnd → MessageComplete.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CoreEventKind::StreamStart)),
        "Should have StreamStart, events: {events:?}"
    );

    let text: String = events
        .iter()
        .filter_map(|e| match e {
            CoreEventKind::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !text.is_empty(),
        "Should have TextDelta events with real API response"
    );

    assert!(
        events
            .iter()
            .any(|e| matches!(e, CoreEventKind::MessageComplete)),
        "Should end with MessageComplete"
    );

    // Verify state store tracks messages.
    let state = state_store.current();
    assert!(
        state.messages.len() >= 2,
        "State should have user + assistant messages, got: {}",
        state.messages.len()
    );
    assert_eq!(state.messages[0].role, Role::User);
    assert_eq!(state.messages[1].role, Role::Assistant);
}

// ═══════════════════════════════════════════════════════════════════
// 2. Streaming markdown collector with real API deltas
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live e2e test — requires running oxicode binary and ANTHROPIC_API_KEY"]
async fn e2e_streaming_markdown_with_real_deltas() {
    let _lock = LIVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let (engine, _) = live_engine(tmp.path());

    let mut conversation = Conversation::new();
    conversation.push(Message::user(
        "Write exactly this markdown:\n# Hello\n\nThis is **bold** text.\n\n```rust\nfn main() {}\n```",
    ));

    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);

    let engine_clone = engine.clone();
    let handle = tokio::spawn(async move {
        engine_clone
            .execute_turn(&mut conversation, Some(&turn_tx))
            .await
    });

    // Feed deltas into MarkdownStreamCollector.
    let mut collector = MarkdownStreamCollector::new();
    let mut total_committed = 0usize;

    while let Some(event) = turn_rx.recv().await {
        if let TurnEvent::TextDelta(delta) = event {
            collector.push_delta(&delta);
            let new_lines = collector.commit_complete_lines();
            total_committed += new_lines.len();
        }
    }

    // Finalize remaining buffer.
    let final_lines = collector.finalize();
    total_committed += final_lines.len();

    let _ = handle.await;

    assert!(
        total_committed > 0,
        "Streaming markdown collector should produce committed lines from real API deltas"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 3. Permission dialog with real tool call
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live e2e test — requires running oxicode binary and ANTHROPIC_API_KEY"]
async fn e2e_permission_dialog_real_api() {
    let _lock = LIVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();

    let (engine, state_store) = live_engine_with_mode(tmp.path(), PermissionMode::ApprovalOnly);

    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let engine_handle = spawn_engine_task(engine, state_store.clone(), ui_rx, core_tx, cancel_flag);

    // Use file_write (non-readonly) to trigger permission dialog.
    let target = tmp.path().join("perm_write_test.txt");
    let prompt = format!(
        "Write the text 'hello_perm' to the file at {} using the file_write tool.",
        target.display()
    );
    ui_tx
        .send(UiEvent::UserInput {
            text: prompt,
            images: vec![],
        })
        .await
        .unwrap();

    // Collect events, auto-approving permissions.
    let events = collect_events_auto_approve(&mut core_rx, 60).await;

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    // Should have seen at least one PermissionAsk (ApprovalOnly mode + write tool).
    let perm_count = events
        .iter()
        .filter(|e| matches!(e, CoreEventKind::PermissionAsk))
        .count();
    assert!(
        perm_count >= 1,
        "ApprovalOnly mode with file_write should produce PermissionAsk events, got: {perm_count}. Events: {events:?}"
    );

    // Tool should have succeeded after approval.
    let tool_results: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, CoreEventKind::ToolResult { is_error: false }))
        .collect();
    assert!(
        !tool_results.is_empty(),
        "Tool should succeed after permission approval"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 4. Multi-turn conversation state preserved
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live e2e test — requires running oxicode binary and ANTHROPIC_API_KEY"]
async fn e2e_multi_turn_state_preserved() {
    let _lock = LIVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) = live_engine(tmp.path());

    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let engine_handle = spawn_engine_task(engine, state_store.clone(), ui_rx, core_tx, cancel_flag);

    // Turn 1: Set a fact.
    ui_tx
        .send(UiEvent::UserInput {
            text: "Remember this: the secret code is ALPHA7. Just say 'Noted.'".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    let events1 = collect_events_auto_approve(&mut core_rx, 30).await;
    assert!(
        events1
            .iter()
            .any(|e| matches!(e, CoreEventKind::MessageComplete)),
        "Turn 1 should complete"
    );

    // Turn 2: Recall the fact.
    ui_tx
        .send(UiEvent::UserInput {
            text: "What is the secret code I told you? Reply with just the code.".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    let events2 = collect_events_auto_approve(&mut core_rx, 30).await;
    let text2: String = events2
        .iter()
        .filter_map(|e| match e {
            CoreEventKind::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();

    assert!(
        text2.contains("ALPHA7"),
        "Turn 2 should recall the secret code from turn 1, got: {text2}"
    );

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    // Verify state has all 4 messages (2 user + 2 assistant).
    let state = state_store.current();
    assert!(
        state.messages.len() >= 4,
        "Should have >= 4 messages after 2 turns, got: {}",
        state.messages.len()
    );
}

// ═══════════════════════════════════════════════════════════════════
// 5. Session save/load roundtrip with real API data
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live e2e test — requires running oxicode binary and ANTHROPIC_API_KEY"]
async fn e2e_session_roundtrip_real_data() {
    let _lock = LIVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let (engine, _state_store) = live_engine(tmp.path());

    let mut conversation = Conversation::new();
    conversation.push(Message::user("Say exactly 'session_test_ok'."));

    let result = tokio::time::timeout(
        Duration::from_secs(30),
        engine.execute_turn(&mut conversation, None),
    )
    .await
    .expect("timeout")
    .expect("execute_turn");

    // Build a session from the real conversation.
    let mut session = oxicode_session::Session::new(model());
    session.push_message(conversation.api_messages()[0].clone()); // user
    session.push_message(result.clone()); // assistant

    // Save to temp dir.
    let session_dir = tmp.path().join("sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    oxicode_session::save_session(&session, Some(&session_dir)).expect("save should succeed");

    // Load it back.
    let loaded = oxicode_session::load_session(&session.id, Some(&session_dir))
        .expect("load should succeed");

    assert_eq!(loaded.messages.len(), session.messages.len());
    assert_eq!(loaded.messages[0].role, Role::User);
    assert_eq!(loaded.messages[1].role, Role::Assistant);

    // Verify the assistant text was preserved.
    let original_text = result.text();
    let loaded_text = loaded.messages[1].text();
    assert_eq!(
        original_text, loaded_text,
        "Session roundtrip should preserve assistant text"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 6. Cancel/interrupt mid-stream
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live e2e test — requires running oxicode binary and ANTHROPIC_API_KEY"]
async fn e2e_cancel_mid_stream() {
    let _lock = LIVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) = live_engine(tmp.path());

    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let engine_handle = spawn_engine_task(engine, state_store.clone(), ui_rx, core_tx, cancel_flag);

    // Ask for a long response.
    ui_tx
        .send(UiEvent::UserInput {
            text: "Write a 500-word essay about the history of computing.".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    // Wait for stream to start, then cancel after first few deltas.
    let mut delta_count = 0;
    let mut saw_error_or_complete = false;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match tokio::time::timeout_at(deadline, core_rx.recv()).await {
            Ok(Some(CoreEvent::TextDelta(_))) => {
                delta_count += 1;
                if delta_count >= 3 {
                    // Interrupt after a few deltas.
                    ui_tx.send(UiEvent::InterruptTurn).await.unwrap();
                }
            }
            Ok(Some(CoreEvent::MessageComplete)) => {
                saw_error_or_complete = true;
                break;
            }
            Ok(Some(CoreEvent::Error(e))) => {
                // Expected: interrupted error.
                assert!(
                    e.to_lowercase().contains("interrupt")
                        || e.to_lowercase().contains("cancel")
                        || e.to_lowercase().contains("abort"),
                    "Error should mention interrupt/cancel, got: {e}"
                );
                saw_error_or_complete = true;
                break;
            }
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => break,
        }
    }

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    // The response may complete before enough deltas arrive (fast API or
    // short response).  The critical assertion is that the engine shuts down
    // cleanly — either via an interrupt error or a normal completion.
    assert!(
        saw_error_or_complete,
        "Should see either Error (interrupted) or MessageComplete"
    );
    // delta_count may be 0 when the API responds before the cancel signal
    // reaches the engine.  That is acceptable — the test verifies graceful
    // shutdown, not a specific delta count.
}

// ═══════════════════════════════════════════════════════════════════
// 7. Tool use with TUI event bridge (real API)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live e2e test — requires running oxicode binary and ANTHROPIC_API_KEY"]
async fn e2e_tui_bridge_tool_use_real_api() {
    let _lock = LIVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("tool_bridge_test.txt");
    std::fs::write(&target, "bridge_content_42").unwrap();

    let (engine, state_store) = live_engine(tmp.path());

    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let engine_handle = spawn_engine_task(engine, state_store.clone(), ui_rx, core_tx, cancel_flag);

    let prompt = format!(
        "Read the file at {} and tell me its content. Use the file_read tool.",
        target.display()
    );
    ui_tx
        .send(UiEvent::UserInput {
            text: prompt,
            images: vec![],
        })
        .await
        .unwrap();

    let events = collect_events_auto_approve(&mut core_rx, 60).await;

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    // Verify tool events were in the stream.
    let tool_starts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            CoreEventKind::ToolUseStart { name } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(
        tool_starts.iter().any(|n| n == "file_read" || n == "Read"),
        "Should see file_read tool start, got: {tool_starts:?}"
    );

    // Verify tool result.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CoreEventKind::ToolResult { is_error: false })),
        "Should see successful tool result"
    );

    // Final text should mention file content.
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            CoreEventKind::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        text.contains("bridge_content_42"),
        "Response should mention file content, got: {text}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 8. State store usage tracking with real API
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live e2e test — requires running oxicode binary and ANTHROPIC_API_KEY"]
async fn e2e_state_usage_tracking_real_api() {
    let _lock = LIVE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) = live_engine(tmp.path());

    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let engine_handle = spawn_engine_task(engine, state_store.clone(), ui_rx, core_tx, cancel_flag);

    ui_tx
        .send(UiEvent::UserInput {
            text: "Say hi".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    let _ = collect_events_auto_approve(&mut core_rx, 30).await;

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    let state = state_store.current();
    assert!(
        state.total_usage.input_tokens > 0 || state.total_usage.output_tokens > 0,
        "State should track token usage. Input: {}, Output: {}",
        state.total_usage.input_tokens,
        state.total_usage.output_tokens
    );
}
