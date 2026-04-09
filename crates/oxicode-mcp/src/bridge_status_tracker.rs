//! Connection state tracking for the MCP bridge.
//!
//! Enabled only when the `bridge_debug` feature flag is active.

#[cfg(feature = "bridge_debug")]
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// ConnectionState
// ---------------------------------------------------------------------------

/// Current lifecycle state of the bridge connection.
#[cfg(feature = "bridge_debug")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// No active connection.
    Disconnected,
    /// Handshake / TLS / auth in progress.
    Connecting,
    /// Fully established and ready to exchange messages.
    Connected,
    /// Terminal error with a human-readable description.
    Error(String),
}

// ---------------------------------------------------------------------------
// BridgeStatusTracker
// ---------------------------------------------------------------------------

/// Tracks connection state transitions and uptime for the MCP bridge.
///
/// Only compiled when the `bridge_debug` feature is enabled.
#[cfg(feature = "bridge_debug")]
pub struct BridgeStatusTracker {
    /// Current connection state.
    state: ConnectionState,
    /// Instant of the most recent state transition.
    last_transition: Instant,
    /// Instant of the most recent `Connected` transition (for uptime calculation).
    connected_at: Option<Instant>,
    /// How many times we have re-entered `Connecting` after an error or disconnect.
    reconnect_count: u32,
}

#[cfg(feature = "bridge_debug")]
impl Default for BridgeStatusTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "bridge_debug")]
impl BridgeStatusTracker {
    /// Create a new tracker starting in `Disconnected`.
    pub fn new() -> Self {
        tracing::debug!("BridgeStatusTracker initialised (state=Disconnected)");
        Self {
            state: ConnectionState::Disconnected,
            last_transition: Instant::now(),
            connected_at: None,
            reconnect_count: 0,
        }
    }

    /// Transition to a new connection state.
    ///
    /// Increments `reconnect_count` when transitioning into `Connecting`
    /// from any state other than `Disconnected` on startup.
    pub fn transition(&mut self, new_state: ConnectionState) {
        tracing::debug!("Bridge state: {:?} → {:?}", self.state, new_state);

        // Count reconnection attempts (any Connecting transition after the first).
        if new_state == ConnectionState::Connecting && self.state != ConnectionState::Disconnected {
            self.reconnect_count += 1;
        }

        // Record when we became connected (for uptime).
        if new_state == ConnectionState::Connected {
            self.connected_at = Some(Instant::now());
        } else if matches!(
            new_state,
            ConnectionState::Disconnected | ConnectionState::Error(_)
        ) {
            self.connected_at = None;
        }

        self.state = new_state;
        self.last_transition = Instant::now();
    }

    /// Return a reference to the current state.
    pub fn state(&self) -> &ConnectionState {
        &self.state
    }

    /// Duration since the bridge last transitioned into `Connected`.
    ///
    /// Returns `Duration::ZERO` when not currently connected.
    pub fn uptime(&self) -> Duration {
        self.connected_at
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO)
    }

    /// Number of reconnection attempts since this tracker was created.
    pub fn reconnect_count(&self) -> u32 {
        self.reconnect_count
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "bridge_debug"))]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let tracker = BridgeStatusTracker::new();
        assert_eq!(*tracker.state(), ConnectionState::Disconnected);
        assert_eq!(tracker.reconnect_count(), 0);
        assert_eq!(tracker.uptime(), Duration::ZERO);
    }

    #[test]
    fn test_connect_transition() {
        let mut tracker = BridgeStatusTracker::new();
        tracker.transition(ConnectionState::Connecting);
        tracker.transition(ConnectionState::Connected);
        assert_eq!(*tracker.state(), ConnectionState::Connected);
        // Uptime should be a very small positive value.
        assert!(tracker.uptime() < Duration::from_secs(1));
    }

    #[test]
    fn test_reconnect_count_increments() {
        let mut tracker = BridgeStatusTracker::new();
        tracker.transition(ConnectionState::Connecting);
        tracker.transition(ConnectionState::Connected);
        // Disconnect then reconnect — count should increment.
        tracker.transition(ConnectionState::Disconnected);
        tracker.transition(ConnectionState::Connecting);
        assert_eq!(tracker.reconnect_count(), 1);
        tracker.transition(ConnectionState::Error("timeout".to_string()));
        tracker.transition(ConnectionState::Connecting);
        assert_eq!(tracker.reconnect_count(), 2);
    }
}
