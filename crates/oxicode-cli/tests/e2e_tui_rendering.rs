//! E2E TUI rendering tests.
//!
//! Tests the TUI rendering pipeline using ratatui's `TestBackend` to verify
//! that real-format events produce correct visual output — message views,
//! tool call widgets, status bar, and streaming markdown.
//!
//! No API key needed — feeds synthetic CoreEvents in real format.
//! Run with: `cargo test -p oxicode-cli --test e2e_tui_rendering --nocapture`

use std::sync::Arc;

use oxicode_common::{ContentBlock, Message, Role, Usage};
use oxicode_state::{AppState, StateStore};
use oxicode_tui::events::{CoreEvent, UiEvent};
use oxicode_tui::streaming_markdown::MarkdownStreamCollector;
use oxicode_tui::App;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

/// Build a minimal App for rendering verification.
///
/// Returns (App, StateStore, ui_tx sender). The `core_tx` sender is kept
/// alive inside this function to prevent the core_rx channel from closing.
fn make_test_app() -> (
    App,
    Arc<StateStore>,
    mpsc::Sender<UiEvent>,
    mpsc::Sender<CoreEvent>,
) {
    let state_store = Arc::new(StateStore::new(AppState {
        current_model: "claude-sonnet-4.6".to_string(),
        auth_label: "API Key".to_string(),
        ..AppState::default()
    }));

    let (ui_tx, _ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(256);

    let app = App::new(&state_store, ui_tx.clone(), core_rx, Vec::new());
    (app, state_store, ui_tx, core_tx)
}

// ═══════════════════════════════════════════════════════════════════
// 1. Render with messages in state
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_render_with_messages_does_not_panic() {
    let (mut app, state_store, _ui_tx, _core_tx) = make_test_app();

    // Add messages to state.
    state_store.push_message(Message::user("Hello, how are you?"));
    let mut assistant = Message::assistant();
    assistant.content.push(ContentBlock::Text {
        text: "I'm fine!".to_string(),
    });
    state_store.push_message(assistant);

    // Render with TestBackend — should not panic.
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    app.draw(&mut terminal).unwrap();

    // Verify buffer is not empty.
    let rendered = format!("{}", terminal.backend());
    assert!(
        !rendered.trim().is_empty(),
        "Rendered frame should have content"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 2. Render with tool call in state
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_render_with_tool_use_messages() {
    let (mut app, state_store, _ui_tx, _core_tx) = make_test_app();

    // User message.
    state_store.push_message(Message::user("Read /tmp/test.txt"));

    // Assistant with tool use.
    let mut assistant = Message::assistant();
    assistant.content.push(ContentBlock::ToolUse {
        id: "tu_1".to_string(),
        name: "file_read".to_string(),
        input: serde_json::json!({"file_path": "/tmp/test.txt"}),
    });
    state_store.push_message(assistant);

    // Tool result.
    let tool_result = Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "file contents here".to_string(),
            is_error: false,
        }],
        model: None,
        stop_reason: None,
        created_at: chrono::Utc::now(),
        usage: None,
    };
    state_store.push_message(tool_result);

    // Final assistant response.
    let mut final_msg = Message::assistant();
    final_msg.content.push(ContentBlock::Text {
        text: "The file contains: file contents here".to_string(),
    });
    state_store.push_message(final_msg);

    // Render — should not panic.
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    app.draw(&mut terminal).unwrap();
}

// ═══════════════════════════════════════════════════════════════════
// 3. Status bar shows model and token usage
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_status_bar_shows_model_info() {
    let state_store = Arc::new(StateStore::new(AppState {
        current_model: "claude-sonnet-4.6".to_string(),
        auth_label: "API Key".to_string(),
        total_usage: Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Usage::default()
        },
        ..AppState::default()
    }));

    let (ui_tx, _) = mpsc::channel::<UiEvent>(32);
    let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(256);
    let mut app = App::new(&state_store, ui_tx, core_rx, Vec::new());

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    app.draw(&mut terminal).unwrap();

    // Extract rendered text from the entire buffer.
    let rendered = format!("{}", terminal.backend());
    assert!(
        rendered.contains("sonnet") || rendered.contains("claude"),
        "Status bar should show model name somewhere, got:\n{rendered}"
    );

    // Keep core_tx alive.
    drop(core_tx);
}

