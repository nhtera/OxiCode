//! Bridge protocol message definitions for IDE integration.
//!
//! Defines JSON-RPC method names (IDE -> OxiCode) and notification types
//! (OxiCode -> IDE) used by the bridge protocol. All types are serde-friendly
//! for wire serialization.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// JSON-RPC method names (IDE -> OxiCode requests)
// ---------------------------------------------------------------------------

/// Bridge method namespace prefix.
pub const BRIDGE_PREFIX: &str = "bridge";

/// Handshake: exchange capabilities, protocol version.
pub const METHOD_INITIALIZE: &str = "bridge.initialize";
/// Send a user message from IDE.
pub const METHOD_SEND_MESSAGE: &str = "bridge.sendMessage";
/// Query current bridge/session status.
pub const METHOD_GET_STATUS: &str = "bridge.getStatus";
/// Retrieve full conversation history.
pub const METHOD_GET_CONVERSATION: &str = "bridge.getConversation";
/// Approve a pending permission request.
pub const METHOD_APPROVE_PERMISSION: &str = "bridge.approvePermission";
/// Cancel the current LLM turn.
pub const METHOD_CANCEL_TURN: &str = "bridge.cancelTurn";
/// Switch the active model.
pub const METHOD_SWITCH_MODEL: &str = "bridge.switchModel";

// ---------------------------------------------------------------------------
// Notification method names (OxiCode -> IDE)
// ---------------------------------------------------------------------------

/// Streaming text delta from LLM.
pub const NOTIFY_TEXT_DELTA: &str = "bridge.textDelta";
/// Tool execution started.
pub const NOTIFY_TOOL_USE: &str = "bridge.toolUse";
/// Tool execution completed with result.
pub const NOTIFY_TOOL_RESULT: &str = "bridge.toolResult";
/// Permission approval needed from IDE.
pub const NOTIFY_PERMISSION_REQUEST: &str = "bridge.permissionRequest";
/// LLM turn completed.
pub const NOTIFY_TURN_COMPLETE: &str = "bridge.turnComplete";
/// Error occurred during execution.
pub const NOTIFY_ERROR: &str = "bridge.error";

// ---------------------------------------------------------------------------
// Request parameters (IDE -> OxiCode)
// ---------------------------------------------------------------------------

/// `bridge.initialize` params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// Protocol version the IDE supports (e.g. "1.0").
    pub protocol_version: String,
    /// IDE name for identification (e.g. "vscode", "jetbrains").
    #[serde(default)]
    pub ide_name: Option<String>,
    /// IDE version string.
    #[serde(default)]
    pub ide_version: Option<String>,
    /// Capabilities the IDE supports.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// `bridge.initialize` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Protocol version OxiCode supports.
    pub protocol_version: String,
    /// Session ID for subsequent requests.
    pub session_id: String,
    /// Model currently active.
    pub model: String,
    /// Capabilities OxiCode supports.
    pub capabilities: Vec<String>,
}

/// `bridge.sendMessage` params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageParams {
    /// Target session ID.
    pub session_id: String,
    /// User message content.
    pub content: String,
}

/// `bridge.getStatus` params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStatusParams {
    pub session_id: String,
}

/// `bridge.getStatus` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStatusResult {
    pub session_id: String,
    /// "idle" | "streaming" | "tool_executing" | "awaiting_permission"
    pub state: String,
    pub model: String,
    pub message_count: usize,
}

/// `bridge.getConversation` params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetConversationParams {
    pub session_id: String,
    /// Optional: only return messages after this index.
    #[serde(default)]
    pub after_index: Option<usize>,
}

/// `bridge.approvePermission` params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovePermissionParams {
    pub session_id: String,
    /// Permission request ID from `bridge.permissionRequest`.
    pub permission_id: String,
    /// Whether to approve.
    pub approve: bool,
    /// Whether to always allow/deny this tool.
    #[serde(default)]
    pub always: bool,
}

/// `bridge.cancelTurn` params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelTurnParams {
    pub session_id: String,
}

/// `bridge.switchModel` params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchModelParams {
    pub session_id: String,
    pub model: String,
}

// ---------------------------------------------------------------------------
// Notification payloads (OxiCode -> IDE)
// ---------------------------------------------------------------------------

/// `bridge.textDelta` notification payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDeltaNotification {
    pub session_id: String,
    pub text: String,
}

/// `bridge.toolUse` notification payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseNotification {
    pub session_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

/// `bridge.toolResult` notification payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultNotification {
    pub session_id: String,
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// `bridge.permissionRequest` notification payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestNotification {
    pub session_id: String,
    pub permission_id: String,
    pub tool_name: String,
    pub input_summary: String,
    pub prompt: String,
}

/// `bridge.turnComplete` notification payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCompleteNotification {
    pub session_id: String,
    pub stop_reason: String,
    /// Final assistant text (if any).
    #[serde(default)]
    pub text: Option<String>,
}

