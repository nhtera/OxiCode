//! Hook event types and payloads.
//!
//! Each lifecycle event carries a typed JSON payload sent to hook scripts via stdin.

use serde::{Deserialize, Serialize};

/// Hook lifecycle events (29 total: 10 core + 16 extended + 2 rate limit + 1 cost).
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

    // ── Rate limit events (Phase 2 gap-closure) ──
    /// Fires when approaching rate limit threshold.
    RateLimitWarning,
    /// Fires when rate limit is exceeded (429).
    RateLimitExceeded,

    // ── Cost events (Phase 3 gap-closure) ──
    /// Fires after cost tracker is updated with new usage data.
    CostUpdate,
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
        // Rate limit
        Self::RateLimitWarning,
        Self::RateLimitExceeded,
        // Cost
        Self::CostUpdate,
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
            Self::RateLimitWarning => "rate_limit_warning",
            Self::RateLimitExceeded => "rate_limit_exceeded",
            Self::CostUpdate => "cost_update",
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
    /// Surface informational fields from a hook (openclaude-style output).
    ///
    /// Carries `systemMessage`, `additionalContext`, and a non-fatal `reason`
    /// to be displayed to the user without aborting the operation.
    Message {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system_message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        additional_context: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_reason: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_events_count() {
        assert_eq!(HookEvent::ALL.len(), 29);
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

    // -- New comprehensive tests --

    #[test]
    fn test_all_core_events_serialize() {
        let core_events = [
            (HookEvent::SessionStart, "session_start"),
            (HookEvent::SessionEnd, "session_end"),
            (HookEvent::PreQuery, "pre_query"),
            (HookEvent::PostSampling, "post_sampling"),
            (HookEvent::ToolCallBefore, "tool_call_before"),
            (HookEvent::ToolCallAfter, "tool_call_after"),
            (HookEvent::PermissionRequest, "permission_request"),
            (HookEvent::ContextCompact, "context_compact"),
            (HookEvent::ModelSwitch, "model_switch"),
            (HookEvent::Error, "error"),
        ];
        for (event, expected_str) in core_events {
            assert_eq!(event.as_str(), expected_str, "Core event {event:?} as_str");
            let json = serde_json::to_string(&event).unwrap();
            let parsed: HookEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, event, "Core event {event:?} roundtrip");
        }
    }

    #[test]
    fn test_all_extended_events_serialize() {
        let extended_events = [
            (HookEvent::CompactComplete, "compact_complete"),
            (HookEvent::AgentSpawn, "agent_spawn"),
            (HookEvent::AgentComplete, "agent_complete"),
            (HookEvent::SkillActivate, "skill_activate"),
            (HookEvent::PluginInit, "plugin_init"),
            (HookEvent::PluginShutdown, "plugin_shutdown"),
            (HookEvent::SessionSave, "session_save"),
            (HookEvent::SessionLoad, "session_load"),
            (HookEvent::ThemeChange, "theme_change"),
            (HookEvent::CommandExecute, "command_execute"),
            (HookEvent::FileRead, "file_read"),
            (HookEvent::FileWrite, "file_write"),
            (HookEvent::FileEdit, "file_edit"),
            (HookEvent::BashExecute, "bash_execute"),
            (HookEvent::PermissionGrant, "permission_grant"),
            (HookEvent::PermissionDeny, "permission_deny"),
        ];
        for (event, expected_str) in extended_events {
            assert_eq!(
                event.as_str(),
                expected_str,
                "Extended event {event:?} as_str"
            );
            let json = serde_json::to_string(&event).unwrap();
            let parsed: HookEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, event, "Extended event {event:?} roundtrip");
        }
    }

    #[test]
    fn test_rate_limit_events_serialize() {
        let events = [
            (HookEvent::RateLimitWarning, "rate_limit_warning"),
            (HookEvent::RateLimitExceeded, "rate_limit_exceeded"),
        ];
        for (event, expected_str) in events {
            assert_eq!(event.as_str(), expected_str);
            let json = serde_json::to_string(&event).unwrap();
            let parsed: HookEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, event);
        }
    }

    #[test]
    fn test_cost_event_serialize() {
        assert_eq!(HookEvent::CostUpdate.as_str(), "cost_update");
        let json = serde_json::to_string(&HookEvent::CostUpdate).unwrap();
        let parsed: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HookEvent::CostUpdate);
    }

    #[test]
    fn test_all_events_unique_str() {
        let mut seen = std::collections::HashSet::new();
        for event in HookEvent::ALL {
            let s = event.as_str();
            assert!(seen.insert(s), "Duplicate event string: {s}");
        }
    }

    #[test]
    fn test_payload_with_null_data() {
        let payload = HookPayload {
            event: HookEvent::SessionStart,
            data: serde_json::Value::Null,
            session_id: None,
            model: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let parsed: HookPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event, HookEvent::SessionStart);
        assert!(parsed.data.is_null());
    }

    #[test]
    fn test_payload_default_data() {
        // Test that missing "data" field defaults to null/empty.
        let json = r#"{"event":"session_start"}"#;
        let parsed: HookPayload = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.event, HookEvent::SessionStart);
        assert!(parsed.session_id.is_none());
        assert!(parsed.model.is_none());
    }

    #[test]
    fn test_payload_full_fields() {
        let payload = HookPayload {
            event: HookEvent::ToolCallBefore,
            data: serde_json::json!({"tool": "bash", "args": {"command": "ls"}}),
            session_id: Some("sess_abc".to_string()),
            model: Some("claude-sonnet-4-20250514".to_string()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let parsed: HookPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event, HookEvent::ToolCallBefore);
        assert_eq!(parsed.session_id.as_deref(), Some("sess_abc"));
        assert_eq!(parsed.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert!(parsed.data.get("tool").is_some());
    }

    #[test]
    fn test_hook_response_override_result() {
        let json = r#"{"action":"override_result","text":"custom output"}"#;
        let resp: HookResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(resp, HookResponse::OverrideResult { text } if text == "custom output"));
    }

    #[test]
    fn test_hook_response_default() {
        let resp = HookResponse::default();
        assert!(matches!(resp, HookResponse::Pass));
    }

    #[test]
    fn test_hook_response_serialize_roundtrip() {
        let responses = [
            HookResponse::Pass,
            HookResponse::ModifyPrompt {
                text: "inject this".to_string(),
            },
            HookResponse::OverrideResult {
                text: "replaced".to_string(),
            },
            HookResponse::Abort {
                reason: "blocked".to_string(),
            },
        ];
        for resp in &responses {
            let json = serde_json::to_string(resp).unwrap();
            let parsed: HookResponse = serde_json::from_str(&json).unwrap();
            // Compare via serialization since HookResponse doesn't derive PartialEq.
            let re_json = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, re_json);
        }
    }

    #[test]
    fn test_event_all_contains_all_variants() {
        // Verify ALL array has exactly one of each variant.
        // We check count and uniqueness via Hash.
        let set: std::collections::HashSet<_> = HookEvent::ALL.iter().collect();
        assert_eq!(set.len(), HookEvent::ALL.len());
        assert_eq!(set.len(), 29);
    }
}
