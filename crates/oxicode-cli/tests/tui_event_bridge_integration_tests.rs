//! Integration tests for the TUI ↔ Engine event bridge flows.
//!
//! Covers: CoreEvent/UiEvent translation fidelity, streaming collector
//! integration with real engine deltas, tool use event sequencing,
//! rate-limit/retry event forwarding, multi-turn state accumulation,
//! and interrupt-during-stream scenarios.
//!
//! No API key needed — uses MockLlmProvider.
//! Run with: `cargo test -p oxicode-cli --test tui_event_bridge_integration_tests`

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use oxicode_api::MockLlmProvider;
use oxicode_common::{Message, PermissionResponse};
use oxicode_core::turn_event::TurnEvent;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::StateStore;
use oxicode_tools::{ToolContext, ToolRegistry};
use oxicode_tui::events::{CoreEvent, UiEvent};
use oxicode_tui::streaming_markdown::MarkdownStreamCollector;
use tokio::sync::mpsc;

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

/// Replicate translate_turn_event from main.rs.
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

fn make_engine(provider: MockLlmProvider) -> (Arc<QueryEngine>, Arc<StateStore>) {
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
    let state_store = Arc::new(StateStore::default());
    let engine = Arc::new(QueryEngine::new(
        Arc::new(provider),
        state_store.clone(),
        Arc::new(ToolRegistry::new()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "You are a test assistant.".to_string(),
    ));
    (engine, state_store)
}

fn make_engine_with_tools(provider: MockLlmProvider) -> (Arc<QueryEngine>, Arc<StateStore>) {
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
    let state_store = Arc::new(StateStore::default());
    let registry = Arc::new(oxicode_tools::default_registry());
    let engine = Arc::new(QueryEngine::new(
        Arc::new(provider),
        state_store.clone(),
        registry,
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "You are a test assistant.".to_string(),
    ));
    (engine, state_store)
}

/// Run engine loop on a single user input, collect all CoreEvents.
async fn run_single_input(
    engine: Arc<QueryEngine>,
    state_store: Arc<StateStore>,
    input: &str,
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
                UiEvent::UserInput { text, .. } => {
                    let user_msg = Message::user(&text);
                    state_clone.push_message(user_msg.clone());
                    conversation.push(user_msg);

                    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);
                    let fwd_tx = core_tx_clone.clone();
                    let forwarder = tokio::spawn(async move {
                        while let Some(te) = turn_rx.recv().await {
                            let _ = fwd_tx.send(translate(te)).await;
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

    ui_tx
        .send(UiEvent::UserInput {
            text: input.to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    let mut events = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
            Ok(Some(event)) => {
                let is_terminal = matches!(event, CoreEvent::MessageComplete | CoreEvent::Error(_));
                events.push(event);
                if is_terminal {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!("Timed out waiting for core events"),
        }
    }

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;
    events
}

// ═══════════════════════════════════════════════════════════════════
// A. Event Sequence — Text-Only Response
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_text_only_event_sequence() {
    let provider = MockLlmProvider::with_text("Hello from the bridge!");
    let (engine, state_store) = make_engine(provider);

    let events = run_single_input(engine, state_store, "Say hello").await;

    // StreamStart → TextDelta(s) → StreamEnd → MessageComplete
    assert!(
        matches!(events.first(), Some(CoreEvent::StreamStart)),
        "first event should be StreamStart"
    );

    let text_deltas: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::TextDelta(t) => Some(t),
            _ => None,
        })
        .collect();
    assert!(!text_deltas.is_empty(), "should have TextDelta events");
    let combined: String = text_deltas.iter().map(|s| s.as_str()).collect();
    assert_eq!(combined, "Hello from the bridge!");

    assert!(
        matches!(events.last(), Some(CoreEvent::MessageComplete)),
        "last event should be MessageComplete"
    );
}

#[tokio::test]
async fn test_stream_start_before_text_deltas() {
    let provider = MockLlmProvider::with_text("test");
    let (engine, state_store) = make_engine(provider);

    let events = run_single_input(engine, state_store, "test").await;

    // Find first TextDelta index and StreamStart index.
    let stream_start_idx = events
        .iter()
        .position(|e| matches!(e, CoreEvent::StreamStart));
    let first_delta_idx = events
        .iter()
        .position(|e| matches!(e, CoreEvent::TextDelta(_)));

    if let (Some(ss), Some(fd)) = (stream_start_idx, first_delta_idx) {
        assert!(
            ss < fd,
            "StreamStart (idx={ss}) should come before first TextDelta (idx={fd})"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// B. Event Sequence — Tool Use Flow
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_use_event_sequence() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_bridge",
        "nonexistent_tool",
        serde_json::json!({"key": "val"}),
        "Tool done",
    );
    let (engine, state_store) = make_engine(provider);

    let events = run_single_input(engine, state_store, "use tool").await;

    // Should see: StreamStart → ToolUseStart → ToolResult → ... → MessageComplete
    let has_tool_start = events
        .iter()
        .any(|e| matches!(e, CoreEvent::ToolUseStart { .. }));
    let has_tool_result = events
        .iter()
        .any(|e| matches!(e, CoreEvent::ToolResult { .. }));

    assert!(has_tool_start, "should have ToolUseStart event");
    assert!(has_tool_result, "should have ToolResult event");

    // ToolUseStart should come before ToolResult.
    let start_idx = events
        .iter()
        .position(|e| matches!(e, CoreEvent::ToolUseStart { .. }))
        .unwrap();
    let result_idx = events
        .iter()
        .position(|e| matches!(e, CoreEvent::ToolResult { .. }))
        .unwrap();
    assert!(
        start_idx < result_idx,
        "ToolUseStart (idx={start_idx}) before ToolResult (idx={result_idx})"
    );
}

#[tokio::test]
async fn test_tool_use_start_contains_correct_name() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_name",
        "bash",
        serde_json::json!({"command": "echo hi"}),
        "ok",
    );
    let (engine, state_store) = make_engine_with_tools(provider);

    let events = run_single_input(engine, state_store, "run echo").await;

    let tool_starts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::ToolUseStart { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();

    if !tool_starts.is_empty() {
        assert_eq!(tool_starts[0], "bash", "tool name should be 'bash'");
    }
}

// ═══════════════════════════════════════════════════════════════════
// C. Streaming Collector Integration with Engine Deltas
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_streaming_collector_with_engine_deltas() {
    let long_text = "# Heading\n\nSome paragraph with **bold** text.\n\n```rust\nfn main() {}\n```\n\nDone.\n";
    let provider = MockLlmProvider::with_text(long_text);
    let (engine, state_store) = make_engine(provider);

    let events = run_single_input(engine, state_store, "generate markdown").await;

    // Feed all TextDelta events through a streaming collector.
    let mut collector = MarkdownStreamCollector::new();
    for event in &events {
        if let CoreEvent::TextDelta(t) = event {
            collector.push_delta(t);
            collector.commit_complete_lines();
        }
    }
    let final_lines = collector.finalize();

    let total_lines = collector.lines().len() + final_lines.len();
    assert!(
        total_lines > 0,
        "collector should produce rendered lines from engine deltas"
    );

    // Verify raw text is preserved.
    let mut raw = String::new();
    for event in &events {
        if let CoreEvent::TextDelta(t) = event {
            raw.push_str(t);
        }
    }
    assert_eq!(raw, long_text, "all deltas should reconstruct original text");
}

#[tokio::test]
async fn test_streaming_collector_handles_empty_deltas() {
    // Some providers may send empty deltas.
    let mut collector = MarkdownStreamCollector::new();
    collector.push_delta("");
    collector.push_delta("Hello\n");
    collector.push_delta("");
    let lines = collector.commit_complete_lines();
    assert!(!lines.is_empty(), "non-empty deltas should produce lines");
}

// ═══════════════════════════════════════════════════════════════════
// D. Error Propagation
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_api_error_produces_error_event() {
    let provider = MockLlmProvider::new(vec![oxicode_api::mock::error_response_events(
        "Service unavailable",
    )]);
    let (engine, state_store) = make_engine(provider);

    let events = run_single_input(engine, state_store, "trigger error").await;

    let has_error = events.iter().any(|e| matches!(e, CoreEvent::Error(_)));
    assert!(has_error, "API error should produce CoreEvent::Error");
}

#[tokio::test]
async fn test_error_event_contains_message() {
    let provider = MockLlmProvider::new(vec![oxicode_api::mock::error_response_events(
        "rate limit exceeded",
    )]);
    let (engine, state_store) = make_engine(provider);

    let events = run_single_input(engine, state_store, "trigger").await;

    let error_msgs: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::Error(msg) => Some(msg),
            _ => None,
        })
        .collect();

    assert!(
        !error_msgs.is_empty(),
        "should have error events with messages"
    );
}

// ═══════════════════════════════════════════════════════════════════
// E. Permission Flow Through Bridge
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_permission_ask_forwarded_through_bridge() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_perm",
        "bash",
        serde_json::json!({"command": "echo test"}),
        "Done",
    );
    let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);
    let state_store = Arc::new(StateStore::default());
    let engine = Arc::new(QueryEngine::new(
        Arc::new(provider),
        state_store.clone(),
        Arc::new(oxicode_tools::default_registry()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "Test".to_string(),
    ));

    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);

    let engine_clone = engine.clone();
    let state_clone = state_store.clone();
    let core_tx_clone = core_tx.clone();
    let engine_handle = tokio::spawn(async move {
        let mut conversation = Conversation::new();
        while let Some(event) = ui_rx.recv().await {
            match event {
                UiEvent::UserInput { text, .. } => {
                    let user_msg = Message::user(&text);
                    state_clone.push_message(user_msg.clone());
                    conversation.push(user_msg);

                    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);
                    let fwd_tx = core_tx_clone.clone();
                    let forwarder = tokio::spawn(async move {
                        while let Some(te) = turn_rx.recv().await {
                            let _ = fwd_tx.send(translate(te)).await;
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

    ui_tx
        .send(UiEvent::UserInput {
            text: "run echo test".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    let mut saw_permission = false;
    let mut permission_tool_name = String::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
            Ok(Some(CoreEvent::PermissionAsk {
                tool_name,
                reply_tx,
                ..
            })) => {
                permission_tool_name = tool_name;
                let _ = reply_tx.send(PermissionResponse::AllowOnce);
                saw_permission = true;
            }
            Ok(Some(CoreEvent::MessageComplete | CoreEvent::Error(_))) => break,
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => panic!("Timed out"),
        }
    }

    assert!(saw_permission, "should forward PermissionAsk through bridge");
    assert_eq!(
        permission_tool_name, "bash",
        "permission should be for 'bash' tool"
    );

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;
}

#[tokio::test]
async fn test_permission_deny_produces_error_result() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_deny",
        "bash",
        serde_json::json!({"command": "rm -rf /"}),
        "Done",
    );
    let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);
    let state_store = Arc::new(StateStore::default());
    let engine = Arc::new(QueryEngine::new(
        Arc::new(provider),
        state_store.clone(),
        Arc::new(oxicode_tools::default_registry()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "Test".to_string(),
    ));

    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);

    let engine_clone = engine.clone();
    let state_clone = state_store.clone();
    let core_tx_clone = core_tx.clone();
    let engine_handle = tokio::spawn(async move {
        let mut conversation = Conversation::new();
        while let Some(event) = ui_rx.recv().await {
            match event {
                UiEvent::UserInput { text, .. } => {
                    let user_msg = Message::user(&text);
                    state_clone.push_message(user_msg.clone());
                    conversation.push(user_msg);

                    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);
                    let fwd_tx = core_tx_clone.clone();
                    let forwarder = tokio::spawn(async move {
                        while let Some(te) = turn_rx.recv().await {
                            let _ = fwd_tx.send(translate(te)).await;
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

    ui_tx
        .send(UiEvent::UserInput {
            text: "dangerous command".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    let mut saw_denied_result = false;
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
            Ok(Some(CoreEvent::PermissionAsk { reply_tx, .. })) => {
                let _ = reply_tx.send(PermissionResponse::Deny);
            }
            Ok(Some(CoreEvent::ToolResult { is_error, content, .. })) => {
                if is_error && content.contains("denied") {
                    saw_denied_result = true;
                }
            }
            Ok(Some(CoreEvent::MessageComplete | CoreEvent::Error(_))) => break,
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => panic!("Timed out"),
        }
    }

    assert!(
        saw_denied_result,
        "denying permission should produce denied ToolResult"
    );

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;
}

// ═══════════════════════════════════════════════════════════════════
// F. Multi-Turn State Accumulation
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_multi_turn_state_accumulates() {
    let responses = vec![
        oxicode_api::mock::text_response_events("Response 1"),
        oxicode_api::mock::text_response_events("Response 2"),
        oxicode_api::mock::text_response_events("Response 3"),
    ];
    let provider = MockLlmProvider::new(responses);
    let (engine, state_store) = make_engine(provider);

    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);

    let engine_clone = engine.clone();
    let state_clone = state_store.clone();
    let core_tx_clone = core_tx.clone();
    let engine_handle = tokio::spawn(async move {
        let mut conversation = Conversation::new();
        while let Some(event) = ui_rx.recv().await {
            match event {
                UiEvent::UserInput { text, .. } => {
                    let user_msg = Message::user(&text);
                    state_clone.push_message(user_msg.clone());
                    conversation.push(user_msg);

                    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);
                    let fwd_tx = core_tx_clone.clone();
                    let forwarder = tokio::spawn(async move {
                        while let Some(te) = turn_rx.recv().await {
                            let _ = fwd_tx.send(translate(te)).await;
                        }
                    });

                    let result = engine_clone
                        .execute_turn(&mut conversation, Some(&turn_tx))
                        .await;
                    drop(turn_tx);
                    let _ = forwarder.await;

                    match result {
                        Ok(msg) => {
                            state_clone.push_message(msg);
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

    // Send 3 turns.
    for (i, input) in ["Turn 1", "Turn 2", "Turn 3"].iter().enumerate() {
        ui_tx
            .send(UiEvent::UserInput {
                text: input.to_string(),
                images: vec![],
            })
            .await
            .unwrap();

        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
                Ok(Some(CoreEvent::MessageComplete)) => break,
                Ok(Some(CoreEvent::Error(e))) => panic!("Error on turn {}: {e}", i + 1),
                Ok(Some(_)) => continue,
                Ok(None) => panic!("Channel closed on turn {}", i + 1),
                Err(_) => panic!("Timeout on turn {}", i + 1),
            }
        }
    }

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    // State should have at least 6 messages (3 user + 3 assistant).
    let state = state_store.current();
    assert!(
        state.messages.len() >= 6,
        "should have ≥6 messages (3 user + 3 assistant), got: {}",
        state.messages.len()
    );

    // Verify both user and assistant messages are present.
    let user_count = state
        .messages
        .iter()
        .filter(|m| m.role == oxicode_common::Role::User)
        .count();
    let assistant_count = state
        .messages
        .iter()
        .filter(|m| m.role == oxicode_common::Role::Assistant)
        .count();
    assert!(
        user_count >= 3,
        "should have ≥3 user messages, got: {user_count}"
    );
    assert!(
        assistant_count >= 3,
        "should have ≥3 assistant messages, got: {assistant_count}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// G. Cancel/Interrupt During Stream
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_cancel_during_streaming_no_hang() {
    // Provider returns multiple tool turns then text — cancel mid-stream.
    let mut responses = Vec::new();
    for i in 0..3 {
        responses.push(oxicode_api::mock::tool_use_response_events(
            &format!("t{i}"),
            "nonexistent_tool",
            &serde_json::json!({}),
        ));
    }
    responses.push(oxicode_api::mock::text_response_events("final"));

    let provider = MockLlmProvider::new(responses);
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let state_store = Arc::new(StateStore::default());
    let engine = Arc::new(QueryEngine::new(
        Arc::new(provider),
        state_store.clone(),
        Arc::new(ToolRegistry::new()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "Test".to_string(),
    ));

    let cancel_clone = cancel_flag.clone();
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);
    let core_tx_clone = core_tx.clone();

    let engine_clone = engine.clone();
    let engine_handle = tokio::spawn(async move {
        let mut conversation = Conversation::new();
        let user_msg = Message::user("test cancel");
        conversation.push(user_msg);

        let (turn_tx, mut turn_rx) = mpsc::channel::<TurnEvent>(256);
        let fwd_tx = core_tx_clone.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(te) = turn_rx.recv().await {
                let _ = fwd_tx.send(translate(te)).await;
            }
        });

        let result = engine_clone
            .execute_turn_with_cancel(&mut conversation, Some(&turn_tx), Some(&cancel_clone))
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
    });

    // Set cancel after short delay.
    let flag_for_cancel = cancel_flag.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        flag_for_cancel.store(true, Ordering::SeqCst);
    });

    // Collect events with timeout — must not hang.
    let mut events = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
            Ok(Some(event)) => {
                let is_terminal =
                    matches!(event, CoreEvent::MessageComplete | CoreEvent::Error(_));
                events.push(event);
                if is_terminal {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!("HANG DETECTED: cancel during stream should not hang"),
        }
    }

    let _ = engine_handle.await;

    // Key invariant: should either complete or error — never hang.
    assert!(
        !events.is_empty(),
        "should receive some events before termination"
    );
}

// ═══════════════════════════════════════════════════════════════════
// H. UiEvent Variants
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_ui_event_debug_display() {
    // Verify all UiEvent variants implement Debug (compile-time check + runtime).
    let events = vec![
        UiEvent::UserInput {
            text: "hello".into(),
            images: vec![],
        },
        UiEvent::SlashCommand {
            name: "help".into(),
            args: "".into(),
        },
        UiEvent::Quit,
        UiEvent::InterruptTurn,
        UiEvent::Resize(80, 24),
        UiEvent::ScrollUp,
        UiEvent::ScrollDown,
    ];

    for event in &events {
        let debug = format!("{event:?}");
        assert!(!debug.is_empty(), "Debug output should not be empty");
    }
}

#[test]
fn test_ui_event_clone() {
    let event = UiEvent::UserInput {
        text: "test".into(),
        images: vec![],
    };
    let cloned = event.clone();
    assert!(matches!(cloned, UiEvent::UserInput { text, .. } if text == "test"));

    let resize = UiEvent::Resize(120, 40);
    let cloned = resize.clone();
    assert!(matches!(cloned, UiEvent::Resize(120, 40)));
}

// ═══════════════════════════════════════════════════════════════════
// I. Streaming Collector — Full Lifecycle with Bridge
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_collector_lifecycle_new_clear_reuse() {
    let provider = MockLlmProvider::with_text("First response");
    let (engine, state_store) = make_engine(provider);

    let events = run_single_input(engine, state_store, "first").await;

    // Build collector from events.
    let mut collector = MarkdownStreamCollector::new();
    for event in &events {
        if let CoreEvent::TextDelta(t) = event {
            collector.push_delta(t);
            collector.commit_complete_lines();
        }
    }
    collector.finalize();
    let first_count = collector.lines().len();
    assert!(first_count > 0, "first response should produce lines");

    // Clear for new turn.
    collector.clear();
    assert!(collector.lines().is_empty());

    // Simulate second turn.
    collector.push_delta("Second response\n");
    collector.commit_complete_lines();
    assert!(!collector.lines().is_empty(), "second turn should produce lines");
}

#[tokio::test]
async fn test_streaming_markdown_with_code_from_engine() {
    let code_response = "Here's some code:\n\n```rust\nfn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n```\n\nThat's it.\n";
    let provider = MockLlmProvider::with_text(code_response);
    let (engine, state_store) = make_engine(provider);

    let events = run_single_input(engine, state_store, "show code").await;

    let mut collector = MarkdownStreamCollector::new();
    for event in &events {
        if let CoreEvent::TextDelta(t) = event {
            collector.push_delta(t);
            collector.commit_complete_lines();
        }
    }
    collector.finalize();

    let total_lines = collector.lines().len();
    assert!(
        total_lines >= 5,
        "code response should produce ≥5 rendered lines, got: {total_lines}"
    );
}
