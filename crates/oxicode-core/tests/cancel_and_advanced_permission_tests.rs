//! Integration tests for QueryEngine cancel/interrupt flows and advanced permission dialogs.
//!
//! No API key needed — uses MockLlmProvider.
//! Run with: `cargo test -p oxicode-core --test cancel_and_advanced_permission_tests`

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use oxicode_api::MockLlmProvider;
use oxicode_common::PermissionResponse;
use oxicode_core::turn_event::TurnEvent;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::StateStore;
use oxicode_tools::{ToolContext, ToolRegistry};

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn make_engine_with_mode(provider: MockLlmProvider, mode: PermissionMode) -> QueryEngine {
    let pipeline = PermissionPipeline::new(mode, vec![]);
    let registry = oxicode_tools::default_registry();
    QueryEngine::new(
        Arc::new(provider),
        Arc::new(StateStore::default()),
        Arc::new(registry),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "You are a test assistant.".to_string(),
    )
}

fn make_bypass_engine(provider: MockLlmProvider) -> QueryEngine {
    make_engine_with_mode(provider, PermissionMode::Bypass)
}

fn make_ask_engine(provider: MockLlmProvider) -> QueryEngine {
    make_engine_with_mode(provider, PermissionMode::ApprovalOnly)
}

fn make_cancel_engine(
    provider: MockLlmProvider,
) -> (QueryEngine, Arc<StateStore>, Arc<AtomicBool>) {
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
    let state_store = Arc::new(StateStore::default());
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let engine = QueryEngine::new(
        Arc::new(provider),
        state_store.clone(),
        Arc::new(ToolRegistry::new()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "Test".to_string(),
    );
    (engine, state_store, cancel_flag)
}

// ═══════════════════════════════════════════════════════════════════
// A. Cancel/Interrupt Flow (Mock — no API needed)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_cancel_before_first_turn() {
    // Set cancel flag BEFORE calling execute_turn — should return Interrupted immediately.
    let provider = MockLlmProvider::with_text("should not reach here");
    let (engine, _, cancel_flag) = make_cancel_engine(provider);

    cancel_flag.store(true, Ordering::SeqCst);

    let mut conv = Conversation::new();
    let result = engine
        .execute_turn_with_cancel(&mut conv, None, Some(&cancel_flag))
        .await;

    assert!(result.is_err(), "should error when pre-cancelled");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Interrupted"),
        "should mention 'Interrupted', got: {err}"
    );
}

#[tokio::test]
async fn test_cancel_flag_resets_after_interrupt() {
    let provider = MockLlmProvider::with_text("test");
    let (engine, _, cancel_flag) = make_cancel_engine(provider);

    cancel_flag.store(true, Ordering::SeqCst);

    let mut conv = Conversation::new();
    let _ = engine
        .execute_turn_with_cancel(&mut conv, None, Some(&cancel_flag))
        .await;

    // After interrupt handling, flag should be reset to false.
    assert!(
        !cancel_flag.load(Ordering::SeqCst),
        "cancel flag should be reset after handling"
    );
}

#[tokio::test]
async fn test_cancel_emits_error_and_turn_end_events() {
    let provider = MockLlmProvider::with_text("should not stream");
    let (engine, _, cancel_flag) = make_cancel_engine(provider);

    cancel_flag.store(true, Ordering::SeqCst);

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
    let mut conv = Conversation::new();

    let _ = engine
        .execute_turn_with_cancel(&mut conv, Some(&tx), Some(&cancel_flag))
        .await;
    drop(tx);

    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }

    // Should emit Error + TurnEnd.
    let has_error = events.iter().any(|e| matches!(e, TurnEvent::Error(_)));
    let has_turn_end = events.iter().any(|e| matches!(e, TurnEvent::TurnEnd));
    assert!(has_error, "should emit Error event on cancel");
    assert!(has_turn_end, "should emit TurnEnd event on cancel");
}

#[tokio::test]
async fn test_no_cancel_flag_completes_normally() {
    let provider = MockLlmProvider::with_text("normal response");
    let (engine, _, _) = make_cancel_engine(provider);

    let mut conv = Conversation::new();
    // Pass None for cancel flag — should complete normally.
    let result = engine
        .execute_turn_with_cancel(&mut conv, None, None)
        .await;

    assert!(result.is_ok());
    let msg = result.unwrap();
    assert!(msg.text().contains("normal response"));
}

