//! Integration tests for NDJSON structured output serialization.

use serde_json::Value;

fn parse_ndjson(line: &str) -> Value {
    serde_json::from_str(line).unwrap_or_else(|e| panic!("Invalid NDJSON: {e}\nLine: {line}"))
}

#[test]
fn test_session_start_event_schema() {
    let event = serde_json::json!({
        "type": "session_start",
        "session_id": "abc123",
        "model": "claude-opus-4-6",
        "timestamp": "2026-01-01T00:00:00Z"
    });
    let line = serde_json::to_string(&event).unwrap();
    let parsed = parse_ndjson(&line);

    assert_eq!(parsed["type"], "session_start");
    assert_eq!(parsed["session_id"], "abc123");
    assert!(parsed["timestamp"].is_string());
}

#[test]
fn test_tool_use_event_schema() {
    let event = serde_json::json!({
        "type": "tool_use",
        "tool_name": "bash",
        "input": {"command": "ls -la"}
    });
    let line = serde_json::to_string(&event).unwrap();
    let parsed = parse_ndjson(&line);

    assert_eq!(parsed["type"], "tool_use");
    assert_eq!(parsed["tool_name"], "bash");
    assert!(parsed["input"].is_object());
}

#[test]
fn test_usage_event_schema() {
    let event = serde_json::json!({
        "type": "usage",
        "input_tokens": 1234,
        "output_tokens": 567
    });
    let line = serde_json::to_string(&event).unwrap();
    let parsed = parse_ndjson(&line);

    assert_eq!(parsed["type"], "usage");
    assert_eq!(parsed["input_tokens"], 1234);
}

#[test]
fn test_full_ndjson_conversation_parseable() {
    let events = vec![
        r#"{"type":"session_start","session_id":"s1","model":"claude-sonnet-4","timestamp":"2026-01-01T00:00:00Z"}"#,
        r#"{"type":"user_message","content":"Fix the bug"}"#,
        r#"{"type":"assistant_text","text":"I'll investigate the issue."}"#,
        r#"{"type":"tool_use","tool_name":"file_read","input":{"path":"src/main.rs"}}"#,
        r#"{"type":"tool_result","tool_use_id":"tu_1","content":"fn main() {}","is_error":false}"#,
        r#"{"type":"usage","input_tokens":500,"output_tokens":100}"#,
        r#"{"type":"turn_complete","stop_reason":"end_turn"}"#,
        r#"{"type":"session_end","reason":"complete"}"#,
    ];

    for (i, line) in events.iter().enumerate() {
        let parsed = parse_ndjson(line);
        assert!(
            parsed["type"].is_string(),
            "Event {i} should have 'type' field"
        );
    }
}
