//! JSON-RPC 2.0 protocol types for server mode.
//!
//! IDE extensions connect via stdin/stdout using line-delimited JSON-RPC.
//! Requests flow IDE -> OxiCode, notifications flow OxiCode -> IDE.

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 protocol version.
pub const JSONRPC_VERSION: &str = "2.0";

// ---------------------------------------------------------------------------
// Wire types (raw JSON-RPC envelope)
// ---------------------------------------------------------------------------

/// Incoming JSON-RPC request from IDE.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: RpcId,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC ID — integer or string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RpcId {
    Num(u64),
    Str(String),
}

/// Outgoing JSON-RPC response to IDE.
#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: RpcId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn ok(id: RpcId, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: RpcId, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// JSON-RPC error object.
#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Outgoing JSON-RPC notification (no id, OxiCode -> IDE).
#[derive(Debug, Serialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
}

impl RpcNotification {
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
        }
    }
}

// ---------------------------------------------------------------------------
// Standard JSON-RPC error codes
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    /// Custom: session not found.
    pub const SESSION_NOT_FOUND: i32 = -32001;
    /// Custom: request cancelled.
    pub const REQUEST_CANCELLED: i32 = -32002;
    /// Custom: permission denied.
    pub const PERMISSION_DENIED: i32 = -32003;
}

// ---------------------------------------------------------------------------
// Typed request parameters
// ---------------------------------------------------------------------------

/// `session.create` params.
#[derive(Debug, Deserialize)]
pub struct SessionCreateParams {
    #[serde(default)]
    pub model: Option<String>,
}

/// `session.resume` params.
#[derive(Debug, Deserialize)]
pub struct SessionResumeParams {
    pub session_id: String,
}

/// `message.send` params.
#[derive(Debug, Deserialize)]
pub struct MessageSendParams {
    pub session_id: String,
    pub content: String,
}

/// `message.cancel` params.
#[derive(Debug, Deserialize)]
pub struct MessageCancelParams {
    pub session_id: String,
}

/// `tool.approve` / `tool.deny` params.
#[derive(Debug, Deserialize)]
pub struct ToolDecisionParams {
    pub session_id: String,
    /// Permission request ID (matches the one sent in `permission.ask`).
    pub permission_id: String,
    /// Only for approve: whether to always-allow this tool.
    #[serde(default)]
    pub always: bool,
}

/// `config.get` params (reserved for future use).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ConfigGetParams {
    pub key: String,
}

/// `config.set` params (reserved for future use).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ConfigSetParams {
    pub key: String,
    pub value: serde_json::Value,
}

/// `compact` params.
#[derive(Debug, Deserialize)]
pub struct CompactParams {
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// Typed notification payloads (OxiCode -> IDE)
// ---------------------------------------------------------------------------

/// `stream.text` notification params.
#[derive(Debug, Serialize)]
pub struct StreamTextParams {
    pub session_id: String,
    pub text: String,
}

/// `stream.thinking` notification params (reserved for extended thinking).
#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct StreamThinkingParams {
    pub session_id: String,
    pub thinking: String,
}

/// `tool.start` notification params.
#[derive(Debug, Serialize)]
pub struct ToolStartParams {
    pub session_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

/// `tool.result` notification params.
#[derive(Debug, Serialize)]
pub struct ToolResultParams {
    pub session_id: String,
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// `permission.ask` notification params.
#[derive(Debug, Serialize)]
pub struct PermissionAskParams {
    pub session_id: String,
    pub permission_id: String,
    pub tool_name: String,
    pub input_summary: String,
    pub prompt: String,
}

/// `session.updated` notification params.
#[derive(Debug, Serialize)]
pub struct SessionUpdatedParams {
    pub session_id: String,
    pub message_count: usize,
    pub model: String,
}

/// `error` notification params.
#[derive(Debug, Serialize)]
pub struct ErrorNotificationParams {
    pub session_id: Option<String>,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_request_deserialize() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"session.create","params":{}}"#;
        let req: RpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "session.create");
        assert_eq!(req.id, RpcId::Num(1));
    }

    #[test]
    fn test_rpc_request_string_id() {
        let json = r#"{"jsonrpc":"2.0","id":"abc","method":"shutdown","params":{}}"#;
        let req: RpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, RpcId::Str("abc".to_string()));
    }

    #[test]
    fn test_rpc_response_ok_serialize() {
        let resp = RpcResponse::ok(RpcId::Num(1), serde_json::json!({"session_id": "s1"}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_rpc_response_err_serialize() {
        let resp = RpcResponse::err(RpcId::Num(2), error_codes::METHOD_NOT_FOUND, "not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32601"));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn test_notification_serialize() {
        let n = RpcNotification::new(
            "stream.text",
            serde_json::to_value(StreamTextParams {
                session_id: "s1".into(),
                text: "hello".into(),
            })
            .unwrap(),
        );
        let json = serde_json::to_string(&n).unwrap();
        assert!(json.contains("\"method\":\"stream.text\""));
        assert!(json.contains("\"hello\""));
    }

    #[test]
    fn test_message_send_params() {
        let json = r#"{"session_id":"s1","content":"hello world"}"#;
        let params: MessageSendParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.session_id, "s1");
        assert_eq!(params.content, "hello world");
    }

    #[test]
    fn test_tool_decision_params() {
        let json = r#"{"session_id":"s1","permission_id":"p1","always":true}"#;
        let params: ToolDecisionParams = serde_json::from_str(json).unwrap();
        assert!(params.always);
    }
}
