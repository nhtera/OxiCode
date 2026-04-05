//! Integration tests for the permission dialog flow.
//!
//! Tests the full cycle: engine executes tool → permission Ask → oneshot channel
//! → user response → tool execution or denial.
//!
//! No API key needed — uses MockLlmProvider with ApprovalOnly permission pipeline.
//! Run with: `cargo test -p oxicode-core --test permission_dialog_flow_tests`

use std::sync::Arc;

use oxicode_api::MockLlmProvider;
use oxicode_common::PermissionResponse;
use oxicode_core::turn_event::TurnEvent;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::StateStore;
use oxicode_tools::ToolContext;

/// Build a QueryEngine with ApprovalOnly permissions (forces Ask for non-read tools).
fn make_ask_engine(provider: MockLlmProvider) -> QueryEngine {
    let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);
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

/// Build a QueryEngine with Bypass permissions (auto-allows everything).
fn make_bypass_engine(provider: MockLlmProvider) -> QueryEngine {
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
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

// ── AllowOnce → tool executes ──────────────────────────────────

#[tokio::test]
async fn test_permission_allow_once_executes_tool() {
    // Provider: tool_use(bash echo) → text response.
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_1",
        "bash",
        serde_json::json!({"command": "echo hello"}),
        "Done",
    );
    let engine = make_ask_engine(provider);
    let mut conv = Conversation::new();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    // Spawn a task that auto-responds AllowOnce to permission asks.
    let responder = tokio::spawn(async move {
        let mut permission_count = 0u32;
        while let Some(event) = rx.recv().await {
            if let TurnEvent::PermissionAsk { reply_tx, .. } = event {
                let _ = reply_tx.send(PermissionResponse::AllowOnce);
                permission_count += 1;
            }
        }
        permission_count
    });

    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    let perm_count = responder.await.unwrap();
    assert!(result.is_ok(), "should succeed when permission is granted");
    assert!(
        perm_count >= 1,
        "should have asked for at least 1 permission, got: {perm_count}"
    );
}

// ── Deny → tool denied ────────────────────────────────────────

#[tokio::test]
async fn test_permission_deny_blocks_tool() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_deny",
        "bash",
        serde_json::json!({"command": "rm -rf /tmp/test"}),
        "Done",
    );
    let engine = make_ask_engine(provider);
    let mut conv = Conversation::new();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    // Collect tool results to verify denial message.
    let collector = tokio::spawn(async move {
        let mut tool_results = Vec::new();
        let mut permission_asks = 0u32;
        while let Some(event) = rx.recv().await {
            match event {
                TurnEvent::PermissionAsk { reply_tx, .. } => {
                    let _ = reply_tx.send(PermissionResponse::Deny);
                    permission_asks += 1;
                }
                TurnEvent::ToolResult {
                    content, is_error, ..
                } => {
                    tool_results.push((content, is_error));
                }
                _ => {}
            }
        }
        (permission_asks, tool_results)
    });

    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    let (perm_count, tool_results) = collector.await.unwrap();
    assert!(result.is_ok(), "conversation should complete even with denied tools");
    assert!(perm_count >= 1, "should have asked permission");

    // At least one tool result should be an error with "denied" message.
    let has_denial = tool_results
        .iter()
        .any(|(content, is_error)| *is_error && content.contains("denied"));
    assert!(
        has_denial,
        "should have a denied tool result, got: {tool_results:?}"
    );
}

// ── No event_tx → auto-deny ───────────────────────────────────

#[tokio::test]
async fn test_no_event_tx_auto_denies_permission() {
    // With ApprovalOnly + no event_tx, Ask decisions should auto-deny.
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_auto",
        "bash",
        serde_json::json!({"command": "echo test"}),
        "Done",
    );
    let engine = make_ask_engine(provider);
    let mut conv = Conversation::new();

    // Execute with None event_tx — permission Ask should auto-deny.
    let result = engine.execute_turn(&mut conv, None).await;
    assert!(
        result.is_ok(),
        "should complete without panic when auto-denying"
    );
}