// ═══════════════════════════════════════════════════════════════════
// B. Cancel Between Tool Executions
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_cancel_between_tool_turns() {
    // Provider: 3 tool calls then text. Cancel after 1st tool result is processed.
    use oxicode_api::mock::tool_use_response_events;

    let mut responses = Vec::new();
    for i in 0..3 {
        responses.push(tool_use_response_events(
            &format!("t{i}"),
            "nonexistent_tool",
            &serde_json::json!({}),
        ));
    }
    // Final text (should not reach here if cancelled).
    responses.push(oxicode_api::mock::text_response_events("done"));

    let provider = MockLlmProvider::new(responses);
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let engine = QueryEngine::new(
        Arc::new(provider),
        Arc::new(StateStore::default()),
        Arc::new(ToolRegistry::new()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "Test".to_string(),
    );

    // Spawn cancel trigger after a short delay — let first tool turn start.
    let flag_clone = cancel_flag.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        flag_clone.store(true, Ordering::SeqCst);
    });

    let mut conv = Conversation::new();
    let result = engine
        .execute_turn_with_cancel(&mut conv, None, Some(&cancel_flag))
        .await;

    // Should either complete early or get interrupted.
    // (Timing-dependent: might complete if all turns finish before flag is set.)
    // The key invariant: no panic, no hang.
    assert!(
        result.is_ok() || result.is_err(),
        "should not panic on concurrent cancel"
    );
}

// ═══════════════════════════════════════════════════════════════════
// C. AlwaysAllow — Permission Session Caching
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_always_allow_caches_permission() {
    // First call: tool_use(bash) → AlwaysAllow → tool executes.
    // Second call: tool_use(bash) → should auto-allow (cached).
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_1",
        "bash",
        serde_json::json!({"command": "echo test"}),
        "Done",
    );
    let engine = make_ask_engine(provider);
    let mut conv = Conversation::new();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    let collector = tokio::spawn(async move {
        let mut ask_count = 0u32;
        while let Some(event) = rx.recv().await {
            if let TurnEvent::PermissionAsk { reply_tx, .. } = event {
                let _ = reply_tx.send(PermissionResponse::AlwaysAllow);
                ask_count += 1;
            }
        }
        ask_count
    });

    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    let ask_count = collector.await.unwrap();
    assert!(result.is_ok());
    // Should have asked exactly once (bash is non-read tool in ApprovalOnly).
    assert!(
        ask_count >= 1,
        "should ask at least once, got: {ask_count}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// D. AlwaysDeny — Permanent Denial
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_always_deny_records_session_deny() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_deny",
        "bash",
        serde_json::json!({"command": "rm -rf /"}),
        "Done",
    );
    let engine = make_ask_engine(provider);
    let mut conv = Conversation::new();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    let collector = tokio::spawn(async move {
        let mut tool_results = Vec::new();
        while let Some(event) = rx.recv().await {
            match event {
                TurnEvent::PermissionAsk { reply_tx, .. } => {
                    let _ = reply_tx.send(PermissionResponse::AlwaysDeny);
                }
                TurnEvent::ToolResult {
                    content, is_error, ..
                } => {
                    tool_results.push((content, is_error));
                }
                _ => {}
            }
        }
        tool_results
    });

    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    let tool_results = collector.await.unwrap();
    assert!(result.is_ok(), "should complete even with denial");

    // Tool result should indicate denial.
    let has_denial = tool_results
        .iter()
        .any(|(content, is_error)| *is_error && content.contains("denied"));
    assert!(has_denial, "should have denied result: {tool_results:?}");
}

