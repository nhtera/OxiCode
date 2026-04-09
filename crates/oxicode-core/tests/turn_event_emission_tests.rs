//! Tests for TurnEvent emission during execute_turn.
//!
//! No API key needed — uses MockLlmProvider + mpsc channel.
//! Run with: `cargo test -p oxicode-core --test turn_event_emission_tests`

use std::sync::Arc;

use oxicode_api::MockLlmProvider;
use oxicode_core::turn_event::TurnEvent;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::PermissionPipeline;
use oxicode_state::StateStore;
use oxicode_tools::{ToolContext, ToolRegistry};

/// Build a QueryEngine with a mock provider and bypass permissions.
fn make_engine(provider: MockLlmProvider) -> QueryEngine {
    let pipeline = PermissionPipeline::new(
        oxicode_permissions::pipeline::PermissionMode::Bypass,
        vec![],
    );
    QueryEngine::new(
        Arc::new(provider),
        Arc::new(StateStore::default()),
        Arc::new(ToolRegistry::new()),
        Arc::new(pipeline),
        ToolContext::default(),
        "test-model".to_string(),
        4096,
        "You are a test assistant.".to_string(),
    )
}

/// Collect all TurnEvents emitted during execute_turn.
async fn collect_events(
    engine: &QueryEngine,
    conv: &mut Conversation,
) -> (
    Result<oxicode_common::Message, oxicode_common::OxiError>,
    Vec<TurnEvent>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(256);

    let result = engine.execute_turn(conv, Some(&tx)).await;
    drop(tx); // Close sender so rx.recv() returns None.

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    (result, events)
}

#[tokio::test]
async fn test_text_delta_events_emitted_during_stream() {
    let provider = MockLlmProvider::with_text("Hello from mock!");
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    let (result, events) = collect_events(&engine, &mut conv).await;
    assert!(result.is_ok());

    let text_deltas: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            TurnEvent::TextDelta(text) => Some(text),
            _ => None,
        })
        .collect();

    assert!(
        !text_deltas.is_empty(),
        "should emit at least one TextDelta event"
    );
    let combined: String = text_deltas.iter().map(|s| s.as_str()).collect();
    assert_eq!(combined, "Hello from mock!");
}

#[tokio::test]
async fn test_tool_use_start_event_emitted() {
    // Provider returns tool_use then text.
    let provider = MockLlmProvider::with_tool_then_text(
        "tool_123",
        "test_tool",
        serde_json::json!({"key": "value"}),
        "Done",
    );
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    let (result, events) = collect_events(&engine, &mut conv).await;
    assert!(result.is_ok());

    let tool_starts: Vec<(&String, &String)> = events
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolUseStart { id, name, .. } => Some((id, name)),
            _ => None,
        })
        .collect();

    assert!(!tool_starts.is_empty(), "should emit ToolUseStart event");
    assert_eq!(tool_starts[0].0, "tool_123");
    assert_eq!(tool_starts[0].1, "test_tool");
}

#[tokio::test]
async fn test_tool_result_event_emitted() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tr_id",
        "nonexistent_tool",
        serde_json::json!({}),
        "Done",
    );
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    let (result, events) = collect_events(&engine, &mut conv).await;
    assert!(result.is_ok());

    let tool_results: Vec<(&String, bool)> = events
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => Some((tool_use_id, *is_error)),
            _ => None,
        })
        .collect();

    assert!(!tool_results.is_empty(), "should emit ToolResult event");
    assert_eq!(tool_results[0].0, "tr_id", "tool_use_id should match");
}

#[tokio::test]
async fn test_turn_start_and_end_events_bracket_stream() {
    let provider = MockLlmProvider::with_text("test");
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    let (result, events) = collect_events(&engine, &mut conv).await;
    assert!(result.is_ok());

    // Find indices of TurnStart and TurnEnd.
    let start_idx = events
        .iter()
        .position(|e| matches!(e, TurnEvent::TurnStart));
    let end_idx = events.iter().rposition(|e| matches!(e, TurnEvent::TurnEnd));

    assert!(start_idx.is_some(), "should have TurnStart event");
    assert!(end_idx.is_some(), "should have TurnEnd event");
    assert!(
        start_idx.unwrap() < end_idx.unwrap(),
        "TurnStart should come before TurnEnd"
    );

    // TextDelta events should be between TurnStart and TurnEnd.
    for (i, event) in events.iter().enumerate() {
        if matches!(event, TurnEvent::TextDelta(_)) {
            assert!(
                i > start_idx.unwrap() && i < end_idx.unwrap(),
                "TextDelta at index {i} should be between TurnStart and TurnEnd"
            );
        }
    }
}

#[tokio::test]
async fn test_no_events_when_tx_is_none() {
    let provider = MockLlmProvider::with_text("no events");
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    // Call with event_tx=None — should complete without panic.
    let result = engine.execute_turn(&mut conv, None).await;
    assert!(
        result.is_ok(),
        "execute_turn with None event_tx should complete normally"
    );
}
