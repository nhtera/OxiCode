//! Hook event types and payloads.
//!
//! Mirrors Claude Code's hook event taxonomy and JSON payload schema so
//! existing `~/.claude/settings.json` hook scripts work unchanged with
//! oxicode. Hook scripts receive a JSON object on stdin with fields like
//! `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `tool_name`,
//! `tool_input`, etc., matching the Claude Code spec.

use serde::{Deserialize, Serialize};

/// Hook lifecycle events. Names match Claude Code spec exactly so the same
/// `settings.json` works for both clients. Snake-case TOML keys (`pre_tool_use`)
/// are NOT supported — use the PascalCase form (`PreToolUse`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookEvent {
    /// Fires once at session start (also after `--resume` and after `/compact`).
    SessionStart,
    /// Fires once before session exit.
    SessionEnd,
    /// Fires when the user submits a prompt, before the LLM is called.
    UserPromptSubmit,
    /// Fires before each tool execution. Hook may block via
    /// `{"decision":"block","reason":"..."}` to cancel the tool call.
    PreToolUse,
    /// Fires after a successful tool execution. `tool_response` is included.
    PostToolUse,
    /// Fires after a tool execution that returned `is_error=true`.
    PostToolUseFailure,
    /// Fires once when the assistant turn ends (final EndTurn, not per intermediate turn).
    Stop,
    /// Fires when a spawned sub-agent finishes.
    SubagentStop,
    /// Fires before context compaction. `trigger` indicates `manual` / `auto`.
    PreCompact,
    /// Fires when the assistant emits a notification (idle, permission, etc).
    Notification,
}

impl HookEvent {
    /// All supported events.
    pub const ALL: &[Self] = &[
        Self::SessionStart,
        Self::SessionEnd,
        Self::UserPromptSubmit,
        Self::PreToolUse,
        Self::PostToolUse,
        Self::PostToolUseFailure,
        Self::Stop,
        Self::SubagentStop,
        Self::PreCompact,
        Self::Notification,
    ];

    /// Event name as used in config keys and the `hook_event_name` payload field.
    /// Always PascalCase (Claude Code spec).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PostToolUseFailure => "PostToolUseFailure",
            Self::Stop => "Stop",
            Self::SubagentStop => "SubagentStop",
            Self::PreCompact => "PreCompact",
            Self::Notification => "Notification",
        }
    }

    /// Parse event name from a string (case-sensitive PascalCase, Claude Code spec).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|e| e.as_str() == s)
    }
}

/// Payload sent to hook scripts via stdin (JSON). Field names match Claude
/// Code's `HookInput` types so existing scripts that read `tool_name` /
/// `tool_input` / `cwd` / `session_id` work unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    /// Session UUID (always present).
    pub session_id: String,
    /// Path to the session JSONL transcript file.
    #[serde(default)]
    pub transcript_path: String,
    /// Current working directory.
    #[serde(default)]
    pub cwd: String,
    /// Event name (PascalCase, e.g. "PreToolUse").
    pub hook_event_name: HookEvent,
    /// Permission mode at fire time: `default` | `bypass` | `approval_only` | `plan`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub permission_mode: String,
    /// Sub-agent ID (only set inside a sub-agent process).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Sub-agent type name (only set inside a sub-agent process).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// PreToolUse / PostToolUse / PostToolUseFailure: tool being invoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// PreToolUse / PostToolUse / PostToolUseFailure: tool input arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    /// PostToolUse: tool result content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<serde_json::Value>,
    /// PostToolUseFailure: whether the tool itself errored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// UserPromptSubmit: the user's prompt text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// PreCompact: trigger ("manual" or "auto").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// SessionStart: source ("startup" | "resume" | "compact").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// SubagentStop: stopped sub-agent's ID, name, and exit-error flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_agent_id: Option<String>,
    /// Stop: model's stop_reason ("end_turn" | "max_tokens" | "tool_use" | etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Notification: notification message text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Optional model name (for telemetry / multi-model setups).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl HookPayload {
    /// Construct a minimal payload with just the event + session id; other
    /// fields default to None / empty. Use the builder methods to fill in
    /// event-specific data.
    pub fn new(event: HookEvent, session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: event,
            permission_mode: String::new(),
            agent_id: None,
            agent_type: None,
            tool_name: None,
            tool_input: None,
            tool_response: None,
            is_error: None,
            prompt: None,
            trigger: None,
            source: None,
            stopped_agent_id: None,
            stop_reason: None,
            message: None,
            model: None,
        }
    }
}

/// Response from a hook script (read from stdout as JSON).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "action", rename_all = "snake_case")]
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
    /// Surface informational fields from a hook (Claude Code-style output).
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
        assert_eq!(HookEvent::ALL.len(), 10);
    }

    #[test]
    fn test_event_serialization_pascal_case() {
        let json = serde_json::to_string(&HookEvent::PreToolUse).unwrap();
        assert_eq!(json, "\"PreToolUse\"");
        let parsed: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HookEvent::PreToolUse);
    }

    #[test]
    fn test_user_prompt_submit_serde() {
        let json = serde_json::to_string(&HookEvent::UserPromptSubmit).unwrap();
        assert_eq!(json, "\"UserPromptSubmit\"");
    }

    #[test]
    fn test_subagent_stop_serde() {
        let json = serde_json::to_string(&HookEvent::SubagentStop).unwrap();
        assert_eq!(json, "\"SubagentStop\"");
    }

    #[test]
    fn test_from_str_round_trip() {
        for event in HookEvent::ALL {
            let s = event.as_str();
            assert_eq!(HookEvent::from_str(s), Some(*event));
        }
    }

    #[test]
    fn test_from_str_unknown_returns_none() {
        assert_eq!(HookEvent::from_str("NotAnEvent"), None);
        // Old snake_case names from previous schema must not match.
        assert_eq!(HookEvent::from_str("pre_query"), None);
        assert_eq!(HookEvent::from_str("tool_call_before"), None);
    }

    #[test]
    fn test_payload_serialization_omits_none() {
        let payload = HookPayload::new(HookEvent::SessionStart, "sess_1");
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"hook_event_name\":\"SessionStart\""));
        assert!(json.contains("\"session_id\":\"sess_1\""));
        assert!(!json.contains("tool_name"));
        assert!(!json.contains("prompt"));
    }

    #[test]
    fn test_pre_tool_use_payload() {
        let mut payload = HookPayload::new(HookEvent::PreToolUse, "sess_1");
        payload.tool_name = Some("Bash".to_string());
        payload.tool_input = Some(serde_json::json!({"command": "ls"}));
        payload.cwd = "/tmp/work".to_string();
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"tool_name\":\"Bash\""));
        assert!(json.contains("\"tool_input\":{\"command\":\"ls\"}"));
        assert!(json.contains("\"cwd\":\"/tmp/work\""));
    }

    #[test]
    fn test_hook_response_block_decision_is_separate() {
        // `decision: block` is not a HookResponse::Abort tag — that path is
        // handled by `parse_hook_output` in executor.rs.
        let json = r#"{"action":"abort","reason":"blocked"}"#;
        let resp: HookResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(resp, HookResponse::Abort { reason } if reason == "blocked"));
    }
}
