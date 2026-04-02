//! Hook event types and payloads.
//!
//! Each lifecycle event carries a typed JSON payload sent to hook scripts via stdin.

use serde::{Deserialize, Serialize};

/// Hook lifecycle events (26 total: 10 core + 16 extended).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    // ── Core events (Phase 3) ──
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

    // ── Extended events (Phase 5) ──
    /// Fires after context compaction completes.
    CompactComplete,
    /// Fires when a subagent is spawned.
    AgentSpawn,
    /// Fires when a subagent completes.
    AgentComplete,
    /// Fires when a skill is activated.
    SkillActivate,
    /// Fires when a plugin initializes.
    PluginInit,
    /// Fires when a plugin shuts down.
    PluginShutdown,
    /// Fires when a session is saved.
    SessionSave,
    /// Fires when a session is loaded.
    SessionLoad,
    /// Fires when the TUI theme changes.
    ThemeChange,
    /// Fires when a slash command is executed.
    CommandExecute,
    /// Fires when a file is read by the agent.
    FileRead,
    /// Fires when a file is written by the agent.
    FileWrite,
    /// Fires when a file is edited by the agent.
    FileEdit,
    /// Fires when a bash command is executed.
    BashExecute,
    /// Fires when a permission is granted.
    PermissionGrant,
    /// Fires when a permission is denied.
    PermissionDeny,
}

impl HookEvent {
    /// All supported events.
    pub const ALL: &[Self] = &[
        // Core
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
        // Extended
        Self::CompactComplete,
        Self::AgentSpawn,
        Self::AgentComplete,
        Self::SkillActivate,
        Self::PluginInit,
        Self::PluginShutdown,
        Self::SessionSave,
        Self::SessionLoad,
        Self::ThemeChange,
        Self::CommandExecute,
        Self::FileRead,
        Self::FileWrite,
        Self::FileEdit,
        Self::BashExecute,
        Self::PermissionGrant,
        Self::PermissionDeny,
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
            Self::CompactComplete => "compact_complete",
            Self::AgentSpawn => "agent_spawn",
            Self::AgentComplete => "agent_complete",
            Self::SkillActivate => "skill_activate",
            Self::PluginInit => "plugin_init",
            Self::PluginShutdown => "plugin_shutdown",
            Self::SessionSave => "session_save",
            Self::SessionLoad => "session_load",
            Self::ThemeChange => "theme_change",
            Self::CommandExecute => "command_execute",
            Self::FileRead => "file_read",
            Self::FileWrite => "file_write",
            Self::FileEdit => "file_edit",
            Self::BashExecute => "bash_execute",
            Self::PermissionGrant => "permission_grant",
            Self::PermissionDeny => "permission_deny",
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
    fn test_all_events_count() {
        assert_eq!(HookEvent::ALL.len(), 26);
    }

    #[test]
    fn test_event_serialization() {
        let json = serde_json::to_string(&HookEvent::PreQuery).unwrap();
        assert_eq!(json, "\"pre_query\"");
        let parsed: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HookEvent::PreQuery);
    }

    #[test]
    fn test_extended_event_serialization() {
        let json = serde_json::to_string(&HookEvent::AgentSpawn).unwrap();
        assert_eq!(json, "\"agent_spawn\"");
        let parsed: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HookEvent::AgentSpawn);
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

    #[test]
    fn test_as_str_roundtrip() {
        for event in HookEvent::ALL {
            let s = event.as_str();
            assert!(!s.is_empty());
            // Verify the as_str output matches serde serialization (without quotes).
            let serialized = serde_json::to_string(event).unwrap();
            assert_eq!(format!("\"{s}\""), serialized);
        }
    }
}
