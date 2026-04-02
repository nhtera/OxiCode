//! HookManager: central coordinator for hook lifecycle events.
//!
//! Loads config, fires events, collects responses.

use std::time::Duration;

use crate::config::HooksConfig;
use crate::events::{HookEvent, HookPayload, HookResponse};
use crate::executor::execute_hook_script;

/// Manages hook lifecycle: config + dispatch.
pub struct HookManager {
    config: HooksConfig,
    session_id: Option<String>,
    model: Option<String>,
}

impl HookManager {
    /// Create from an existing config.
    pub fn new(config: HooksConfig) -> Self {
        Self {
            config,
            session_id: None,
            model: None,
        }
    }

    /// Load from default settings directory.
    pub fn from_settings() -> Self {
        Self::new(HooksConfig::load_from_settings_dir())
    }

    /// Create a no-op manager (no hooks configured).
    pub fn noop() -> Self {
        Self::new(HooksConfig::default())
    }

    /// Set current session ID (included in payloads).
    pub fn set_session_id(&mut self, id: impl Into<String>) {
        self.session_id = Some(id.into());
    }

    /// Set current model (included in payloads).
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = Some(model.into());
    }

    /// Check if a hook is registered for the given event.
    pub fn has_hook(&self, event: HookEvent) -> bool {
        self.config.get(event).is_some()
    }

    /// Fire a hook event with event-specific data.
    ///
    /// Returns `HookResponse::Pass` if no hook is configured for this event.
    pub async fn fire(&self, event: HookEvent, data: serde_json::Value) -> HookResponse {
        let Some(hook_def) = self.config.get(event) else {
            return HookResponse::Pass;
        };

        let payload = HookPayload {
            event,
            data,
            session_id: self.session_id.clone(),
            model: self.model.clone(),
        };

        let timeout = Duration::from_secs(hook_def.timeout_secs);
        tracing::debug!("Firing hook {}: {}", event.as_str(), hook_def.command);

        execute_hook_script(&hook_def.command, &payload, Some(timeout)).await
    }

    /// Fire a simple event with no extra data.
    pub async fn fire_simple(&self, event: HookEvent) -> HookResponse {
        self.fire(event, serde_json::json!({})).await
    }

    /// Get the underlying config (for /hooks command listing).
    pub fn config(&self) -> &HooksConfig {
        &self.config
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
        assert!(!manager.has_hook(HookEvent::PreQuery));
    }

    #[tokio::test]
    async fn test_configured_hook_fires() {
        let toml_str = "session_start = \"echo pass\"";
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let manager = HookManager::new(config);

        assert!(manager.has_hook(HookEvent::SessionStart));
        // "echo pass" outputs "pass" which is not valid JSON → HookResponse::Pass (fallback).
        let resp = manager.fire_simple(HookEvent::SessionStart).await;
        assert!(matches!(resp, HookResponse::Pass));
    }
}