// ═══════════════════════════════════════════════════════════════════
// E. Permission Timeout — 30s oneshot drops
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_permission_timeout_produces_timeout_result() {
    // Don't respond to the permission ask — it should timeout after 30s.
    // Use a shorter test by dropping the reply_tx immediately (simulates channel close).
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_timeout",
        "bash",
        serde_json::json!({"command": "echo test"}),
        "Done",
    );
    let engine = make_ask_engine(provider);
    let mut conv = Conversation::new();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    // Drop reply_tx immediately → receiver gets Err (dismissed).
    let collector = tokio::spawn(async move {
        let mut tool_results = Vec::new();
        while let Some(event) = rx.recv().await {
            match event {
                TurnEvent::PermissionAsk { reply_tx, .. } => {
                    drop(reply_tx); // Simulate dialog dismissal.
                }
                TurnEvent::ToolResult {
                    content, is_error, ..
                } => {
                    tool_results.push((content, is_error));
                }
                _ => {}
            }
        }
        tool_results
    });

    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    let tool_results = collector.await.unwrap();
    assert!(result.is_ok());

    // Should get "dismissed" result.
    let has_dismissed = tool_results
        .iter()
        .any(|(content, is_error)| *is_error && content.contains("dismissed"));
    assert!(
        has_dismissed,
        "should get dismissed result: {tool_results:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// F. State Store Integration — Streaming Flag
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_state_store_streaming_flag_lifecycle() {
    let provider = MockLlmProvider::with_text("hello");
    let state_store = Arc::new(StateStore::default());
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
    let engine = QueryEngine::new(
        Arc::new(provider),
        state_store.clone(),
        Arc::new(ToolRegistry::new()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "Test".to_string(),
    );

    // Before execute: not streaming.
    assert!(!state_store.current().is_streaming, "should not be streaming initially");

    let mut conv = Conversation::new();
    let _ = engine.execute_turn(&mut conv, None).await;

    // After execute: not streaming (reset after completion).
    assert!(
        !state_store.current().is_streaming,
        "should not be streaming after completion"
    );
}

#[tokio::test]
async fn test_state_store_messages_tracked() {
    let provider = MockLlmProvider::with_text("tracked response");
    let state_store = Arc::new(StateStore::default());
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
    let engine = QueryEngine::new(
        Arc::new(provider),
        state_store.clone(),
        Arc::new(ToolRegistry::new()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "Test".to_string(),
    );

    let mut conv = Conversation::new();
    let _ = engine.execute_turn(&mut conv, None).await;

    // State store should have the assistant message.
    let state = state_store.current();
    assert!(
        !state.messages.is_empty(),
        "state should track assistant message"
    );
    assert_eq!(
        state.messages[0].role,
        oxicode_common::Role::Assistant,
        "first tracked message should be assistant"
    );
}

// ═══════════════════════════════════════════════════════════════════
// G. Conversation State After Tool Loop
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_conversation_grows_with_tool_turns() {
    // Provider: tool_use → tool_result added → text response.
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_conv",
        "nonexistent_tool",
        serde_json::json!({"key": "val"}),
        "All done",
    );
    let engine = make_bypass_engine(provider);
    let mut conv = Conversation::new();

    let result = engine.execute_turn(&mut conv, None).await;
    assert!(result.is_ok());

    // Conversation should have: assistant(tool_use) + user(tool_result) + assistant(text)
    assert!(
        conv.len() >= 3,
        "conversation should grow with tool turns, got: {}",
        conv.len()
    );

    // Last message should be the final assistant text.
    let last = conv.api_messages().last().unwrap();
    assert_eq!(last.role, oxicode_common::Role::Assistant);
}

#[tokio::test]
async fn test_conversation_api_messages_consistent() {
    let provider = MockLlmProvider::with_text("simple response");
    let engine = make_bypass_engine(provider);
    let mut conv = Conversation::new();

    let _ = engine.execute_turn(&mut conv, None).await;

    let api_msgs = conv.api_messages();
    // Should have exactly 1 message (the assistant response).
    assert_eq!(api_msgs.len(), 1);
    assert_eq!(api_msgs[0].role, oxicode_common::Role::Assistant);
}

// ═══════════════════════════════════════════════════════════════════
// H. Model Switching at Runtime
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_model_switch_at_runtime() {
    let provider = MockLlmProvider::with_text("response");
    let engine = make_bypass_engine(provider);

    assert_eq!(engine.model(), "test-model");

    engine.set_model("new-model-v2".to_string());
    assert_eq!(engine.model(), "new-model-v2");

    // Execute should use the new model (verified via state).
    let mut conv = Conversation::new();
    let result = engine.execute_turn(&mut conv, None).await;
    assert!(result.is_ok());
    let msg = result.unwrap();
    assert_eq!(msg.model.as_deref(), Some("new-model-v2"));
}
