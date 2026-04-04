//! SDK message adapter — convert between internal `TurnEvent` and wire format.
//!
//! Wire format uses NDJSON messages over WebSocket, matching the OpenClaude
//! `sdkMessageAdapter.ts` protocol for IDE extension compatibility.

use serde::{Deserialize, Serialize};

/// Wire message types sent from server to client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Session started acknowledgment.
    #[serde(rename = "session.start")]
    SessionStart { session_id: String },

    /// Text content delta (streaming).
    #[serde(rename = "content.delta")]
    ContentDelta { text: String },

    /// Tool use started.
    #[serde(rename = "tool.start")]
    ToolStart {
        tool_use_id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool execution result.
    #[serde(rename = "tool.result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },

    /// Permission request (needs client approval).
    #[serde(rename = "permission.request")]
    PermissionRequest {
        request_id: String,
        tool_name: String,
        input_summary: String,
        prompt: String,
    },

    /// Turn completed.
    #[serde(rename = "turn.complete")]
    TurnComplete { stop_reason: String },

    /// Error from server.
    #[serde(rename = "error")]
    Error { message: String },

    /// Session ended.
    #[serde(rename = "session.end")]
    SessionEnd { reason: String },
}

/// Wire message types sent from client to server.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    /// Send a user message for processing.
    #[serde(rename = "message.send")]
    MessageSend {
        session_id: String,
        text: String,
    },

    /// Permission response (approve/deny a tool execution).
    #[serde(rename = "permission.response")]
    PermissionResponse {
        request_id: String,
        approved: bool,
    },

    /// Cancel the current turn.
    #[serde(rename = "message.cancel")]
    MessageCancel { session_id: String },

    /// Create a new session.
    #[serde(rename = "session.create")]
    SessionCreate {
        model: Option<String>,
        working_dir: Option<String>,
    },

    /// Destroy a session.
    #[serde(rename = "session.destroy")]
    SessionDestroy { session_id: String },
}

/// Serialize a server message to NDJSON (single line JSON + newline).
pub fn encode_server_message(msg: &ServerMessage) -> Result<String, String> {
    serde_json::to_string(msg).map_err(|e| format!("Failed to encode server message: {e}"))
}

/// Deserialize a client message from JSON.
pub fn decode_client_message(json: &str) -> Result<ClientMessage, String> {
    serde_json::from_str(json).map_err(|e| format!("Failed to decode client message: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_message_session_start() {
        let msg = ServerMessage::SessionStart {
            session_id: "abc-123".to_string(),
        };
        let json = encode_server_message(&msg).unwrap();
        assert!(json.contains("session.start"));
        assert!(json.contains("abc-123"));
    }

    #[test]
    fn test_server_message_content_delta() {
        let msg = ServerMessage::ContentDelta {
            text: "Hello world".to_string(),
        };
        let json = encode_server_message(&msg).unwrap();
        assert!(json.contains("content.delta"));
        assert!(json.contains("Hello world"));
    }

    #[test]
    fn test_server_message_tool_start() {
        let msg = ServerMessage::ToolStart {
            tool_use_id: "tu-1".to_string(),
            name: "Bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = encode_server_message(&msg).unwrap();
        assert!(json.contains("tool.start"));
        assert!(json.contains("Bash"));
    }

    #[test]
    fn test_server_message_permission_request() {
        let msg = ServerMessage::PermissionRequest {
            request_id: "req-1".to_string(),
            tool_name: "Write".to_string(),
            input_summary: "write /tmp/test.txt".to_string(),
            prompt: "Allow file write?".to_string(),
        };
        let json = encode_server_message(&msg).unwrap();
        assert!(json.contains("permission.request"));
        assert!(json.contains("req-1"));
    }

    #[test]
    fn test_client_message_send() {
        let json = r#"{"type":"message.send","session_id":"s1","text":"hello"}"#;
        let msg = decode_client_message(json).unwrap();
        match msg {
            ClientMessage::MessageSend { session_id, text } => {
                assert_eq!(session_id, "s1");
                assert_eq!(text, "hello");
            }
            _ => panic!("Expected MessageSend"),
        }
    }

    #[test]
    fn test_client_message_permission_response() {
        let json = r#"{"type":"permission.response","request_id":"req-1","approved":true}"#;
        let msg = decode_client_message(json).unwrap();
        match msg {
            ClientMessage::PermissionResponse {
                request_id,
                approved,
            } => {
                assert_eq!(request_id, "req-1");
                assert!(approved);
            }
            _ => panic!("Expected PermissionResponse"),
        }
    }

    #[test]
    fn test_client_message_session_create() {
        let json = r#"{"type":"session.create","model":"claude-sonnet-4-20250514","working_dir":"/tmp"}"#;
        let msg = decode_client_message(json).unwrap();
        match msg {
            ClientMessage::SessionCreate { model, working_dir } => {
                assert_eq!(model.as_deref(), Some("claude-sonnet-4-20250514"));
                assert_eq!(working_dir.as_deref(), Some("/tmp"));
            }
            _ => panic!("Expected SessionCreate"),
        }
    }

    #[test]
    fn test_client_message_session_destroy() {
        let json = r#"{"type":"session.destroy","session_id":"s1"}"#;
        let msg = decode_client_message(json).unwrap();
        match msg {
            ClientMessage::SessionDestroy { session_id } => {
                assert_eq!(session_id, "s1");
            }
            _ => panic!("Expected SessionDestroy"),
        }
    }

    #[test]
    fn test_roundtrip_server_messages() {
        let messages = vec![
            ServerMessage::SessionStart {
                session_id: "s1".to_string(),
            },
            ServerMessage::ContentDelta {
                text: "test".to_string(),
            },
            ServerMessage::TurnComplete {
                stop_reason: "end_turn".to_string(),
            },
            ServerMessage::Error {
                message: "oops".to_string(),
            },
            ServerMessage::SessionEnd {
                reason: "complete".to_string(),
            },
        ];
        for msg in &messages {
            let json = encode_server_message(msg).unwrap();
            assert!(!json.is_empty());
            // Verify it's valid JSON.
            let _: serde_json::Value = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_decode_invalid_json() {
        let result = decode_client_message("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_unknown_type() {
        let json = r#"{"type":"unknown.message","data":"test"}"#;
        let result = decode_client_message(json);
        assert!(result.is_err());
    }
}
