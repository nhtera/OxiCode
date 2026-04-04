//! Bridge connection status — health, uptime, message counts.
//!
//! Exposed via `/bridge-status` command and TUI status bar.

use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Connection state of the bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ConnectionState {
    /// Bridge is connected and operational.
    Connected,
    /// Bridge is not connected (initial state or after giving up reconnection).
    #[default]
    Disconnected,
    /// Bridge is attempting to reconnect after a disconnect.
    Reconnecting,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connected => write!(f, "connected"),
            Self::Disconnected => write!(f, "disconnected"),
            Self::Reconnecting => write!(f, "reconnecting"),
        }
    }
}

/// Snapshot of bridge status for display / serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStatusSnapshot {
    pub state: ConnectionState,
    pub uptime_secs: f64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub reconnect_attempts: u32,
    pub last_error: Option<String>,
}

/// Live bridge status tracker (thread-safe via atomics).
pub struct BridgeStatus {
    state: std::sync::Mutex<ConnectionState>,
    connected_at: std::sync::Mutex<Option<Instant>>,
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    reconnect_attempts: AtomicU32,
    last_error: std::sync::Mutex<Option<String>>,
}

impl BridgeStatus {
    /// Create a new status tracker in disconnected state.
    pub fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(ConnectionState::Disconnected),
            connected_at: std::sync::Mutex::new(None),
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            reconnect_attempts: AtomicU32::new(0),
            last_error: std::sync::Mutex::new(None),
        }
    }

    /// Mark bridge as connected. Resets reconnect attempts, records connect time.
    pub fn on_connected(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = ConnectionState::Connected;
        }
        if let Ok(mut ts) = self.connected_at.lock() {
            *ts = Some(Instant::now());
        }
        self.reconnect_attempts.store(0, Ordering::Relaxed);
        if let Ok(mut err) = self.last_error.lock() {
            *err = None;
        }
    }

    /// Mark bridge as disconnected. Clears uptime tracking.
    pub fn on_disconnected(&self, error: Option<String>) {
        if let Ok(mut state) = self.state.lock() {
            *state = ConnectionState::Disconnected;
        }
        if let Ok(mut ts) = self.connected_at.lock() {
            *ts = None; // Clear so uptime() returns None when disconnected.
        }
        if let Some(e) = error {
            if let Ok(mut err) = self.last_error.lock() {
                *err = Some(e);
            }
        }
    }

    /// Mark bridge as reconnecting (increments attempt counter).
    pub fn on_reconnecting(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = ConnectionState::Reconnecting;
        }
        self.reconnect_attempts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a sent message.
    pub fn on_message_sent(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a received message.
    pub fn on_message_received(&self) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Set last error (without changing state).
    pub fn set_error(&self, error: String) {
        if let Ok(mut err) = self.last_error.lock() {
            *err = Some(error);
        }
    }

    /// Current connection state.
    pub fn state(&self) -> ConnectionState {
        self.state
            .lock()
            .map(|guard| *guard)
            .unwrap_or(ConnectionState::Disconnected)
    }

    /// Uptime since last successful connection (None if never connected).
    pub fn uptime(&self) -> Option<std::time::Duration> {
        self.connected_at
            .lock()
            .ok()
            .and_then(|ts| ts.map(|t| t.elapsed()))
    }

    /// Take a serializable snapshot of the current status.
    pub fn snapshot(&self) -> BridgeStatusSnapshot {
        BridgeStatusSnapshot {
            state: self.state(),
            uptime_secs: self.uptime().map_or(0.0, |d| d.as_secs_f64()),
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            messages_received: self.messages_received.load(Ordering::Relaxed),
            reconnect_attempts: self.reconnect_attempts.load(Ordering::Relaxed),
            last_error: self.last_error.lock().ok().and_then(|e| e.clone()),
        }
    }
}

impl Default for BridgeStatus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let status = BridgeStatus::new();
        assert_eq!(status.state(), ConnectionState::Disconnected);
        assert!(status.uptime().is_none());
        let snap = status.snapshot();
        assert_eq!(snap.messages_sent, 0);
        assert_eq!(snap.messages_received, 0);
        assert_eq!(snap.reconnect_attempts, 0);
        assert!(snap.last_error.is_none());
    }

    #[test]
    fn test_connected_state() {
        let status = BridgeStatus::new();
        status.on_connected();
        assert_eq!(status.state(), ConnectionState::Connected);
        assert!(status.uptime().is_some());
    }

    #[test]
    fn test_disconnect_records_error() {
        let status = BridgeStatus::new();
        status.on_connected();
        assert!(status.uptime().is_some());
        status.on_disconnected(Some("connection reset".to_string()));
        assert_eq!(status.state(), ConnectionState::Disconnected);
        let snap = status.snapshot();
        assert_eq!(snap.last_error.as_deref(), Some("connection reset"));
        // Uptime should be cleared on disconnect.
        assert!(status.uptime().is_none());
    }

    #[test]
    fn test_reconnecting_increments_attempts() {
        let status = BridgeStatus::new();
        status.on_reconnecting();
        status.on_reconnecting();
        status.on_reconnecting();
        assert_eq!(status.state(), ConnectionState::Reconnecting);
        assert_eq!(status.snapshot().reconnect_attempts, 3);
    }

    #[test]
    fn test_connected_resets_attempts() {
        let status = BridgeStatus::new();
        status.on_reconnecting();
        status.on_reconnecting();
        status.on_connected();
        assert_eq!(status.snapshot().reconnect_attempts, 0);
    }

    #[test]
    fn test_message_counts() {
        let status = BridgeStatus::new();
        status.on_message_sent();
        status.on_message_sent();
        status.on_message_received();
        let snap = status.snapshot();
        assert_eq!(snap.messages_sent, 2);
        assert_eq!(snap.messages_received, 1);
    }

    #[test]
    fn test_connection_state_display() {
        assert_eq!(ConnectionState::Connected.to_string(), "connected");
        assert_eq!(ConnectionState::Disconnected.to_string(), "disconnected");
        assert_eq!(ConnectionState::Reconnecting.to_string(), "reconnecting");
    }

    #[test]
    fn test_connection_state_serde() {
        let json = serde_json::to_string(&ConnectionState::Reconnecting).unwrap();
        assert_eq!(json, "\"reconnecting\"");
        let parsed: ConnectionState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ConnectionState::Reconnecting);
    }

    #[test]
    fn test_snapshot_serde() {
        let snap = BridgeStatusSnapshot {
            state: ConnectionState::Connected,
            uptime_secs: 123.45,
            messages_sent: 10,
            messages_received: 8,
            reconnect_attempts: 2,
            last_error: None,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: BridgeStatusSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.messages_sent, 10);
        assert_eq!(parsed.state, ConnectionState::Connected);
    }

    #[test]
    fn test_set_error() {
        let status = BridgeStatus::new();
        status.on_connected();
        status.set_error("timeout".to_string());
        assert_eq!(status.state(), ConnectionState::Connected); // state unchanged
        assert_eq!(status.snapshot().last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_connected_clears_error() {
        let status = BridgeStatus::new();
        status.set_error("previous error".to_string());
        status.on_connected();
        assert!(status.snapshot().last_error.is_none());
    }
}
