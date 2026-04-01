//! Hook event types and payloads.
//!
//! Each lifecycle event carries a typed JSON payload sent to hook scripts via stdin.

use serde::{Deserialize, Serialize};

/// Hook lifecycle events (10 core events for Phase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Fires after session initialization.
    SessionStart,
    /// Fires before session exit.
    SessionEnd,
    /// Fires before sending a query to the LLM.
    PreQuery,
    /// Fires after receiving a complete LLM response.
    PostSampling,
    /// Fires before executing a tool.
    ToolCallBefore,
    /// Fires after a tool execution completes.
    ToolCallAfter,
    /// Fires before showing a permission dialog.
    PermissionRequest,
    /// Fires before context compaction.
    ContextCompact,
    /// Fires when the active model changes.
    ModelSwitch,
    /// Fires on errors.
    Error,
}

impl HookEvent {
    /// All supported events.
    pub const ALL: &[Self] = &[
        Self::SessionStart,
        Self::SessionEnd,
        Self::PreQuery,
        Self::PostSampling,
        Self::ToolCallBefore,
        Self::ToolCallAfter,
        Self::PermissionRequest,
        Self::ContextCompact,
        Self::ModelSwitch,
        Self::Error,
    ];

    /// Event name as used in config keys.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::PreQuery => "pre_query",
            Self::PostSampling => "post_sampling",
            Self::ToolCallBefore => "tool_call_before",
            Self::ToolCallAfter => "tool_call_after",
            Self::PermissionRequest => "permission_request",
            Self::ContextCompact => "context_compact",
            Self::ModelSwitch => "model_switch",
            Self::Error => "error",
        }
    }
}

/// Payload sent to hook scripts via stdin (JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    pub event: HookEvent,
    /// Event-specific data (varies by event type).
    #[serde(default)]
    pub data: serde_json::Value,
    /// Current session ID.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Current model name.
    #[serde(default)]
    pub model: Option<String>,
}

/// Response from a hook script (read from stdout as JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[derive(Default)]
pub enum HookResponse {
    /// Continue normally.
    #[default]
    Pass,
    /// Inject text into the system prompt.
    ModifyPrompt { text: String },
    /// Replace the tool result with custom content.
    OverrideResult { text: String },
    /// Cancel the operation.
    Abort { reason: String },
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let json = serde_json::to_string(&HookEvent::PreQuery).unwrap();
        assert_eq!(json, "\"pre_query\"");
        let parsed: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HookEvent::PreQuery);
    }

    #[test]
    fn test_hook_response_pass() {
        let json = r#"{"action":"pass"}"#;
        let resp: HookResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(resp, HookResponse::Pass));
    }

    #[test]
    fn test_hook_response_modify() {
        let json = r#"{"action":"modify_prompt","text":"extra context"}"#;
        let resp: HookResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(resp, HookResponse::ModifyPrompt { text } if text == "extra context"));
    }

    #[test]
    fn test_hook_response_abort() {
        let json = r#"{"action":"abort","reason":"blocked by policy"}"#;
        let resp: HookResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(resp, HookResponse::Abort { reason } if reason == "blocked by policy"));
    }

    #[test]
    fn test_payload_serialization() {
        let payload = HookPayload {
            event: HookEvent::ToolCallBefore,
            data: serde_json::json!({"tool": "bash", "command": "ls"}),
            session_id: Some("sess_123".to_string()),
            model: Some("claude-sonnet-4".to_string()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("tool_call_before"));
        assert!(json.contains("bash"));
    }
}
