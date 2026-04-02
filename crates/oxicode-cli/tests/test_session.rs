//! Integration tests for session persistence (save/load/list).

use oxicode_common::Message;
use oxicode_session::Session;

fn temp_config_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_path_buf();
    (dir, path)
}

#[test]
fn test_session_create_and_save() {
    let (_tmp, config_dir) = temp_config_dir();

    let mut session = Session::new("claude-sonnet-4");
    session.push_message(Message::user("Hello"));

    let save_path = oxicode_session::save_session(&session, Some(&config_dir)).unwrap();
    assert!(save_path.exists());
}

#[test]
fn test_session_save_and_load_roundtrip() {
    let (_tmp, config_dir) = temp_config_dir();

    let mut session = Session::new("claude-opus-4");
    session.push_message(Message::user("What is Rust?"));

    let mut assistant = Message::assistant();
    assistant.content.push(oxicode_common::ContentBlock::Text {
        text: "Rust is a systems language.".to_string(),
    });
    session.push_message(assistant);

    oxicode_session::save_session(&session, Some(&config_dir)).unwrap();

    let loaded = oxicode_session::load_session(&session.id, Some(&config_dir)).unwrap();
    assert_eq!(loaded.id, session.id);
    assert_eq!(loaded.messages.len(), 2);
    assert_eq!(loaded.messages[0].text(), "What is Rust?");
    assert_eq!(loaded.messages[1].text(), "Rust is a systems language.");
}

#[test]
fn test_session_list() {
    let (_tmp, config_dir) = temp_config_dir();

    for i in 0..3 {
        let mut session = Session::new("model");
        session.push_message(Message::user(&format!("Message {i}")));
        oxicode_session::save_session(&session, Some(&config_dir)).unwrap();
    }

    let sessions = oxicode_session::list_sessions(Some(&config_dir)).unwrap();
    assert_eq!(sessions.len(), 3);
}

#[test]
fn test_load_nonexistent_session_fails() {
    let (_tmp, config_dir) = temp_config_dir();
    let result = oxicode_session::load_session("nonexistent-id", Some(&config_dir));
    assert!(result.is_err());
}

#[test]
fn test_session_summary_display() {
    let now = chrono::Utc::now();
    let summary = oxicode_session::SessionSummary {
        id: "abc12345-6789".to_string(),
        model: "claude-sonnet-4".to_string(),
        message_count: 5,
        preview: Some("Hello world".to_string()),
        created_at: now,
        updated_at: now,
    };

    let display = summary.display();
    assert!(display.contains("abc12345"));
    assert!(display.contains("claude-sonnet-4"));
    assert!(display.contains("5 msgs"));
}

#[test]
fn test_session_path_traversal_rejected() {
    let (_tmp, config_dir) = temp_config_dir();

    // C2 FIX: path traversal in session IDs should be rejected
    let result = oxicode_session::load_session("../../etc/passwd", Some(&config_dir));
    assert!(result.is_err());

    let result = oxicode_session::load_session("../secret", Some(&config_dir));
    assert!(result.is_err());

    let result = oxicode_session::load_session("foo/bar", Some(&config_dir));
    assert!(result.is_err());
}
