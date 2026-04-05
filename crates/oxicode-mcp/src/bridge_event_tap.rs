//! Broadcast tap for MCP bridge events — lets multiple observers subscribe
//! to a live stream of bridge messages without blocking the bridge itself.
//!
//! Enabled only when the `bridge_debug` feature flag is active.

#[cfg(feature = "bridge_debug")]
use std::time::Instant;

#[cfg(feature = "bridge_debug")]
use tokio::sync::broadcast;

// Re-export Direction so callers don't need to import bridge_debug_logger.
#[cfg(feature = "bridge_debug")]
pub use crate::bridge_debug_logger::Direction;

// ---------------------------------------------------------------------------
// TapEvent
// ---------------------------------------------------------------------------

/// A single event emitted by the bridge tap.
#[cfg(feature = "bridge_debug")]
#[derive(Debug, Clone)]
pub struct TapEvent {
    /// Wall-clock instant the event was created.
    pub timestamp: Instant,
    /// Whether the message was inbound or outbound.
    pub direction: Direction,
    /// MCP message type string (e.g. `"request"`, `"notification"`).
    pub message_type: String,
    /// Raw payload string (may be truncated by the caller).
    pub payload: String,
}

// ---------------------------------------------------------------------------
// EventTap
// ---------------------------------------------------------------------------

/// Multi-subscriber broadcast tap for live MCP bridge events.
///
/// Only compiled when the `bridge_debug` feature is enabled.
#[cfg(feature = "bridge_debug")]
pub struct EventTap {
    sender: broadcast::Sender<TapEvent>,
}

#[cfg(feature = "bridge_debug")]
impl EventTap {
    /// Create a new tap with the given channel capacity.
    ///
    /// Pass `0` to use the default capacity of 64 events.
    pub fn new(capacity: usize) -> Self {
        let cap = if capacity == 0 { 64 } else { capacity };
        let (sender, _) = broadcast::channel(cap);
        tracing::debug!("EventTap created (capacity={cap})");
        Self { sender }
    }

    /// Emit an event to all active subscribers.
    ///
    /// Silently drops the event when no subscribers are listening or when
    /// all subscriber channels are full (lagged receivers).
    pub fn emit(&self, event: TapEvent) {
        // `send` returns Err when there are no receivers — that is fine.
        let _ = self.sender.send(event);
    }

    /// Subscribe to the event stream.
    ///
    /// Each subscriber receives its own independent receive channel.
    /// Lagged receivers will skip missed events via `broadcast::error::RecvError::Lagged`.
    pub fn subscribe(&self) -> broadcast::Receiver<TapEvent> {
        self.sender.subscribe()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "bridge_debug"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_emit_and_receive() {
        let tap = EventTap::new(8);
        let mut rx = tap.subscribe();

        tap.emit(TapEvent {
            timestamp: Instant::now(),
            direction: Direction::Inbound,
            message_type: "request".to_string(),
            payload: r#"{"id":1}"#.to_string(),
        });

        let event = rx.recv().await.expect("should receive event");
        assert_eq!(event.message_type, "request");
        assert_eq!(event.direction, Direction::Inbound);
    }

    #[tokio::test]
    async fn test_no_subscriber_does_not_panic() {
        let tap = EventTap::new(0); // 0 → default 64
        // Emit with no subscribers — should not panic.
        tap.emit(TapEvent {
            timestamp: Instant::now(),
            direction: Direction::Outbound,
            message_type: "response".to_string(),
            payload: "{}".to_string(),
        });
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let tap = EventTap::new(8);
        let mut rx1 = tap.subscribe();
        let mut rx2 = tap.subscribe();

        tap.emit(TapEvent {
            timestamp: Instant::now(),
            direction: Direction::Outbound,
            message_type: "notify".to_string(),
            payload: "ping".to_string(),
        });

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        assert_eq!(e1.message_type, "notify");
        assert_eq!(e2.message_type, "notify");
    }
}