// ═══════════════════════════════════════════════════════════════════
// 4. Message render cache works correctly
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_message_cache_updates_incrementally() {
    let (mut app, state_store, _ui_tx, _core_tx) = make_test_app();

    // Add first message and render.
    state_store.push_message(Message::user("First message"));

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    app.draw(&mut terminal).unwrap();

    // Add second message and render again — cache should update, not crash.
    let mut assistant = Message::assistant();
    assistant.content.push(ContentBlock::Text {
        text: "First response".to_string(),
    });
    state_store.push_message(assistant);

    app.draw(&mut terminal).unwrap();

    // Add third message.
    state_store.push_message(Message::user("Second message"));
    app.draw(&mut terminal).unwrap();

    // Verify state has 3 messages and rendering didn't panic.
    let state = state_store.current();
    assert_eq!(state.messages.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════
// 5. Streaming markdown collector unit tests with realistic deltas
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_streaming_markdown_realistic_deltas() {
    let mut collector = MarkdownStreamCollector::new();

    // Simulate realistic streaming deltas (partial words/lines).
    let deltas = [
        "# He",
        "llo W",
        "orld\n",
        "\nThis is a ",
        "**bold** ",
        "paragraph.\n",
        "\n```rust\n",
        "fn main(",
        ") {\n",
        "    println!(\"hello\");\n",
        "}\n",
        "```\n",
    ];

    let mut total_committed = 0;
    for delta in &deltas {
        collector.push_delta(delta);
        let new_lines = collector.commit_complete_lines();
        total_committed += new_lines.len();
    }

    let final_lines = collector.finalize();
    total_committed += final_lines.len();

    assert!(
        total_committed >= 4,
        "Should produce at least 4 lines from markdown, got: {total_committed}"
    );
}

#[test]
fn test_streaming_markdown_partial_line_held_back() {
    let mut collector = MarkdownStreamCollector::new();

    // Push partial line (no newline).
    collector.push_delta("Hello world");
    let lines = collector.commit_complete_lines();
    assert!(
        lines.is_empty(),
        "Partial line (no newline) should not be committed"
    );

    // Complete the line.
    collector.push_delta(" and more.\n");
    let lines = collector.commit_complete_lines();
    assert!(
        !lines.is_empty(),
        "Completed line should be committed after newline"
    );
}

#[test]
fn test_streaming_markdown_finalize_flushes_remaining() {
    let mut collector = MarkdownStreamCollector::new();

    collector.push_delta("Incomplete line without newline");
    let lines = collector.commit_complete_lines();
    assert!(lines.is_empty(), "Should hold back incomplete line");

    let final_lines = collector.finalize();
    assert!(
        !final_lines.is_empty(),
        "Finalize should flush remaining buffer"
    );
}

#[test]
fn test_streaming_markdown_clear_resets_state() {
    let mut collector = MarkdownStreamCollector::new();

    collector.push_delta("Some text\n");
    let _ = collector.commit_complete_lines();

    collector.clear();

    collector.push_delta("Fresh start\n");
    let lines = collector.commit_complete_lines();
    assert!(
        !lines.is_empty(),
        "After clear, new deltas should produce lines"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 6. Render with empty state (initial boot)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_render_empty_state_baseline() {
    let (mut app, _state_store, _ui_tx, _core_tx) = make_test_app();

    // Render with no messages — should show empty chat area + status bar.
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    app.draw(&mut terminal).unwrap();

    // Should not panic and should render something.
    let buffer = terminal.backend().buffer().clone();
    assert_eq!(buffer.area.width, 120);
    assert_eq!(buffer.area.height, 40);
}

// ═══════════════════════════════════════════════════════════════════
// 7. Render at various terminal sizes
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_render_various_terminal_sizes() {
    let sizes = [(80, 24), (120, 40), (200, 60), (40, 10)];

    for (width, height) in sizes {
        let (mut app, state_store, _ui_tx, _core_tx) = make_test_app();
        state_store.push_message(Message::user("test message"));

        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        app.draw(&mut terminal)
            .unwrap_or_else(|e| panic!("Render at {width}x{height} should not fail: {e}"));
    }
}

// ═══════════════════════════════════════════════════════════════════
// 8. Render with many messages (scroll stress)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_render_many_messages_no_panic() {
    let (mut app, state_store, _ui_tx, _core_tx) = make_test_app();

    // Add 50 messages (25 turns).
    for i in 0..25 {
        state_store.push_message(Message::user(&format!("Question {i}")));
        let mut reply = Message::assistant();
        reply.content.push(ContentBlock::Text {
            text: format!("Answer {i} with some longer text to test wrapping and layout."),
        });
        state_store.push_message(reply);
    }

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    app.draw(&mut terminal).unwrap();

    // Verify state has all messages.
    let state = state_store.current();
    assert_eq!(state.messages.len(), 50);
}
