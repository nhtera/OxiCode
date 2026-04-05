//! Permission approval dialog stub for the MCP bridge UI.
//!
//! In the full implementation this will forward the request to the connected
//! GUI client via JSON-RPC.  Until that transport is wired up, the stub
//! auto-approves every request and logs the fact with `tracing::debug!`.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// A request to display a permission approval dialog to the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDialogRequest {
    /// Name of the tool requesting permission.
    pub tool_name: String,
    /// Arguments the tool will be invoked with (shown to the user).
    pub tool_args: serde_json::Value,
    /// Human-readable description of what the tool intends to do.
    pub description: String,
}

/// Response returned from the permission dialog (stub or live).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDialogResponse {
    /// `true` when the user (or stub) approved the action.
    pub approved: bool,
    /// Optional reason the user provided (denial or approval note).
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Stub implementation
// ---------------------------------------------------------------------------

/// Display a permission dialog and return the user's decision.
///
/// **Current state:** stub — always returns `approved = true`.
/// When the JSON-RPC bridge transport is connected, this function will send
/// a `permission/request` notification and await a `permission/response`.
pub fn send_permission_prompt(request: &PermissionDialogRequest) -> PermissionDialogResponse {
    // TODO: wire to actual JSON-RPC bridge transport.
    tracing::debug!(
        tool = %request.tool_name,
        "Bridge permission dialog (stub: auto-approved)"
    );
    PermissionDialogResponse {
        approved: true,
        reason: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_request() -> PermissionDialogRequest {
        PermissionDialogRequest {
            tool_name: "bash".to_string(),
            tool_args: json!({"cmd": "rm -rf /tmp/test"}),
            description: "Delete temporary test directory".to_string(),
        }
    }

    #[test]
    fn test_stub_auto_approves() {
        let req = make_request();
        let resp = send_permission_prompt(&req);
        assert!(resp.approved);
        assert!(resp.reason.is_none());
    }

    #[test]
    fn test_request_serialisation() {
        let req = make_request();
        let json = serde_json::to_string(&req).expect("serialise");
        assert!(json.contains("bash"));
        assert!(json.contains("Delete temporary"));
    }

    #[test]
    fn test_response_serialisation() {
        let resp = PermissionDialogResponse {
            approved: false,
            reason: Some("User denied".to_string()),
        };
        let json = serde_json::to_string(&resp).expect("serialise");
        assert!(json.contains("User denied"));
    }
}
