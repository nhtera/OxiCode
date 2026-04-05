//! Integration tests for session persistence and state management.
//!
//! These tests do NOT require API credentials.
//! Run with: `cargo test -p oxicode-session --test live_session_persistence`

use oxicode_common::{ContentBlock, Message, Usage};
use oxicode_session::{Session, list_sessions, load_session, save_session};

#[test]
fn test_session_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = Session::new("claude-sonnet-4-20250514");
    session.push_message(Message::user("What is Rust?"));

    let mut assistant = Message::assistant();
    assistant.content.push(ContentBlock::Text {
        text: "Rust is a systems language.".to_string(),
    });
    session.push_message(assistant);

    session.push_message(Message::user("Tell me more"));

    let path = save_session(&session, Some(tmp.path())).unwrap();
    assert!(path.exists(), "Session file should exist on disk");

    let loaded = load_session(&session.id, Some(tmp.path())).unwrap();
    assert_eq!(loaded.id, session.id);
    assert_eq!(loaded.model, "claude-sonnet-4-20250514");
    assert_eq!(loaded.messages.len(), 3);
    assert_eq!(loaded.messages[0].text(), "What is Rust?");
    assert_eq!(loaded.messages[1].text(), "Rust is a systems language.");
    assert_eq!(loaded.messages[2].text(), "Tell me more");
}

#[test]
fn test_session_with_tool_use_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = Session::new("claude-sonnet-4-20250514");

    session.push_message(Message::user("Read main.rs"));

    let mut assistant = Message::assistant();
    assistant.content.push(ContentBlock::ToolUse {
        id: "tu_1".to_string(),
        name: "file_read".to_string(),
        input: serde_json::json!({"file_path": "/src/main.rs"}),
    });
    session.push_message(assistant);

    let mut tool_msg = Message::user("");
    tool_msg.content = vec![ContentBlock::ToolResult {
        tool_use_id: "tu_1".to_string(),
        content: "fn main() {}".to_string(),
        is_error: false,
    }];
    session.push_message(tool_msg);

    save_session(&session, Some(tmp.path())).unwrap();
    let loaded = load_session(&session.id, Some(tmp.path())).unwrap();

    assert_eq!(loaded.messages.len(), 3);
    // Verify tool use block survived serialization.
    match &loaded.messages[1].content[0] {
        ContentBlock::ToolUse { name, .. } => assert_eq!(name, "file_read"),
        other => panic!("Expected ToolUse, got: {other:?}"),
    }
    match &loaded.messages[2].content[0] {
        ContentBlock::ToolResult {
            content, is_error, ..
        } => {
            assert_eq!(content, "fn main() {}");
            assert!(!is_error);
        }
        other => panic!("Expected ToolResult, got: {other:?}"),
    }
}

#[test]
fn test_session_list_multiple() {
    let tmp = tempfile::tempdir().unwrap();

    let mut s1 = Session::new("model-a");
    s1.push_message(Message::user("first session"));
    save_session(&s1, Some(tmp.path())).unwrap();

    let mut s2 = Session::new("model-b");
    s2.push_message(Message::user("second session"));
    save_session(&s2, Some(tmp.path())).unwrap();

    let mut s3 = Session::new("model-c");
    s3.push_message(Message::user("third session"));
    save_session(&s3, Some(tmp.path())).unwrap();

    let list = list_sessions(Some(tmp.path())).unwrap();
    assert_eq!(list.len(), 3);

    // Check that previews are populated.
    for summary in &list {
        assert!(summary.preview.is_some(), "Session preview should be set");
        assert!(summary.message_count > 0);
    }
}

#[test]
fn test_session_list_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let list = list_sessions(Some(tmp.path())).unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_load_nonexistent_session_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let result = load_session("nonexistent-id", Some(tmp.path()));
    assert!(result.is_err());
}

#[test]
fn test_session_id_path_traversal_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let result = load_session("../../../etc/passwd", Some(tmp.path()));
    assert!(result.is_err());

    let result = load_session("foo/bar", Some(tmp.path()));
    assert!(result.is_err());
}

#[test]
fn test_session_updated_at_advances() {
    let mut session = Session::new("test-model");
    let initial_time = session.updated_at;

    // Small sleep to ensure timestamp difference.
    std::thread::sleep(std::time::Duration::from_millis(10));
    session.push_message(Message::user("hello"));

    assert!(
        session.updated_at >= initial_time,
        "updated_at should advance after push_message"
    );
}

#[test]
fn test_session_summary_display() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = Session::new("claude-sonnet-4-20250514");
    session.push_message(Message::user("What is the meaning of life?"));
    save_session(&session, Some(tmp.path())).unwrap();

    let list = list_sessions(Some(tmp.path())).unwrap();
    assert_eq!(list.len(), 1);

    let display = list[0].display();
    assert!(display.contains("claude-sonnet-4-20250514"));
    assert!(display.contains("1 msgs"));
}

// ── State Store tests ──────────────────────────────────────────────

#[test]
fn test_state_store_usage_tracking() {
    use oxicode_state::{AppState, StateStore};

    let store = StateStore::new(AppState::default());

    let usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };

    store.add_usage(&usage, "claude-sonnet-4-20250514");

    let state = store.current();
    assert_eq!(state.total_usage.input_tokens, 100);
    assert_eq!(state.total_usage.output_tokens, 50);
}

#[test]
fn test_state_store_message_history() {
    use oxicode_state::{AppState, StateStore};

    let store = StateStore::new(AppState::default());

    store.push_message(Message::user("first"));
    store.push_message(Message::user("second"));

    let state = store.current();
    assert_eq!(state.messages.len(), 2);
    assert_eq!(state.messages[0].text(), "first");
    assert_eq!(state.messages[1].text(), "second");
}

#[test]
fn test_state_store_streaming_flag() {
    use oxicode_state::{AppState, StateStore};

    let store = StateStore::new(AppState::default());

    assert!(!store.current().is_streaming);
    store.set_streaming(true);
    assert!(store.current().is_streaming);
    store.set_streaming(false);
    assert!(!store.current().is_streaming);
}
