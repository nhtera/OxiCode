//! Notification dispatch stub for the MCP bridge UI.
//!
//! In the full implementation this will push notifications to the connected
//! GUI client via JSON-RPC.  Until that transport is wired up the stub logs
//! via `tracing::debug!`.

// ---------------------------------------------------------------------------
// NotificationLevel
// ---------------------------------------------------------------------------

/// Severity level for a bridge UI notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationLevel {
    /// Informational message — no action required.
    Info,
    /// Warning — the user should be aware of a potential issue.
    Warning,
    /// Error — an operation failed and may require user intervention.
    Error,
}

// ---------------------------------------------------------------------------
// Stub implementation
// ---------------------------------------------------------------------------

/// Send a notification to the connected GUI client.
///
/// **Current state:** stub — logs the notification via `tracing::debug!`.
/// When the JSON-RPC bridge transport is connected, this function will send
/// a `notification/show` request to the client.
pub fn send_notification(level: NotificationLevel, message: &str) {
    // TODO: wire to JSON-RPC transport.
    tracing::debug!("Bridge notification [{level:?}]: {message}");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_info_notification() {
        // Smoke test — must not panic.
        send_notification(NotificationLevel::Info, "Connected to bridge");
    }

    #[test]
    fn test_send_warning_notification() {
        send_notification(NotificationLevel::Warning, "High latency detected");
    }

    #[test]
    fn test_send_error_notification() {
        send_notification(NotificationLevel::Error, "Bridge connection lost");
    }

    #[test]
    fn test_level_equality() {
        assert_eq!(NotificationLevel::Info, NotificationLevel::Info);
        assert_ne!(NotificationLevel::Info, NotificationLevel::Error);
    }
}
