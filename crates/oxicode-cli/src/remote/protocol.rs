//! Bridge protocol message types — JSON-over-WebSocket for IDE/cloud clients.
//!
//! Subprotocol identifier: `oxicode.bridge.v1`
//! Messages are tagged by a `type` field (snake_case).
//!
//! Outbound (server → client) and inbound (client → server) enums map
//! directly onto the `structured_output::NdjsonEvent` shapes so tooling can
//! share deserialization code between NDJSON and WebSocket modes.

use serde::{Deserialize, Serialize};

/// WebSocket subprotocol identifier negotiated during the handshake.
pub const BRIDGE_SUBPROTOCOL: &str = "oxicode.bridge.v1";

// ─────────────────────────────────────────────────────────────────────────────
// Inbound: client → server
// ─────────────────────────────────────────────────────────────────────────────

/// Messages the client sends to the server.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundMessage {
    /// User text to process (starts or continues a turn).
    UserMessage {
        /// Content of the user's message.
        content: String,
        /// Optional model override for this turn.
        #[serde(default)]
        model: Option<String>,
    },
    /// Cancel the running turn.
    Cancel,
    /// Execute a slash command (e.g. `/clear`, `/model`).
    SlashCommand {
        /// The slash command text including the leading `/`.
        command: String,
    },
    /// Response to a `permission_ask` outbound event.
    PermissionResponse {
        /// Must match `request_id` from the `permission_ask` event.
        request_id: String,
        /// Whether the user approved the tool invocation.
        approved: bool,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Outbound: server → client
// ─────────────────────────────────────────────────────────────────────────────

/// Messages the server sends to the client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundMessage {
    /// Emitted immediately after JWT is verified and session is admitted.
    SessionStart {
        /// Server-assigned session ID.
        session_id: String,
        /// Model the session will use.
        model: String,
        /// ISO-8601 timestamp.
        timestamp: String,
        /// Protocol version negotiated.
        protocol: String,
    },
    /// Streaming text delta from the assistant.
    TextDelta { text: String },
    /// Tool invocation by the assistant.
    ToolUse {
        tool_use_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// Tool execution result.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// Server needs user approval before executing a tool.
    PermissionAsk {
        /// Correlates with `PermissionResponse.request_id`.
        request_id: String,
        tool_name: String,
        input_summary: String,
        prompt: String,
    },
    /// Token usage for the most recent turn.
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    /// Non-fatal error (session continues).
    Error { message: String },
    /// Session terminated — last message for this connection.
    SessionEnd { reason: String },
}

/// Serialize an `OutboundMessage` to a JSON string.
///
/// Returns `Err(String)` on the (very rare) case where serialization fails.
pub fn encode(msg: &OutboundMessage) -> Result<String, String> {
    serde_json::to_string(msg).map_err(|e| format!("encode error: {e}"))
}

/// Deserialize an `InboundMessage` from a JSON string.
pub fn decode(json: &str) -> Result<InboundMessage, String> {
    serde_json::from_str(json).map_err(|e| format!("decode error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_session_start() {
        let msg = OutboundMessage::SessionStart {
            session_id: "s1".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            timestamp: "2026-04-25T00:00:00Z".to_string(),
            protocol: BRIDGE_SUBPROTOCOL.to_string(),
        };
        let json = encode(&msg).unwrap();
        assert!(json.contains(r#""type":"session_start""#));
        assert!(json.contains("s1"));
        assert!(json.contains(BRIDGE_SUBPROTOCOL));
    }

    #[test]
    fn test_encode_text_delta() {
        let msg = OutboundMessage::TextDelta {
            text: "hello".to_string(),
        };
        let json = encode(&msg).unwrap();
        assert!(json.contains(r#""type":"text_delta""#));
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_encode_tool_use() {
        let msg = OutboundMessage::ToolUse {
            tool_use_id: "tu_1".to_string(),
            tool_name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = encode(&msg).unwrap();
        assert!(json.contains(r#""type":"tool_use""#));
        assert!(json.contains("tu_1"));
    }

    #[test]
    fn test_encode_permission_ask() {
        let msg = OutboundMessage::PermissionAsk {
            request_id: "req-1".to_string(),
            tool_name: "bash".to_string(),
            input_summary: "rm -rf /tmp".to_string(),
            prompt: "Allow?".to_string(),
        };
        let json = encode(&msg).unwrap();
        assert!(json.contains(r#""type":"permission_ask""#));
        assert!(json.contains("req-1"));
    }

    #[test]
    fn test_encode_session_end() {
        let msg = OutboundMessage::SessionEnd {
            reason: "client_disconnect".to_string(),
        };
        let json = encode(&msg).unwrap();
        assert!(json.contains(r#""type":"session_end""#));
    }

    #[test]
    fn test_decode_user_message() {
        let json = r#"{"type":"user_message","content":"hello"}"#;
        let msg = decode(json).unwrap();
        match msg {
            InboundMessage::UserMessage { content, model } => {
                assert_eq!(content, "hello");
                assert!(model.is_none());
            }
            _ => panic!("expected UserMessage"),
        }
    }

    #[test]
    fn test_decode_cancel() {
        let json = r#"{"type":"cancel"}"#;
        let msg = decode(json).unwrap();
        assert!(matches!(msg, InboundMessage::Cancel));
    }

    #[test]
    fn test_decode_slash_command() {
        let json = r#"{"type":"slash_command","command":"/clear"}"#;
        let msg = decode(json).unwrap();
        match msg {
            InboundMessage::SlashCommand { command } => assert_eq!(command, "/clear"),
            _ => panic!("expected SlashCommand"),
        }
    }

    #[test]
    fn test_decode_permission_response() {
        let json = r#"{"type":"permission_response","request_id":"req-1","approved":true}"#;
        let msg = decode(json).unwrap();
        match msg {
            InboundMessage::PermissionResponse {
                request_id,
                approved,
            } => {
                assert_eq!(request_id, "req-1");
                assert!(approved);
            }
            _ => panic!("expected PermissionResponse"),
        }
    }

    #[test]
    fn test_decode_invalid_json() {
        let result = decode("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_unknown_type() {
        let result = decode(r#"{"type":"unknown_type","foo":"bar"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_error() {
        let msg = OutboundMessage::Error {
            message: "something went wrong".to_string(),
        };
        let json = encode(&msg).unwrap();
        assert!(json.contains(r#""type":"error""#));
    }

    #[test]
    fn test_encode_usage() {
        let msg = OutboundMessage::Usage {
            input_tokens: 100,
            output_tokens: 200,
        };
        let json = encode(&msg).unwrap();
        assert!(json.contains(r#""type":"usage""#));
        assert!(json.contains("100"));
        assert!(json.contains("200"));
    }

    #[test]
    fn test_subprotocol_constant() {
        assert_eq!(BRIDGE_SUBPROTOCOL, "oxicode.bridge.v1");
    }
}
