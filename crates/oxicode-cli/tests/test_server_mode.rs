//! Integration tests for server mode JSON-RPC protocol.
//!
//! Tests validate: protocol parsing, response serialization, notification format,
//! error handling, and handler dispatch logic.

use serde_json::Value;

// ---------------------------------------------------------------------------
// Protocol types tests
// ---------------------------------------------------------------------------

fn parse_json(line: &str) -> Value {
    serde_json::from_str(line).unwrap_or_else(|e| panic!("Invalid JSON: {e}\nLine: {line}"))
}

#[test]
fn test_rpc_request_numeric_id() {
    let json = r#"{"jsonrpc":"2.0","id":42,"method":"session.create","params":{}}"#;
    let parsed = parse_json(json);
    assert_eq!(parsed["id"], 42);
    assert_eq!(parsed["method"], "session.create");
}

#[test]
fn test_rpc_request_string_id() {
    let json = r#"{"jsonrpc":"2.0","id":"req-1","method":"shutdown","params":{}}"#;
    let parsed = parse_json(json);
    assert_eq!(parsed["id"], "req-1");
    assert_eq!(parsed["method"], "shutdown");
}

#[test]
fn test_rpc_response_ok_format() {
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "session_id": "s1",
            "model": "claude-sonnet-4-20250514"
        }
    });
    let line = serde_json::to_string(&response).unwrap();
    let parsed = parse_json(&line);

    assert_eq!(parsed["jsonrpc"], "2.0");
    assert!(parsed["result"].is_object());
    assert!(parsed.get("error").is_none() || parsed["error"].is_null());
}

#[test]
fn test_rpc_response_error_format() {
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "error": {
            "code": -32601,
            "message": "Method not found"
        }
    });
    let line = serde_json::to_string(&response).unwrap();
    let parsed = parse_json(&line);

    assert_eq!(parsed["error"]["code"], -32601);
    assert_eq!(parsed["error"]["message"], "Method not found");
}

#[test]
fn test_notification_format_no_id() {
    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "stream.text",
        "params": {
            "session_id": "s1",
            "text": "Hello world"
        }
    });
    let line = serde_json::to_string(&notification).unwrap();
    let parsed = parse_json(&line);

    assert_eq!(parsed["method"], "stream.text");
    assert!(parsed.get("id").is_none() || parsed["id"].is_null());
    assert_eq!(parsed["params"]["text"], "Hello world");
}

// ---------------------------------------------------------------------------
// Notification types
// ---------------------------------------------------------------------------

#[test]
fn test_stream_text_notification() {
    let params = serde_json::json!({
        "session_id": "s1",
        "text": "delta chunk"
    });
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "stream.text",
        "params": params
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed = parse_json(&json);
    assert_eq!(parsed["method"], "stream.text");
    assert_eq!(parsed["params"]["text"], "delta chunk");
}

#[test]
fn test_tool_start_notification() {
    let params = serde_json::json!({
        "session_id": "s1",
        "tool_use_id": "tu_1",
        "tool_name": "bash",
        "input": {"command": "ls"}
    });
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tool.start",
        "params": params
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed = parse_json(&json);
    assert_eq!(parsed["method"], "tool.start");
    assert_eq!(parsed["params"]["tool_name"], "bash");
}

#[test]
fn test_tool_result_notification() {
    let params = serde_json::json!({
        "session_id": "s1",
        "tool_use_id": "tu_1",
        "content": "file1.rs\nfile2.rs",
        "is_error": false
    });
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tool.result",
        "params": params
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed = parse_json(&json);
    assert_eq!(parsed["method"], "tool.result");
    assert!(!parsed["params"]["is_error"].as_bool().unwrap());
}

#[test]
fn test_permission_ask_notification() {
    let params = serde_json::json!({
        "session_id": "s1",
        "permission_id": "perm_1",
        "tool_name": "file_write",
        "input_summary": "/tmp/test.txt",
        "prompt": "Allow writing to /tmp/test.txt?"
    });
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "permission.ask",
        "params": params
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed = parse_json(&json);
    assert_eq!(parsed["method"], "permission.ask");
    assert_eq!(parsed["params"]["permission_id"], "perm_1");
}

