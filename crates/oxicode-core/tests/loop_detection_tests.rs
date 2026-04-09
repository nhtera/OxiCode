//! Tests for QueryEngine loop detection — MAX_TOOL_TURNS enforcement.
//!
//! No API key needed — uses MockLlmProvider.
//! Run with: `cargo test -p oxicode-core --test loop_detection_tests`

use std::sync::Arc;

use oxicode_api::mock::{text_response_events, tool_use_response_events};
use oxicode_api::{MockLlmProvider, StreamEvent};
use oxicode_common::StopReason;
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

/// Create a mock provider that always returns ToolUse (never EndTurn).
/// Each call returns a tool_use response with ToolUse stop_reason,
/// so the engine keeps looping until MAX_TOOL_TURNS.
fn looping_provider(iterations: usize) -> MockLlmProvider {
    let responses: Vec<Vec<StreamEvent>> = (0..iterations)
        .map(|i| {
            tool_use_response_events(
                &format!("tool_{i}"),
                "nonexistent_tool",
                &serde_json::json!({"iter": i}),
            )
        })
        .collect();
    MockLlmProvider::new(responses)
}

#[tokio::test]
async fn test_max_tool_turns_enforced() {
    // Create provider that returns ToolUse 100 times (well over the 50 limit).
    let provider = looping_provider(100);
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    // execute_turn should return an error after MAX_TOOL_TURNS (50).
    let result = engine.execute_turn(&mut conv, None).await;

    // Should hit the "Max tool turns exceeded" error.
    assert!(result.is_err(), "should error after max tool turns");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Max tool turns"),
        "error should mention max tool turns, got: {err_msg}"
    );
}

#[tokio::test]
async fn test_tool_turn_count_resets_between_calls() {
    // Provider: 3 tool calls then EndTurn, repeated twice.
    let mut responses = Vec::new();
    for i in 0..3 {
        responses.push(tool_use_response_events(
            &format!("t{i}"),
            "nonexistent_tool",
            &serde_json::json!({}),
        ));
    }
    responses.push(text_response_events("done first"));
    for i in 0..3 {
        responses.push(tool_use_response_events(
            &format!("t2_{i}"),
            "nonexistent_tool",
            &serde_json::json!({}),
        ));
    }
    responses.push(text_response_events("done second"));

    let provider = MockLlmProvider::new(responses);
    let engine = make_engine(provider);

    // First call.
    let mut conv1 = Conversation::new();
    let result1 = engine.execute_turn(&mut conv1, None).await;
    assert!(result1.is_ok(), "first call should succeed");

    // Second call should also succeed (turn count resets).
    let mut conv2 = Conversation::new();
    let result2 = engine.execute_turn(&mut conv2, None).await;
    assert!(
        result2.is_ok(),
        "second call should succeed — turn count should reset"
    );
}

#[tokio::test]
async fn test_normal_conversation_not_affected_by_limit() {
    // Provider returns EndTurn after 3 tool calls — well under 50 limit.
    let mut responses: Vec<Vec<StreamEvent>> = (0..3)
        .map(|i| {
            tool_use_response_events(&format!("t{i}"), "nonexistent_tool", &serde_json::json!({}))
        })
        .collect();
    responses.push(text_response_events("All done!"));

    let provider = MockLlmProvider::new(responses);
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    let result = engine.execute_turn(&mut conv, None).await;
    assert!(result.is_ok(), "normal conversation should complete fine");

    let msg = result.unwrap();
    assert_eq!(msg.stop_reason, Some(StopReason::EndTurn));
}

#[tokio::test]
async fn test_repetitive_tool_calls_detected() {
    // Mock provider returns identical ToolUse block each turn — engine should
    // eventually stop at MAX_TOOL_TURNS rather than looping forever.
    let responses: Vec<Vec<StreamEvent>> = (0..100)
        .map(|_| {
            tool_use_response_events(
                "same_id",
                "same_tool",
                &serde_json::json!({"same": "input"}),
            )
        })
        .collect();

    let provider = MockLlmProvider::new(responses);
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    let result = engine.execute_turn(&mut conv, None).await;
    assert!(result.is_err(), "repetitive loops should hit max turns");

    // Conversation shouldn't have exploded in size.
    // MAX_TOOL_TURNS is 50, so at most 50 iterations × 2 messages (assistant + tool result).
    assert!(
        conv.len() <= 102,
        "conversation should be bounded, got {} messages",
        conv.len()
    );
}

#[tokio::test]
async fn test_max_tool_turns_returns_error_message() {
    let provider = looping_provider(60);
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    let result = engine.execute_turn(&mut conv, None).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Max tool turns exceeded"),
        "should get clear error message, got: {msg}"
    );
}

#[tokio::test]
async fn test_simple_text_response_completes() {
    let provider = MockLlmProvider::with_text("Hello world");
    let engine = make_engine(provider);
    let mut conv = Conversation::new();

    let result = engine.execute_turn(&mut conv, None).await;
    assert!(result.is_ok());

    let msg = result.unwrap();
    assert_eq!(msg.stop_reason, Some(StopReason::EndTurn));
}