// ── Dropped oneshot → "dismissed" ──────────────────────────────

#[tokio::test]
async fn test_dropped_reply_channel_gives_dismissed() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_drop",
        "bash",
        serde_json::json!({"command": "echo test"}),
        "Done",
    );
    let engine = make_ask_engine(provider);
    let mut conv = Conversation::new();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    // Drop the reply_tx without sending — simulates dismissed dialog.
    let collector = tokio::spawn(async move {
        let mut tool_results = Vec::new();
        while let Some(event) = rx.recv().await {
            match event {
                TurnEvent::PermissionAsk { reply_tx, .. } => {
                    drop(reply_tx); // Dismiss without responding.
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
    assert!(result.is_ok(), "should complete even when dialog dismissed");

    // Tool result should indicate dismissal.
    let has_dismissed = tool_results
        .iter()
        .any(|(content, is_error)| *is_error && content.contains("dismissed"));
    assert!(
        has_dismissed,
        "should get 'dismissed' tool result, got: {tool_results:?}"
    );
}

// ── Bypass mode → no permission ask ────────────────────────────

#[tokio::test]
async fn test_bypass_mode_no_permission_ask() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_bypass",
        "bash",
        serde_json::json!({"command": "echo hello"}),
        "Done",
    );
    let engine = make_bypass_engine(provider);
    let mut conv = Conversation::new();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    let collector = tokio::spawn(async move {
        let mut permission_count = 0u32;
        while let Some(event) = rx.recv().await {
            if matches!(event, TurnEvent::PermissionAsk { .. }) {
                permission_count += 1;
            }
        }
        permission_count
    });

    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    let perm_count = collector.await.unwrap();
    assert!(result.is_ok());
    assert_eq!(
        perm_count, 0,
        "Bypass mode should not emit PermissionAsk events"
    );
}

// ── ReadOnly tools pass even in ApprovalOnly mode ──────────────

#[tokio::test]
async fn test_readonly_tool_no_permission_in_approval_mode() {
    // file_read is ReadOnly — should auto-allow even in ApprovalOnly.
    let tmp = tempfile::tempdir().unwrap();
    let test_file = tmp.path().join("test.txt");
    std::fs::write(&test_file, "test content").unwrap();

    let file_path = test_file.to_str().unwrap();
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_read",
        "file_read",
        serde_json::json!({"file_path": file_path}),
        "Done",
    );

    let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);
    let registry = oxicode_tools::default_registry();
    let tool_context = ToolContext {
        working_dir: tmp.path().to_path_buf(),
        ..ToolContext::default()
    };
    let engine = QueryEngine::new(
        Arc::new(provider),
        Arc::new(StateStore::default()),
        Arc::new(registry),
        Arc::new(pipeline),
        tool_context,
        "test-model".to_string(),
        4096,
        "Test".to_string(),
    );

    let mut conv = Conversation::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    let collector = tokio::spawn(async move {
        let mut permission_count = 0u32;
        let mut tool_results = Vec::new();
        while let Some(event) = rx.recv().await {
            match event {
                TurnEvent::PermissionAsk { reply_tx, .. } => {
                    let _ = reply_tx.send(PermissionResponse::AllowOnce);
                    permission_count += 1;
                }
                TurnEvent::ToolResult {
                    content, is_error, ..
                } => {
                    tool_results.push((content, is_error));
                }
                _ => {}
            }
        }
        (permission_count, tool_results)
    });

    let result = engine.execute_turn(&mut conv, Some(&tx)).await;
    drop(tx);

    let (perm_count, tool_results) = collector.await.unwrap();
    assert!(result.is_ok());
    assert_eq!(
        perm_count, 0,
        "ReadOnly tools should not need permission, got {perm_count}"
    );

    // Tool result should contain the file content (not an error).
    let has_content = tool_results
        .iter()
        .any(|(content, is_error)| !is_error && content.contains("test content"));
    assert!(
        has_content,
        "file_read should succeed without permission, got: {tool_results:?}"
    );
}
