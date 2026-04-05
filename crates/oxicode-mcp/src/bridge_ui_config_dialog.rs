//! Configuration editor dialog stub for the MCP bridge UI.
//!
//! In the full implementation this will open an in-app config editor via
//! JSON-RPC.  Until that transport is wired up the stub returns
//! `modified = false` and logs a debug trace.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// A request to open the configuration editor dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDialogRequest {
    /// The current configuration as a JSON value shown in the editor.
    pub current_config: serde_json::Value,
    /// Keys that the user is allowed to edit (all others are read-only).
    pub editable_keys: Vec<String>,
}

/// Response from the configuration editor dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDialogResponse {
    /// `true` when the user saved changes; `false` when they dismissed/cancelled.
    pub modified: bool,
    /// The new configuration value, present only when `modified` is `true`.
    pub new_config: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Stub implementation
// ---------------------------------------------------------------------------

/// Open the config editor dialog and return the user's changes.
///
/// **Current state:** stub — always returns `modified = false` (no changes).
/// When the JSON-RPC bridge transport is connected, this function will send
/// a `config/edit` request and await a `config/response` with the updated
/// values.
pub fn send_config_editor(request: &ConfigDialogRequest) -> ConfigDialogResponse {
    // TODO: wire to actual JSON-RPC bridge transport.
    tracing::debug!(
        editable_keys = ?request.editable_keys,
        "Bridge config dialog (stub: no changes)"
    );
    ConfigDialogResponse {
        modified: false,
        new_config: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_request() -> ConfigDialogRequest {
        ConfigDialogRequest {
            current_config: json!({"model": "claude-3-5-sonnet", "max_tokens": 8192}),
            editable_keys: vec!["model".to_string(), "max_tokens".to_string()],
        }
    }

    #[test]
    fn test_stub_returns_no_changes() {
        let req = make_request();
        let resp = send_config_editor(&req);
        assert!(!resp.modified);
        assert!(resp.new_config.is_none());
    }

    #[test]
    fn test_request_serialisation() {
        let req = make_request();
        let json = serde_json::to_string(&req).expect("serialise");
        assert!(json.contains("model"));
        assert!(json.contains("max_tokens"));
    }

    #[test]
    fn test_response_with_changes_serialisation() {
        let resp = ConfigDialogResponse {
            modified: true,
            new_config: Some(json!({"model": "claude-opus-4"})),
        };
        let json = serde_json::to_string(&resp).expect("serialise");
        assert!(json.contains("claude-opus-4"));
        assert!(json.contains("true"));
    }
}
