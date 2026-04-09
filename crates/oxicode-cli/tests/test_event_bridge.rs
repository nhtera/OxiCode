//! Integration tests for the TurnEvent → CoreEvent bridge (forwarder pattern).
//!
//! Tests the channel-based forwarder that translates `TurnEvent` (from oxicode-core)
//! into `CoreEvent` (for oxicode-tui) — the critical integration point between
//! the core engine and the TUI.
//!
//! No API key needed — uses mock channels.
//! Run with: `cargo test -p oxicode-cli --test test_event_bridge`

use oxicode_common::PermissionResponse;
use oxicode_core::TurnEvent;
use oxicode_tui::events::CoreEvent;
use tokio::sync::{mpsc, oneshot};

/// Replicate the `translate_turn_event` mapping from main.rs.
/// This tests the contract that the forwarder must maintain.
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

/// Spawn a forwarder task matching the pattern in main.rs run_tui().
async fn spawn_forwarder(
    mut turn_rx: mpsc::Receiver<TurnEvent>,
    core_tx: mpsc::Sender<CoreEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(te) = turn_rx.recv().await {
            let _ = core_tx.send(translate_turn_event(te)).await;
        }
    })
}

// ── TextDelta mapping ──────────────────────────────────────────

#[tokio::test]
async fn test_text_delta_forwarded() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;

    turn_tx
        .send(TurnEvent::TextDelta("Hello world".to_string()))
        .await
        .unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    assert!(
        matches!(event, CoreEvent::TextDelta(ref t) if t == "Hello world"),
        "TextDelta should be forwarded with same text"
    );

    handle.await.unwrap();
}

// ── TurnStart → StreamStart ────────────────────────────────────

#[tokio::test]
async fn test_turn_start_maps_to_stream_start() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;
    turn_tx.send(TurnEvent::TurnStart).await.unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    assert!(
        matches!(event, CoreEvent::StreamStart),
        "TurnStart should map to StreamStart"
    );
    handle.await.unwrap();
}

// ── TurnEnd → StreamEnd ────────────────────────────────────────

#[tokio::test]
async fn test_turn_end_maps_to_stream_end() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;
    turn_tx.send(TurnEvent::TurnEnd).await.unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    assert!(
        matches!(event, CoreEvent::StreamEnd),
        "TurnEnd should map to StreamEnd"
    );
    handle.await.unwrap();
}

// ── ToolUseStart passthrough ───────────────────────────────────

#[tokio::test]
async fn test_tool_use_start_forwarded() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;
    turn_tx
        .send(TurnEvent::ToolUseStart {
            id: "tu_1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        })
        .await
        .unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    match event {
        CoreEvent::ToolUseStart { id, name, input } => {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "bash");
            assert_eq!(input, serde_json::json!({"command": "ls"}));
        }
        _ => panic!("Expected ToolUseStart"),
    }
    handle.await.unwrap();
}

// ── ToolResult passthrough ─────────────────────────────────────

#[tokio::test]
async fn test_tool_result_forwarded() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;
    turn_tx
        .send(TurnEvent::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "file contents".to_string(),
            is_error: false,
        })
        .await
        .unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    match event {
        CoreEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tu_1");
            assert_eq!(content, "file contents");
            assert!(!is_error);
        }
        _ => panic!("Expected ToolResult"),
    }
    handle.await.unwrap();
}

// ── ToolResult with error ──────────────────────────────────────

#[tokio::test]
async fn test_tool_result_error_forwarded() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;
    turn_tx
        .send(TurnEvent::ToolResult {
            tool_use_id: "tu_err".to_string(),
            content: "Tool error: not found".to_string(),
            is_error: true,
        })
        .await
        .unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    match event {
        CoreEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tu_err");
            assert!(is_error);
            assert!(content.contains("not found"));
        }
        _ => panic!("Expected error ToolResult"),
    }
    handle.await.unwrap();
}

// ── PermissionAsk passthrough with oneshot ──────────────────────

#[tokio::test]
async fn test_permission_ask_forwarded_with_reply_channel() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;

    let (reply_tx, _reply_rx) = oneshot::channel::<PermissionResponse>();
    turn_tx
        .send(TurnEvent::PermissionAsk {
            tool_name: "bash".to_string(),
            input_summary: "rm -rf /tmp/test".to_string(),
            prompt: "Allow bash execution?".to_string(),
            reply_tx,
        })
        .await
        .unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    match event {
        CoreEvent::PermissionAsk {
            tool_name,
            input_summary,
            prompt,
            reply_tx,
        } => {
            assert_eq!(tool_name, "bash");
            assert_eq!(input_summary, "rm -rf /tmp/test");
            assert_eq!(prompt, "Allow bash execution?");
            // Verify the oneshot channel is functional.
            reply_tx.send(PermissionResponse::AllowOnce).unwrap();
        }
        _ => panic!("Expected PermissionAsk"),
    }
    handle.await.unwrap();
}

// ── Error passthrough ──────────────────────────────────────────

