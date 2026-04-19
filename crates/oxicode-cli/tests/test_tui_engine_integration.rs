//! Integration tests for the TUI ↔ Engine bidirectional flow.
//!
//! Simulates the run_tui() pattern: UiEvent → engine task → TurnEvent →
//! forwarder → CoreEvent, without actual terminal rendering.
//!
//! No API key needed — uses MockLlmProvider.
//! Run with: `cargo test -p oxicode-cli --test test_tui_engine_integration`

use std::sync::Arc;

use oxicode_api::MockLlmProvider;
use oxicode_common::{Message, PermissionResponse};
use oxicode_core::turn_event::TurnEvent;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::StateStore;
use oxicode_tools::{ToolContext, ToolRegistry};
use oxicode_tui::events::{CoreEvent, UiEvent};
use tokio::sync::mpsc;

/// Replicate the translate_turn_event mapping from main.rs.
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

/// Build a test QueryEngine with mock provider and bypass permissions.
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

// ── Full bidirectional flow: UserInput → CoreEvents ────────────

#[tokio::test]
async fn test_user_input_produces_core_events() {
    let provider = MockLlmProvider::with_text("Hello from the engine!");
    let (engine, state_store) = make_engine(provider);

    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);

    // Spawn engine task (simplified version of run_tui engine loop).
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
        .send(UiEvent::UserInput {
            text: "Say hello".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    // Collect events until MessageComplete.
    let mut events = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
            Ok(Some(event)) => {
                let is_complete = matches!(event, CoreEvent::MessageComplete);
                events.push(event);
                if is_complete {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!("Timed out waiting for core events"),
        }
    }

    // Quit the engine.
    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    // Verify event sequence: StreamStart → TextDelta(s) → StreamEnd → MessageComplete.
    assert!(events.len() >= 3, "should have at least 3 events: StreamStart, TextDelta, StreamEnd, MessageComplete, got: {}", events.len());

    assert!(
        matches!(events[0], CoreEvent::StreamStart),
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
    assert_eq!(combined, "Hello from the engine!");

    assert!(
        matches!(events.last(), Some(CoreEvent::MessageComplete)),
        "last event should be MessageComplete"
    );

    // Verify state store has messages.
    let state = state_store.current();
    assert!(
        state.messages.len() >= 2,
        "state should have user + assistant messages, got: {}",
        state.messages.len()
    );
}

// ── Multiple sequential user inputs ────────────────────────────

#[tokio::test]
async fn test_multiple_sequential_user_inputs() {
    let responses = vec![
        oxicode_api::mock::text_response_events("Response 1"),
        oxicode_api::mock::text_response_events("Response 2"),
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

    // Send two messages sequentially.
    for (i, input) in ["First message", "Second message"].iter().enumerate() {
        ui_tx
            .send(UiEvent::UserInput {
                text: input.to_string(),
                images: vec![],
            })
            .await
            .unwrap();

        // Wait for MessageComplete.
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
                Ok(Some(CoreEvent::MessageComplete)) => break,
                Ok(Some(CoreEvent::Error(e))) => panic!("Error on message {}: {e}", i + 1),
                Ok(Some(_)) => continue,
                Ok(None) => panic!("Channel closed on message {}", i + 1),
                Err(_) => panic!("Timeout on message {}", i + 1),
            }
        }
    }

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;

    // State should have 4 messages (2 user + 2 assistant).
    let state = state_store.current();
    assert!(
        state.messages.len() >= 4,
        "should have >= 4 messages (2 user + 2 assistant), got: {}",
        state.messages.len()
    );
}

// ── Engine error propagated as CoreEvent::Error ────────────────

#[tokio::test]
async fn test_engine_error_forwarded_as_core_error() {
    let provider = MockLlmProvider::new(vec![oxicode_api::mock::error_response_events(
        "Internal server error",
    )]);
    let (engine, _state_store) = make_engine(provider);

    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(256);

    let engine_clone = engine.clone();
    let core_tx_clone = core_tx.clone();
    let engine_handle = tokio::spawn(async move {
        let mut conversation = Conversation::new();
        while let Some(event) = ui_rx.recv().await {
            match event {
                UiEvent::UserInput { text, .. } => {
                    let user_msg = Message::user(&text);
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

    ui_tx
        .send(UiEvent::UserInput {
            text: "trigger error".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    // Collect events — should see Error at some point (either from forwarder or from result).
    let mut saw_error = false;
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
            Ok(Some(CoreEvent::Error(_))) => {
                saw_error = true;
                // Continue collecting to drain.
            }
            Ok(Some(CoreEvent::MessageComplete)) => break,
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert!(saw_error, "should propagate API error as CoreEvent::Error");

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;
}

// ── Permission flow through the full TUI bridge ────────────────

#[tokio::test]
async fn test_permission_dialog_through_tui_bridge() {
    let provider = MockLlmProvider::with_tool_then_text(
        "tu_perm",
        "bash",
        serde_json::json!({"command": "echo test"}),
        "Done",
    );
    let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);
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

    ui_tx
        .send(UiEvent::UserInput {
            text: "run echo test".to_string(),
            images: vec![],
        })
        .await
        .unwrap();

    // Respond to permission asks from the CoreEvent stream.
    let mut saw_permission = false;
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), core_rx.recv()).await {
            Ok(Some(CoreEvent::PermissionAsk { reply_tx, .. })) => {
                let _ = reply_tx.send(PermissionResponse::AllowOnce);
                saw_permission = true;
            }
            Ok(Some(CoreEvent::MessageComplete)) => break,
            Ok(Some(CoreEvent::Error(_e))) => {
                // Errors during permission flow are acceptable (tool may fail).
                break;
            }
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => panic!("Timed out waiting for events"),
        }
    }

    assert!(
        saw_permission,
        "ApprovalOnly mode should produce PermissionAsk through the TUI bridge"
    );

    ui_tx.send(UiEvent::Quit).await.unwrap();
    let _ = engine_handle.await;
}
