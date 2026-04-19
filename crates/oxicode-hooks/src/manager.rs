//! HookManager: central coordinator for hook lifecycle events.
//!
//! Loads config, fires events, collects responses.
//! Dispatches to command/agent/http executors based on hook type.

use crate::config::HooksConfig;
use crate::events::{HookEvent, HookPayload, HookResponse};
use crate::executor::execute_hook;

/// Manages hook lifecycle: config + dispatch. Carries ambient session
/// metadata (id, cwd, transcript path, permission mode, model) so each
/// hook fire produces a Claude-Code-compatible payload without callers
/// having to thread these fields through every fire-site.
pub struct HookManager {
    config: HooksConfig,
    session_id: String,
    transcript_path: String,
    cwd: String,
    permission_mode: String,
    model: Option<String>,
    agent_id: Option<String>,
    agent_type: Option<String>,
}

impl HookManager {
    /// Create from an existing config.
    pub fn new(config: HooksConfig) -> Self {
        Self {
            config,
            session_id: String::new(),
            transcript_path: String::new(),
            cwd: std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(String::from))
                .unwrap_or_default(),
            permission_mode: String::new(),
            model: None,
            agent_id: None,
            agent_type: None,
        }
    }

    /// Load from default settings directory (Claude Code-compatible: reads
    /// `~/.claude/settings.json`, project `.claude/settings.json`, and
    /// `~/.oxicode/settings.toml` overlay).
    pub fn from_settings() -> Self {
        Self::new(HooksConfig::load_layered())
    }

    /// Create a no-op manager (no hooks configured).
    pub fn noop() -> Self {
        Self::new(HooksConfig::default())
    }

    /// Set current session ID (included in payloads).
    pub fn set_session_id(&mut self, id: impl Into<String>) {
        self.session_id = id.into();
    }

    /// Set current model (included in payloads).
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = Some(model.into());
    }

    /// Set the working directory for hook payloads.
    pub fn set_cwd(&mut self, cwd: impl Into<String>) {
        self.cwd = cwd.into();
    }

    /// Set the transcript JSONL path for hook payloads.
    pub fn set_transcript_path(&mut self, path: impl Into<String>) {
        self.transcript_path = path.into();
    }

    /// Set the current permission mode for hook payloads.
    pub fn set_permission_mode(&mut self, mode: impl Into<String>) {
        self.permission_mode = mode.into();
    }

    /// Mark this manager as belonging to a sub-agent (sets `agent_id`/`agent_type`
    /// on every payload — Claude Code parity).
    pub fn set_subagent(&mut self, id: impl Into<String>, agent_type: impl Into<String>) {
        self.agent_id = Some(id.into());
        self.agent_type = Some(agent_type.into());
    }

    /// Check if a hook is registered for the given event.
    pub fn has_hook(&self, event: HookEvent) -> bool {
        self.config.get(event).is_some()
    }

    /// Fire a hook event with a fully-built payload (advanced use; most
    /// callers should prefer `fire_with_fields` or one of the typed helpers).
    pub async fn fire_payload(&self, event: HookEvent, payload: HookPayload) -> HookResponse {
        let Some(hook_def) = self.config.get(event) else {
            return HookResponse::Pass;
        };

        tracing::debug!(
            "Firing hook {} (type={:?}): {}",
            event.as_str(),
            hook_def.hook_type,
            hook_def.command
        );

        execute_hook(hook_def, &payload).await
    }

    /// Fire an event by overlaying caller-supplied per-event fields onto the
    /// manager's ambient session metadata. Use `HookFields` to set
    /// `tool_name` / `tool_input` / `prompt` / etc. before firing.
    pub async fn fire_with_fields(&self, event: HookEvent, fields: HookFields) -> HookResponse {
        if self.config.get(event).is_none() {
            return HookResponse::Pass;
        }
        let payload = self.build_payload(event, fields);
        self.fire_payload(event, payload).await
    }

    /// Backward-compatible alias for the most common fire pattern: an event
    /// + an arbitrary JSON `data` blob. The blob's well-known keys
    /// (`tool`, `input`, `result`, `prompt`, `trigger`, `source`) are mapped
    /// onto the structured `HookPayload` fields. Unknown keys are dropped.
    pub async fn fire(&self, event: HookEvent, data: serde_json::Value) -> HookResponse {
        let mut fields = HookFields::default();
        if let Some(obj) = data.as_object() {
            if let Some(v) = obj.get("tool").and_then(|v| v.as_str()) {
                fields.tool_name = Some(v.to_string());
            }
            if let Some(v) = obj.get("tool_name").and_then(|v| v.as_str()) {
                fields.tool_name = Some(v.to_string());
            }
            if let Some(v) = obj.get("input") {
                fields.tool_input = Some(v.clone());
            }
            if let Some(v) = obj.get("tool_input") {
                fields.tool_input = Some(v.clone());
            }
            if let Some(v) = obj.get("result") {
                fields.tool_response = Some(v.clone());
            }
            if let Some(v) = obj.get("tool_response") {
                fields.tool_response = Some(v.clone());
            }
            if let Some(v) = obj.get("is_error").and_then(|v| v.as_bool()) {
                fields.is_error = Some(v);
            }
            if let Some(v) = obj.get("prompt").and_then(|v| v.as_str()) {
                fields.prompt = Some(v.to_string());
            }
            if let Some(v) = obj.get("trigger").and_then(|v| v.as_str()) {
                fields.trigger = Some(v.to_string());
            }
            if let Some(v) = obj.get("source").and_then(|v| v.as_str()) {
                fields.source = Some(v.to_string());
            }
            if let Some(v) = obj.get("stop_reason").and_then(|v| v.as_str()) {
                fields.stop_reason = Some(v.to_string());
            }
            if let Some(v) = obj.get("message").and_then(|v| v.as_str()) {
                fields.message = Some(v.to_string());
            }
            if let Some(v) = obj.get("agent_id").and_then(|v| v.as_str()) {
                fields.stopped_agent_id = Some(v.to_string());
            }
        }
        self.fire_with_fields(event, fields).await
    }

    /// Fire a simple event with no extra data.
    pub async fn fire_simple(&self, event: HookEvent) -> HookResponse {
        self.fire_with_fields(event, HookFields::default()).await
    }

    /// Get the underlying config (for `/hooks` command listing).
    pub fn config(&self) -> &HooksConfig {
        &self.config
    }

    /// Build a fully-populated HookPayload by overlaying `fields` onto the
    /// manager's ambient session metadata.
    fn build_payload(&self, event: HookEvent, fields: HookFields) -> HookPayload {
        HookPayload {
            session_id: self.session_id.clone(),
            transcript_path: self.transcript_path.clone(),
            cwd: self.cwd.clone(),
            hook_event_name: event,
            permission_mode: self.permission_mode.clone(),
            agent_id: self.agent_id.clone(),
            agent_type: self.agent_type.clone(),
            tool_name: fields.tool_name,
            tool_input: fields.tool_input,
            tool_response: fields.tool_response,
            is_error: fields.is_error,
            prompt: fields.prompt,
            trigger: fields.trigger,
            source: fields.source,
            stopped_agent_id: fields.stopped_agent_id,
            stop_reason: fields.stop_reason,
            message: fields.message,
            model: self.model.clone(),
        }
    }
}