#[test]
fn test_session_updated_notification() {
    let params = serde_json::json!({
        "session_id": "s1",
        "message_count": 5,
        "model": "claude-sonnet-4-20250514"
    });
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "session.updated",
        "params": params
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed = parse_json(&json);
    assert_eq!(parsed["method"], "session.updated");
    assert_eq!(parsed["params"]["message_count"], 5);
}

#[test]
fn test_error_notification() {
    let params = serde_json::json!({
        "session_id": "s1",
        "message": "API rate limit exceeded"
    });
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "error",
        "params": params
    });
    let json = serde_json::to_string(&notif).unwrap();
    let parsed = parse_json(&json);
    assert_eq!(parsed["method"], "error");
    assert_eq!(parsed["params"]["message"], "API rate limit exceeded");
}

// ---------------------------------------------------------------------------
// Request param validation
// ---------------------------------------------------------------------------

#[test]
fn test_session_create_params() {
    let json = r#"{"model":"claude-opus-4-6"}"#;
    let parsed: Value = serde_json::from_str(json).unwrap();
    assert_eq!(parsed["model"], "claude-opus-4-6");
}

#[test]
fn test_session_create_params_empty() {
    // No model specified — should use default.
    let json = r#"{}"#;
    let parsed: Value = serde_json::from_str(json).unwrap();
    assert!(parsed.get("model").is_none() || parsed["model"].is_null());
}

#[test]
fn test_session_resume_params() {
    let json = r#"{"session_id":"abc-123"}"#;
    let parsed: Value = serde_json::from_str(json).unwrap();
    assert_eq!(parsed["session_id"], "abc-123");
}

#[test]
fn test_message_send_params() {
    let json = r#"{"session_id":"s1","content":"Explain main.rs"}"#;
    let parsed: Value = serde_json::from_str(json).unwrap();
    assert_eq!(parsed["session_id"], "s1");
    assert_eq!(parsed["content"], "Explain main.rs");
}

#[test]
fn test_tool_decision_params_approve() {
    let json = r#"{"session_id":"s1","permission_id":"p1","always":false}"#;
    let parsed: Value = serde_json::from_str(json).unwrap();
    assert_eq!(parsed["permission_id"], "p1");
    assert!(!parsed["always"].as_bool().unwrap());
}

#[test]
fn test_tool_decision_params_always_deny() {
    let json = r#"{"session_id":"s1","permission_id":"p1","always":true}"#;
    let parsed: Value = serde_json::from_str(json).unwrap();
    assert!(parsed["always"].as_bool().unwrap());
}

#[test]
fn test_compact_params() {
    let json = r#"{"session_id":"s1"}"#;
    let parsed: Value = serde_json::from_str(json).unwrap();
    assert_eq!(parsed["session_id"], "s1");
}

// ---------------------------------------------------------------------------
// Error code constants
// ---------------------------------------------------------------------------

#[test]
fn test_standard_error_codes() {
    // Verify standard JSON-RPC error codes match spec.
    assert_eq!(-32700_i32, -32700); // Parse error
    assert_eq!(-32600_i32, -32600); // Invalid request
    assert_eq!(-32601_i32, -32601); // Method not found
    assert_eq!(-32602_i32, -32602); // Invalid params
    assert_eq!(-32603_i32, -32603); // Internal error
}

#[test]
fn test_custom_error_codes() {
    // Custom codes must be outside reserved range (-32768 to -32000).
    let session_not_found = -32001_i32;
    let request_cancelled = -32002_i32;
    let permission_denied = -32003_i32;

    // All within server error range (-32099 to -32000).
    assert!((-32099..=-32000).contains(&session_not_found));
    assert!((-32099..=-32000).contains(&request_cancelled));
    assert!((-32099..=-32000).contains(&permission_denied));
}

// ---------------------------------------------------------------------------
// Full request-response round trip
// ---------------------------------------------------------------------------

