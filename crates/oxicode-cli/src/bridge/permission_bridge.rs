//! Permission bridge: forward permission requests to IDE and receive decisions.
//!
//! When the query engine encounters a tool that requires approval, it emits a
//! `PermissionAsk` event. The bridge intercepts this, sends a notification to
//! the IDE, and waits for the IDE's decision (with a configurable timeout).

use std::time::Duration;

use oxicode_common::PermissionResponse;
use tokio::sync::oneshot;

use super::session_bridge::PermMap;

/// Default timeout waiting for IDE permission response (60 seconds).
const DEFAULT_PERMISSION_TIMEOUT_SECS: u64 = 60;

/// Manages pending permission requests for a bridge session.
pub struct PermissionBridge {
    /// Timeout for waiting on IDE response.
    timeout: Duration,
}

impl PermissionBridge {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(DEFAULT_PERMISSION_TIMEOUT_SECS),
        }
    }

    /// Create with a custom timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Register a pending permission request into the shared map.
    ///
    /// Returns a receiver that will resolve when the IDE sends its decision,
    /// and the permission_id used to track the request.
    pub async fn register_permission(
        &self,
        active_perms: &PermMap,
        permission_id: String,
        reply_tx: oneshot::Sender<PermissionResponse>,
    ) {
        active_perms.lock().await.insert(permission_id, reply_tx);
    }

    /// Resolve a pending permission request with the IDE's decision.
    ///
    /// Returns `true` if the permission was found and resolved, `false` otherwise.
    pub async fn resolve_permission(
        &self,
        active_perms: &PermMap,
        permission_id: &str,
        approve: bool,
        always: bool,
    ) -> bool {
        let mut perms = active_perms.lock().await;
        if let Some(reply_tx) = perms.remove(permission_id) {
            let response = match (approve, always) {
                (true, true) => PermissionResponse::AlwaysAllow,
                (true, false) => PermissionResponse::AllowOnce,
                (false, true) => PermissionResponse::AlwaysDeny,
                (false, false) => PermissionResponse::Deny,
            };
            reply_tx.send(response).is_ok()
        } else {
            false
        }
    }

    /// Wait for a permission to be resolved, with timeout.
    ///
    /// If the IDE doesn't respond within the timeout, returns `Deny` by default.
    pub async fn wait_for_decision(
        &self,
        rx: oneshot::Receiver<PermissionResponse>,
    ) -> PermissionResponse {
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => {
                tracing::warn!("permission reply channel closed unexpectedly");
                PermissionResponse::Deny
            }
            Err(_) => {
                tracing::warn!(
                    timeout_secs = self.timeout.as_secs(),
                    "permission request timed out — defaulting to deny"
                );
                PermissionResponse::Deny
            }
        }
    }

    /// Get the configured timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Count pending permissions in the shared map.
    pub async fn pending_count(active_perms: &PermMap) -> usize {
        active_perms.lock().await.len()
    }

    /// Clear all pending permissions (used during shutdown/cancel).
    /// Drops all reply channels, causing waiters to get `Deny`.
    pub async fn clear_all(active_perms: &PermMap) {
        active_perms.lock().await.clear();
    }
}

impl Default for PermissionBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use super::*;

    fn make_perm_map() -> PermMap {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[tokio::test]
    async fn register_and_resolve_approve() {
        let bridge = PermissionBridge::new();
        let perms = make_perm_map();
        let (tx, rx) = oneshot::channel();

        bridge
            .register_permission(&perms, "p1".to_string(), tx)
            .await;
        assert_eq!(PermissionBridge::pending_count(&perms).await, 1);

        let resolved = bridge
            .resolve_permission(&perms, "p1", true, false)
            .await;
        assert!(resolved);
        assert_eq!(rx.await.unwrap(), PermissionResponse::AllowOnce);
    }

    #[tokio::test]
    async fn register_and_resolve_deny() {
        let bridge = PermissionBridge::new();
        let perms = make_perm_map();
        let (tx, rx) = oneshot::channel();

        bridge
            .register_permission(&perms, "p2".to_string(), tx)
            .await;
        let resolved = bridge
            .resolve_permission(&perms, "p2", false, false)
            .await;
        assert!(resolved);
        assert_eq!(rx.await.unwrap(), PermissionResponse::Deny);
    }

    #[tokio::test]
    async fn resolve_always_allow() {
        let bridge = PermissionBridge::new();
        let perms = make_perm_map();
        let (tx, rx) = oneshot::channel();

        bridge
            .register_permission(&perms, "p3".to_string(), tx)
            .await;
        bridge
            .resolve_permission(&perms, "p3", true, true)
            .await;
        assert_eq!(rx.await.unwrap(), PermissionResponse::AlwaysAllow);
    }

    #[tokio::test]
    async fn resolve_always_deny() {
        let bridge = PermissionBridge::new();
        let perms = make_perm_map();
        let (tx, rx) = oneshot::channel();

        bridge
            .register_permission(&perms, "p4".to_string(), tx)
            .await;
        bridge
            .resolve_permission(&perms, "p4", false, true)
            .await;
        assert_eq!(rx.await.unwrap(), PermissionResponse::AlwaysDeny);
    }

    #[tokio::test]
    async fn resolve_nonexistent_returns_false() {
        let bridge = PermissionBridge::new();
        let perms = make_perm_map();
        let resolved = bridge
            .resolve_permission(&perms, "nonexistent", true, false)
            .await;
        assert!(!resolved);
    }

    #[tokio::test]
    async fn timeout_returns_deny() {
        let bridge = PermissionBridge::with_timeout(Duration::from_millis(50));
        let (_tx, rx) = oneshot::channel::<PermissionResponse>();

        // tx is held but never sent — should timeout.
        let result = bridge.wait_for_decision(rx).await;
        assert_eq!(result, PermissionResponse::Deny);
    }

    #[tokio::test]
    async fn dropped_sender_returns_deny() {
        let bridge = PermissionBridge::new();
        let (tx, rx) = oneshot::channel::<PermissionResponse>();
        drop(tx); // Simulate sender being dropped.

        let result = bridge.wait_for_decision(rx).await;
        assert_eq!(result, PermissionResponse::Deny);
    }

    #[tokio::test]
    async fn clear_all_empties_map() {
        let perms = make_perm_map();
        let (tx1, _rx1) = oneshot::channel();
        let (tx2, _rx2) = oneshot::channel();
        perms.lock().await.insert("p1".to_string(), tx1);
        perms.lock().await.insert("p2".to_string(), tx2);

        assert_eq!(PermissionBridge::pending_count(&perms).await, 2);
        PermissionBridge::clear_all(&perms).await;
        assert_eq!(PermissionBridge::pending_count(&perms).await, 0);
    }

    #[test]
    fn default_timeout() {
        let bridge = PermissionBridge::default();
        assert_eq!(bridge.timeout().as_secs(), DEFAULT_PERMISSION_TIMEOUT_SECS);
    }

    #[test]
    fn custom_timeout() {
        let bridge = PermissionBridge::with_timeout(Duration::from_secs(30));
        assert_eq!(bridge.timeout().as_secs(), 30);
    }
}