/// Per-event fields callers may want to set before firing a hook. Typed
/// builder so call sites don't fiddle with stringly-typed JSON `data` blobs.
#[derive(Debug, Default, Clone)]
pub struct HookFields {
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_response: Option<serde_json::Value>,
    pub is_error: Option<bool>,
    pub prompt: Option<String>,
    pub trigger: Option<String>,
    pub source: Option<String>,
    pub stopped_agent_id: Option<String>,
    pub stop_reason: Option<String>,
    pub message: Option<String>,
}

impl HookFields {
    pub fn for_tool(tool_name: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            tool_name: Some(tool_name.into()),
            tool_input: Some(input),
            ..Self::default()
        }
    }

    pub fn for_tool_response(
        tool_name: impl Into<String>,
        input: serde_json::Value,
        response: serde_json::Value,
        is_error: bool,
    ) -> Self {
        Self {
            tool_name: Some(tool_name.into()),
            tool_input: Some(input),
            tool_response: Some(response),
            is_error: Some(is_error),
            ..Self::default()
        }
    }

    pub fn for_user_prompt(prompt: impl Into<String>) -> Self {
        Self {
            prompt: Some(prompt.into()),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_manager_returns_pass() {
        let manager = HookManager::noop();
        let response = manager.fire_simple(HookEvent::SessionStart).await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_has_hook_false_for_noop() {
        let manager = HookManager::noop();
        assert!(!manager.has_hook(HookEvent::PreToolUse));
    }

    #[tokio::test]
    async fn test_configured_hook_fires_with_pascal_case_key() {
        let toml_str = r#"SessionStart = "echo pass""#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let manager = HookManager::new(config);

        assert!(manager.has_hook(HookEvent::SessionStart));
        let resp = manager.fire_simple(HookEvent::SessionStart).await;
        assert!(matches!(resp, HookResponse::Pass));
    }
}
