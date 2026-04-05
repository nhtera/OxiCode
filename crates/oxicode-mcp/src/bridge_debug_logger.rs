//! Message-level debug logging for the MCP bridge.
//!
//! Enabled only when the `bridge_debug` feature flag is active.

#[cfg(feature = "bridge_debug")]
use std::collections::VecDeque;

#[cfg(feature = "bridge_debug")]
use std::time::Instant;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Direction of a logged bridge message.
#[cfg(feature = "bridge_debug")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Direction {
    /// Message received from the remote side.
    Inbound,
    /// Message sent to the remote side.
    Outbound,
}

/// A single logged bridge message entry.
#[cfg(feature = "bridge_debug")]
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Wall-clock instant when the message was recorded.
    pub timestamp: Instant,
    /// Whether the message was inbound or outbound.
    pub direction: Direction,
    /// The MCP message type string (e.g. `"request"`, `"response"`).
    pub message_type: String,
    /// First 200 characters of the raw payload for quick inspection.
    pub payload_preview: String,
}

// ---------------------------------------------------------------------------
// BridgeDebugLogger
// ---------------------------------------------------------------------------

/// Ring-buffer logger that records recent bridge messages for debug inspection.
///
/// Only compiled when the `bridge_debug` feature is enabled.
#[cfg(feature = "bridge_debug")]
pub struct BridgeDebugLogger {
    messages: VecDeque<LogEntry>,
    max_entries: usize,
}

#[cfg(feature = "bridge_debug")]
impl BridgeDebugLogger {
    /// Create a new logger with the given ring-buffer capacity.
    ///
    /// Pass `0` to use the default of 100 entries.
    pub fn new(max_entries: usize) -> Self {
        let cap = if max_entries == 0 { 100 } else { max_entries };
        tracing::debug!("BridgeDebugLogger created (capacity={cap})");
        Self {
            messages: VecDeque::with_capacity(cap),
            max_entries: cap,
        }
    }

    /// Record an inbound message.
    pub fn log_inbound(&mut self, msg_type: &str, payload: &str) {
        self.push(Direction::Inbound, msg_type, payload);
    }

    /// Record an outbound message.
    pub fn log_outbound(&mut self, msg_type: &str, payload: &str) {
        self.push(Direction::Outbound, msg_type, payload);
    }

    /// Return the most recent `count` entries (oldest first).
    pub fn recent(&self, count: usize) -> Vec<&LogEntry> {
        let skip = self.messages.len().saturating_sub(count);
        self.messages.iter().skip(skip).collect()
    }

    /// Clear all buffered entries.
    pub fn clear(&mut self) {
        self.messages.clear();
        tracing::debug!("BridgeDebugLogger cleared");
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn push(&mut self, direction: Direction, msg_type: &str, payload: &str) {
        // Truncate payload preview to 200 characters (byte-safe via char boundary).
        let preview_end = payload
            .char_indices()
            .nth(200)
            .map_or(payload.len(), |(i, _)| i);
        let payload_preview = payload[..preview_end].to_string();

        let entry = LogEntry {
            timestamp: Instant::now(),
            direction,
            message_type: msg_type.to_string(),
            payload_preview,
        };

        if self.messages.len() == self.max_entries {
            self.messages.pop_front();
        }
        self.messages.push_back(entry);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "bridge_debug"))]
mod tests {
    use super::*;

    #[test]
    fn test_log_inbound_outbound() {
        let mut logger = BridgeDebugLogger::new(10);
        logger.log_inbound("request", r#"{"id":1}"#);
        logger.log_outbound("response", r#"{"id":1,"result":{}}"#);
        assert_eq!(logger.messages.len(), 2);
        assert_eq!(logger.messages[0].direction, Direction::Inbound);
        assert_eq!(logger.messages[1].direction, Direction::Outbound);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let mut logger = BridgeDebugLogger::new(3);
        for i in 0..5u32 {
            logger.log_inbound("ping", &format!("payload-{i}"));
        }
        assert_eq!(logger.messages.len(), 3);
        // The oldest entries should have been evicted.
        assert!(logger.messages[0].payload_preview.contains("payload-2"));
    }

    #[test]
    fn test_recent_and_clear() {
        let mut logger = BridgeDebugLogger::new(0); // 0 → default 100
        logger.log_inbound("a", "aaa");
        logger.log_inbound("b", "bbb");
        logger.log_inbound("c", "ccc");
        let recent = logger.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message_type, "b");
        logger.clear();
        assert!(logger.messages.is_empty());
    }
}
