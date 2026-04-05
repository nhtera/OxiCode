//! Diagnostic tracker — collects anonymous usage telemetry.
//!
//! All collection is opt-in: the tracker is disabled by default unless the
//! `OXICODE_TELEMETRY` environment variable is set to `1` or `true`.
//! No personally identifiable information (PII) is ever included in payloads.

use std::collections::HashMap;
use std::env;

/// Collects anonymous runtime diagnostics for opt-in telemetry.
///
/// Construct via [`DiagnosticTracker::new`], which reads `OXICODE_TELEMETRY`
/// from the environment. When disabled, all recording operations are no-ops.
#[derive(Debug, Default)]
pub struct DiagnosticTracker {
    /// Seconds this session has been running.
    pub session_duration_secs: u64,
    /// Number of times each tool was called.
    pub tool_call_counts: HashMap<String, u32>,
    /// Total errors encountered.
    pub error_count: u32,
    /// Active model identifier (e.g. `"claude-opus-4-5"`).
    pub model: String,
    /// Provider name (e.g. `"anthropic"`, `"bedrock"`).
    pub provider: String,
    /// Operating system family (e.g. `"linux"`, `"macos"`, `"windows"`).
    pub os: String,
    /// Oxicode version string.
    pub version: String,
    /// Whether telemetry collection is enabled.
    enabled: bool,
}

impl DiagnosticTracker {
    /// Create a new tracker.
    ///
    /// Enabled when the `OXICODE_TELEMETRY` env var equals `"1"` or `"true"`
    /// (case-insensitive). All other values (including absent) → disabled.
    pub fn new() -> Self {
        let enabled = matches!(
            env::var("OXICODE_TELEMETRY")
                .unwrap_or_default()
                .to_lowercase()
                .as_str(),
            "1" | "true"
        );

        if enabled {
            tracing::debug!("Diagnostic telemetry enabled");
        }

        Self {
            enabled,
            os: std::env::consts::OS.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ..Default::default()
        }
    }

    /// Whether telemetry collection is currently active.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Increment the call counter for `tool`. No-op when disabled.
    pub fn record_tool_call(&mut self, tool: &str) {
        if !self.enabled {
            return;
        }
        *self.tool_call_counts.entry(tool.to_string()).or_insert(0) += 1;
    }

    /// Increment the error counter. No-op when disabled.
    pub fn record_error(&mut self) {
        if !self.enabled {
            return;
        }
        self.error_count += 1;
    }

    /// Serialise the tracker state into a JSON payload suitable for upload.
    ///
    /// The payload contains **no PII** — only aggregate counts and platform info.
    pub fn to_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "session_duration_secs": self.session_duration_secs,
            "tool_call_counts": self.tool_call_counts,
            "error_count": self.error_count,
            "model": self.model,
            "provider": self.provider,
            "os": self.os,
            "version": self.version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default_without_env() {
        // OXICODE_TELEMETRY is unlikely to be set in test environment
        // but we create one with telemetry off explicitly.
        let tracker = DiagnosticTracker {
            enabled: false,
            ..Default::default()
        };
        assert!(!tracker.is_enabled());
    }

    #[test]
    fn record_tool_call_no_op_when_disabled() {
        let mut tracker = DiagnosticTracker {
            enabled: false,
            ..Default::default()
        };
        tracker.record_tool_call("bash");
        assert!(tracker.tool_call_counts.is_empty());
    }

    #[test]
    fn record_tool_and_error_when_enabled() {
        let mut tracker = DiagnosticTracker {
            enabled: true,
            ..Default::default()
        };
        tracker.record_tool_call("read_file");
        tracker.record_tool_call("read_file");
        tracker.record_error();

        assert_eq!(tracker.tool_call_counts["read_file"], 2);
        assert_eq!(tracker.error_count, 1);
    }

    #[test]
    fn payload_contains_expected_keys() {
        let tracker = DiagnosticTracker {
            enabled: true,
            session_duration_secs: 42,
            model: "claude-opus-4-5".to_string(),
            provider: "anthropic".to_string(),
            ..Default::default()
        };
        let payload = tracker.to_payload();
        assert_eq!(payload["session_duration_secs"], 42);
        assert_eq!(payload["model"], "claude-opus-4-5");
        assert_eq!(payload["provider"], "anthropic");
        // Ensure no PII-like keys are present
        assert!(payload.get("user_id").is_none());
        assert!(payload.get("email").is_none());
    }
}