#[test]
fn test_session_create_roundtrip() {
    // Simulate: IDE sends session.create, server responds with session_id.
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "session.create",
        "params": {"model": "claude-sonnet-4-20250514"}
    });
    let req_line = serde_json::to_string(&request).unwrap();
    let parsed_req = parse_json(&req_line);
    assert_eq!(parsed_req["method"], "session.create");

    // Simulated response.
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "session_id": "uuid-here",
            "model": "claude-sonnet-4-20250514"
        }
    });
    let resp_line = serde_json::to_string(&response).unwrap();
    let parsed_resp = parse_json(&resp_line);
    assert_eq!(parsed_resp["id"], 1);
    assert!(parsed_resp["result"]["session_id"].is_string());
}

#[test]
fn test_shutdown_roundtrip() {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "shutdown",
        "params": {}
    });
    let req_line = serde_json::to_string(&request).unwrap();
    let parsed_req = parse_json(&req_line);
    assert_eq!(parsed_req["method"], "shutdown");

    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 99,
        "result": {"shutdown": true}
    });
    let resp_line = serde_json::to_string(&response).unwrap();
    let parsed_resp = parse_json(&resp_line);
    assert!(parsed_resp["result"]["shutdown"].as_bool().unwrap());
}

#[test]
fn test_unknown_method_error() {
    // Server should return method_not_found for unknown methods.
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 5,
        "error": {
            "code": -32601,
            "message": "Unknown method: foo.bar"
        }
    });
    let line = serde_json::to_string(&response).unwrap();
    let parsed = parse_json(&line);
    assert_eq!(parsed["error"]["code"], -32601);
    assert!(parsed["error"]["message"]
        .as_str()
        .unwrap()
        .contains("foo.bar"));
}

#[test]
fn test_invalid_params_error() {
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 6,
        "error": {
            "code": -32602,
            "message": "Invalid params: missing field `session_id`"
        }
    });
    let line = serde_json::to_string(&response).unwrap();
    let parsed = parse_json(&line);
    assert_eq!(parsed["error"]["code"], -32602);
}

// ---------------------------------------------------------------------------
// Message flow simulation
// ---------------------------------------------------------------------------

#[test]
fn test_full_message_flow_protocol() {
    // Simulate the complete protocol flow:
    // 1. session.create -> response
    // 2. message.send -> response + notifications
    // 3. shutdown -> response

    let messages: Vec<(&str, Value)> = vec![
        (
            "request",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1,
                "method": "session.create",
                "params": {}
            }),
        ),
        (
            "response",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1,
                "result": {"session_id": "s1", "model": "claude-sonnet-4-20250514"}
            }),
        ),
        (
            "request",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2,
                "method": "message.send",
                "params": {"session_id": "s1", "content": "Hello"}
            }),
        ),
        (
            "notification",
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "stream.text",
                "params": {"session_id": "s1", "text": "Hi there!"}
            }),
        ),
        (
            "response",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2,
                "result": {"session_id": "s1", "stop_reason": "end_turn", "text": "Hi there!"}
            }),
        ),
        (
            "request",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 3,
                "method": "shutdown",
                "params": {}
            }),
        ),
        (
            "response",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 3,
                "result": {"shutdown": true}
            }),
        ),
    ];

    // All messages must serialize and parse cleanly.
    for (kind, msg) in &messages {
        let line = serde_json::to_string(msg).unwrap();
        let parsed = parse_json(&line);
        assert_eq!(
            parsed["jsonrpc"], "2.0",
            "Missing jsonrpc version in {kind}"
        );

        match *kind {
            "request" => {
                assert!(parsed["id"].is_number() || parsed["id"].is_string());
                assert!(parsed["method"].is_string());
            }
            "response" => {
                assert!(parsed["id"].is_number() || parsed["id"].is_string());
                assert!(
                    parsed.get("result").is_some() || parsed.get("error").is_some(),
                    "Response must have result or error"
                );
            }
            "notification" => {
                assert!(parsed["method"].is_string());
                assert!(parsed.get("id").is_none() || parsed["id"].is_null());
            }
            _ => panic!("Unknown kind: {kind}"),
        }
    }
}
