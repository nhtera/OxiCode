//! Permission bridge — forward tool permission requests to remote WebSocket clients.
//!
//! When a tool needs user approval, the bridge serializes the request as JSON,
//! sends it to the remote client, and waits for the approval/denial response.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// Permission request sent to remote client via WebSocket.
#[derive(Debug, Clone, Serialize)]
pub struct RemotePermissionRequest {
    /// Unique request ID for correlating responses.
    pub request_id: String,
    /// Tool name requesting permission.
    pub tool_name: String,
    /// Summary of the tool input (human-readable).
    pub input_summary: String,
    /// Full permission prompt text.
    pub prompt: String,
}

/// Permission response from remote client.
#[derive(Debug, Clone, Deserialize)]
pub struct RemotePermissionResponse {
    /// Must match the `request_id` from the request.
    pub request_id: String,
    /// Whether the user approved the tool execution.
    pub approved: bool,
}

/// Timeout for waiting for a remote permission response.
const PERMISSION_TIMEOUT_SECS: u64 = 60;

/// Bridge that manages pending permission requests and their response channels.
#[derive(Debug, Default)]
pub struct PermissionBridge {
    /// Pending requests awaiting response, keyed by request_id.
    pending: std::collections::HashMap<String, oneshot::Sender<bool>>,
}

impl PermissionBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new permission request. Returns a future that resolves when
    /// the remote client responds (or times out).
    ///
    /// The caller should serialize `request` and send it over WebSocket.
    pub fn register_request(
        &mut self,
        request: &RemotePermissionRequest,
    ) -> tokio::task::JoinHandle<bool> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(request.request_id.clone(), tx);

        // Spawn a timeout wrapper — returns false if no response in time.
        tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(PERMISSION_TIMEOUT_SECS), rx).await {
                Ok(Ok(approved)) => approved,
                Ok(Err(_)) => {
                    tracing::warn!("Permission request channel dropped");
                    false
                }
                Err(_) => {
                    tracing::warn!("Permission request timed out after {PERMISSION_TIMEOUT_SECS}s");
                    false
                }
            }
        })
    }

    /// Handle a permission response from the remote client.
    /// Returns `true` if the response was matched to a pending request.
    pub fn handle_response(&mut self, response: RemotePermissionResponse) -> bool {
        if let Some(tx) = self.pending.remove(&response.request_id) {
            let _ = tx.send(response.approved);
            true
        } else {
            tracing::warn!(
                request_id = %response.request_id,
                "Permission response for unknown request"
            );
            false
        }
    }

    /// Number of pending permission requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Cancel all pending requests (e.g. on session disconnect).
    pub fn cancel_all(&mut self) {
        let count = self.pending.len();
        self.pending.clear();
        if count > 0 {
            tracing::info!("Cancelled {count} pending permission requests");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_request_serialization() {
        let req = RemotePermissionRequest {
            request_id: "req-123".to_string(),
            tool_name: "Bash".to_string(),
            input_summary: "rm -rf /tmp/test".to_string(),
            prompt: "Allow Bash to run: rm -rf /tmp/test?".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("req-123"));
        assert!(json.contains("Bash"));
    }

    #[test]
    fn test_permission_response_deserialization() {
        let json = r#"{"request_id":"req-123","approved":true}"#;
        let resp: RemotePermissionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.request_id, "req-123");
        assert!(resp.approved);
    }

    #[tokio::test]
    async fn test_bridge_handle_response() {
        let mut bridge = PermissionBridge::new();
        let req = RemotePermissionRequest {
            request_id: "req-1".to_string(),
            tool_name: "Write".to_string(),
            input_summary: "write file.txt".to_string(),
            prompt: "Allow?".to_string(),
        };

        let handle = bridge.register_request(&req);
        assert_eq!(bridge.pending_count(), 1);

        let resp = RemotePermissionResponse {
            request_id: "req-1".to_string(),
            approved: true,
        };
        assert!(bridge.handle_response(resp));
        assert_eq!(bridge.pending_count(), 0);

        let result = handle.await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn test_bridge_unknown_response() {
        let mut bridge = PermissionBridge::new();
        let resp = RemotePermissionResponse {
            request_id: "unknown".to_string(),
            approved: true,
        };
        assert!(!bridge.handle_response(resp));
    }

    #[tokio::test]
    async fn test_bridge_cancel_all() {
        let mut bridge = PermissionBridge::new();
        let req = RemotePermissionRequest {
            request_id: "req-1".to_string(),
            tool_name: "Test".to_string(),
            input_summary: "test".to_string(),
            prompt: "Allow?".to_string(),
        };
        let _handle = bridge.register_request(&req);
        assert_eq!(bridge.pending_count(), 1);
        bridge.cancel_all();
        assert_eq!(bridge.pending_count(), 0);
    }
}