#[tokio::test]
async fn test_error_forwarded() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;
    turn_tx
        .send(TurnEvent::Error("API failure".to_string()))
        .await
        .unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    assert!(
        matches!(event, CoreEvent::Error(ref e) if e == "API failure"),
        "Error should be forwarded with same message"
    );
    handle.await.unwrap();
}

// ── RateLimited passthrough ────────────────────────────────────

#[tokio::test]
async fn test_rate_limited_forwarded() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;
    turn_tx
        .send(TurnEvent::RateLimited {
            message: "429 Too Many Requests".to_string(),
            attempt: 2,
            max_retries: 5,
            retry_in_secs: 30.0,
        })
        .await
        .unwrap();
    drop(turn_tx);

    let event = core_rx.recv().await.unwrap();
    match event {
        CoreEvent::RateLimited {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        } => {
            assert_eq!(message, "429 Too Many Requests");
            assert_eq!(attempt, 2);
            assert_eq!(max_retries, 5);
            assert!((retry_in_secs - 30.0).abs() < f64::EPSILON);
        }
        _ => panic!("Expected RateLimited"),
    }
    handle.await.unwrap();
}

// ── Multi-event sequence (simulates real turn) ─────────────────

#[tokio::test]
async fn test_full_turn_event_sequence() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let _handle = spawn_forwarder(turn_rx, core_tx).await;

    // Simulate a typical turn: Start → TextDelta → TextDelta → End.
    turn_tx.send(TurnEvent::TurnStart).await.unwrap();
    turn_tx
        .send(TurnEvent::TextDelta("Hello ".to_string()))
        .await
        .unwrap();
    turn_tx
        .send(TurnEvent::TextDelta("world!".to_string()))
        .await
        .unwrap();
    turn_tx.send(TurnEvent::TurnEnd).await.unwrap();
    drop(turn_tx);

    let mut events = Vec::new();
    while let Some(event) = core_rx.recv().await {
        events.push(event);
    }

    assert_eq!(events.len(), 4, "should receive 4 events");
    assert!(matches!(events[0], CoreEvent::StreamStart));
    assert!(matches!(events[1], CoreEvent::TextDelta(ref t) if t == "Hello "));
    assert!(matches!(events[2], CoreEvent::TextDelta(ref t) if t == "world!"));
    assert!(matches!(events[3], CoreEvent::StreamEnd));
}

// ── Turn with tool use sequence ────────────────────────────────

#[tokio::test]
async fn test_turn_with_tool_use_sequence() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, mut core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;

    // Simulate: Start → ToolUseStart → ToolResult → TextDelta → End.
    turn_tx.send(TurnEvent::TurnStart).await.unwrap();
    turn_tx
        .send(TurnEvent::ToolUseStart {
            id: "t1".to_string(),
            name: "file_read".to_string(),
            input: serde_json::json!({"file_path": "/tmp/test.rs"}),
        })
        .await
        .unwrap();
    turn_tx
        .send(TurnEvent::ToolResult {
            tool_use_id: "t1".to_string(),
            content: "fn main() {}".to_string(),
            is_error: false,
        })
        .await
        .unwrap();
    turn_tx
        .send(TurnEvent::TextDelta("The file contains...".to_string()))
        .await
        .unwrap();
    turn_tx.send(TurnEvent::TurnEnd).await.unwrap();
    drop(turn_tx);

    let mut events = Vec::new();
    while let Some(event) = core_rx.recv().await {
        events.push(event);
    }

    assert_eq!(events.len(), 5, "should receive 5 events");
    assert!(matches!(events[0], CoreEvent::StreamStart));
    assert!(matches!(events[1], CoreEvent::ToolUseStart { ref name, .. } if name == "file_read"));
    assert!(
        matches!(events[2], CoreEvent::ToolResult { ref tool_use_id, .. } if tool_use_id == "t1")
    );
    assert!(matches!(events[3], CoreEvent::TextDelta(_)));
    assert!(matches!(events[4], CoreEvent::StreamEnd));

    handle.await.unwrap();
}

// ── Forwarder closes cleanly when sender drops ─────────────────

#[tokio::test]
async fn test_forwarder_completes_when_sender_drops() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, _core_rx) = mpsc::channel::<CoreEvent>(32);

    let fwd = spawn_forwarder(turn_rx, core_tx).await;

    // Drop the sender immediately.
    drop(turn_tx);

    // Forwarder should complete without error.
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), fwd).await;
    assert!(
        result.is_ok(),
        "forwarder should complete within 1s after sender drops"
    );
    assert!(result.unwrap().is_ok());
}

// ── Forwarder handles receiver drop gracefully ─────────────────

#[tokio::test]
async fn test_forwarder_handles_dropped_receiver() {
    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(32);
    let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(32);

    let handle = spawn_forwarder(turn_rx, core_tx).await;

    // Drop the CoreEvent receiver — forwarder should still drain without panic.
    drop(core_rx);

    turn_tx
        .send(TurnEvent::TextDelta("orphaned".to_string()))
        .await
        .unwrap();
    drop(turn_tx);

    // Should complete without panic (send errors are ignored with `let _`).
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), handle).await;
    assert!(
        result.is_ok(),
        "forwarder should not panic on dropped receiver"
    );
}