/// `bridge.error` notification payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorNotification {
    pub session_id: String,
    pub message: String,
    #[serde(default)]
    pub code: Option<i32>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_params_roundtrip() {
        let params = InitializeParams {
            protocol_version: "1.0".to_string(),
            ide_name: Some("vscode".to_string()),
            ide_version: Some("1.85.0".to_string()),
            capabilities: vec!["streaming".to_string(), "permissions".to_string()],
        };
        let json = serde_json::to_string(&params).unwrap();
        let parsed: InitializeParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.protocol_version, "1.0");
        assert_eq!(parsed.ide_name.as_deref(), Some("vscode"));
        assert_eq!(parsed.capabilities.len(), 2);
    }

    #[test]
    fn initialize_params_minimal() {
        let json = r#"{"protocol_version":"1.0"}"#;
        let params: InitializeParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.protocol_version, "1.0");
        assert!(params.ide_name.is_none());
        assert!(params.capabilities.is_empty());
    }

    #[test]
    fn initialize_result_roundtrip() {
        let result = InitializeResult {
            protocol_version: "1.0".to_string(),
            session_id: "s-123".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            capabilities: vec!["streaming".to_string()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: InitializeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.session_id, "s-123");
    }

    #[test]
    fn send_message_params_roundtrip() {
        let params = SendMessageParams {
            session_id: "s1".to_string(),
            content: "Hello!".to_string(),
        };
        let json = serde_json::to_string(&params).unwrap();
        let parsed: SendMessageParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "Hello!");
    }

    #[test]
    fn get_status_result_roundtrip() {
        let result = GetStatusResult {
            session_id: "s1".to_string(),
            state: "idle".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            message_count: 10,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: GetStatusResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.state, "idle");
        assert_eq!(parsed.message_count, 10);
    }

    #[test]
    fn approve_permission_params_defaults() {
        let json = r#"{"session_id":"s1","permission_id":"p1","approve":true}"#;
        let params: ApprovePermissionParams = serde_json::from_str(json).unwrap();
        assert!(params.approve);
        assert!(!params.always); // default false
    }

    #[test]
    fn text_delta_notification_roundtrip() {
        let notif = TextDeltaNotification {
            session_id: "s1".to_string(),
            text: "Hello ".to_string(),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: TextDeltaNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.text, "Hello ");
    }

    #[test]
    fn tool_use_notification_roundtrip() {
        let notif = ToolUseNotification {
            session_id: "s1".to_string(),
            tool_use_id: "t1".to_string(),
            tool_name: "Read".to_string(),
            input: serde_json::json!({"file_path": "/test.rs"}),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: ToolUseNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tool_name, "Read");
    }

    #[test]
    fn permission_request_roundtrip() {
        let notif = PermissionRequestNotification {
            session_id: "s1".to_string(),
            permission_id: "p1".to_string(),
            tool_name: "Bash".to_string(),
            input_summary: "rm -rf /tmp/test".to_string(),
            prompt: "Allow bash command?".to_string(),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: PermissionRequestNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tool_name, "Bash");
    }

    #[test]
    fn turn_complete_notification_roundtrip() {
        let notif = TurnCompleteNotification {
            session_id: "s1".to_string(),
            stop_reason: "end_turn".to_string(),
            text: Some("Done!".to_string()),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: TurnCompleteNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.stop_reason, "end_turn");
        assert_eq!(parsed.text.as_deref(), Some("Done!"));
    }

    #[test]
    fn error_notification_defaults() {
        let json = r#"{"session_id":"s1","message":"something broke"}"#;
        let notif: ErrorNotification = serde_json::from_str(json).unwrap();
        assert_eq!(notif.message, "something broke");
        assert!(notif.code.is_none());
    }

    #[test]
    fn get_conversation_params_optional_index() {
        let json = r#"{"session_id":"s1"}"#;
        let params: GetConversationParams = serde_json::from_str(json).unwrap();
        assert!(params.after_index.is_none());

        let json2 = r#"{"session_id":"s1","after_index":5}"#;
        let params2: GetConversationParams = serde_json::from_str(json2).unwrap();
        assert_eq!(params2.after_index, Some(5));
    }

    #[test]
    fn method_constants_have_bridge_prefix() {
        assert!(METHOD_INITIALIZE.starts_with(BRIDGE_PREFIX));
        assert!(METHOD_SEND_MESSAGE.starts_with(BRIDGE_PREFIX));
        assert!(METHOD_GET_STATUS.starts_with(BRIDGE_PREFIX));
        assert!(METHOD_CANCEL_TURN.starts_with(BRIDGE_PREFIX));
        assert!(METHOD_SWITCH_MODEL.starts_with(BRIDGE_PREFIX));
    }

    #[test]
    fn notification_constants_have_bridge_prefix() {
        assert!(NOTIFY_TEXT_DELTA.starts_with(BRIDGE_PREFIX));
        assert!(NOTIFY_TOOL_USE.starts_with(BRIDGE_PREFIX));
        assert!(NOTIFY_TOOL_RESULT.starts_with(BRIDGE_PREFIX));
        assert!(NOTIFY_PERMISSION_REQUEST.starts_with(BRIDGE_PREFIX));
        assert!(NOTIFY_TURN_COMPLETE.starts_with(BRIDGE_PREFIX));
        assert!(NOTIFY_ERROR.starts_with(BRIDGE_PREFIX));
    }
}
