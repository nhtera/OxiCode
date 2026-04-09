//! Latency and error diagnostics for the MCP bridge.
//!
//! Enabled only when the `bridge_debug` feature flag is active.

#[cfg(feature = "bridge_debug")]
use std::collections::HashMap;

#[cfg(feature = "bridge_debug")]
use std::time::Duration;

// ---------------------------------------------------------------------------
// BridgeDiagnostics
// ---------------------------------------------------------------------------

/// Collects per-message-type latency samples and error counts for the bridge.
///
/// Only compiled when the `bridge_debug` feature is enabled.
#[cfg(feature = "bridge_debug")]
pub struct BridgeDiagnostics {
    /// Per message-type latency samples (sorted on insertion via binary search).
    latencies: HashMap<String, Vec<Duration>>,
    /// Total error count across all message types.
    error_count: u32,
    /// Total messages recorded (latencies + errors).
    total_messages: u64,
}

#[cfg(feature = "bridge_debug")]
impl Default for BridgeDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "bridge_debug")]
impl BridgeDiagnostics {
    /// Create a new, empty diagnostics collector.
    pub fn new() -> Self {
        tracing::debug!("BridgeDiagnostics created");
        Self {
            latencies: HashMap::new(),
            error_count: 0,
            total_messages: 0,
        }
    }

    /// Record a round-trip latency sample for the given message type.
    pub fn record_latency(&mut self, msg_type: &str, latency: Duration) {
        let samples = self.latencies.entry(msg_type.to_string()).or_default();
        // Keep the vector sorted so percentile queries are O(1).
        let pos = samples.partition_point(|&d| d <= latency);
        samples.insert(pos, latency);
        self.total_messages += 1;
    }

    /// Record a protocol or transport error (no latency available).
    pub fn record_error(&mut self) {
        self.error_count += 1;
        self.total_messages += 1;
        tracing::warn!("Bridge error recorded (total errors={})", self.error_count);
    }

    /// 50th-percentile latency for the given message type.
    ///
    /// Returns `None` when no samples have been recorded.
    pub fn p50(&self, msg_type: &str) -> Option<Duration> {
        self.percentile(msg_type, 50)
    }

    /// 95th-percentile latency for the given message type.
    ///
    /// Returns `None` when no samples have been recorded.
    pub fn p95(&self, msg_type: &str) -> Option<Duration> {
        self.percentile(msg_type, 95)
    }

    /// Total number of recorded messages (including errors).
    pub fn total_messages(&self) -> u64 {
        self.total_messages
    }

    /// Total error count.
    pub fn error_count(&self) -> u32 {
        self.error_count
    }

    /// Human-readable summary of all collected diagnostics.
    pub fn summary(&self) -> String {
        let mut out = format!(
            "Bridge Diagnostics\n\
             Total messages: {}\n\
             Errors:         {}\n\n",
            self.total_messages, self.error_count,
        );

        if self.latencies.is_empty() {
            out.push_str("  No latency data collected.\n");
            return out;
        }

        out.push_str("  Latencies (per message type):\n");
        let mut types: Vec<&String> = self.latencies.keys().collect();
        types.sort();

        for msg_type in types {
            let p50 = self
                .percentile(msg_type, 50)
                .map(|d| format!("{:.1}ms", d.as_secs_f64() * 1000.0))
                .unwrap_or_else(|| "n/a".to_string());
            let p95 = self
                .percentile(msg_type, 95)
                .map(|d| format!("{:.1}ms", d.as_secs_f64() * 1000.0))
                .unwrap_or_else(|| "n/a".to_string());
            let samples = self.latencies[msg_type].len();
            out.push_str(&format!(
                "    {msg_type:<20} p50={p50:<10} p95={p95:<10} n={samples}\n"
            ));
        }
        out
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Calculate the given percentile (0–100) from a sorted sample vector.
    fn percentile(&self, msg_type: &str, pct: usize) -> Option<Duration> {
        let samples = self.latencies.get(msg_type)?;
        if samples.is_empty() {
            return None;
        }
        let idx = ((pct * samples.len()).saturating_sub(1)) / 100;
        Some(samples[idx.min(samples.len() - 1)])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "bridge_debug"))]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_percentiles() {
        let mut diag = BridgeDiagnostics::new();
        for ms in [10u64, 20, 30, 40, 50, 60, 70, 80, 90, 100] {
            diag.record_latency("request", Duration::from_millis(ms));
        }
        // p50 of 10 evenly spaced values → index 4 (50ms)
        assert_eq!(diag.p50("request"), Some(Duration::from_millis(50)));
        // p95 → index 9 (100ms)
        assert_eq!(diag.p95("request"), Some(Duration::from_millis(100)));
    }

    #[test]
    fn test_error_counting() {
        let mut diag = BridgeDiagnostics::new();
        diag.record_error();
        diag.record_error();
        assert_eq!(diag.error_count(), 2);
        assert_eq!(diag.total_messages(), 2);
    }

    #[test]
    fn test_summary_contains_type() {
        let mut diag = BridgeDiagnostics::new();
        diag.record_latency("notify", Duration::from_millis(5));
        let s = diag.summary();
        assert!(s.contains("notify"));
        assert!(s.contains("p50"));
    }
}
