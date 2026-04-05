//! Ping/pong health checks for the MCP bridge connection.
//!
//! Enabled only when the `bridge_debug` feature flag is active.

#[cfg(feature = "bridge_debug")]
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// HealthChecker
// ---------------------------------------------------------------------------

/// Tracks ping/pong health of the MCP bridge connection.
///
/// Considers the bridge unhealthy after 3 consecutive missed pongs.
///
/// Only compiled when the `bridge_debug` feature is enabled.
#[cfg(feature = "bridge_debug")]
pub struct HealthChecker {
    /// Instant of the last received pong (if any).
    last_pong: Option<Instant>,
    /// Number of pings sent without a corresponding pong response.
    missed_pongs: u32,
    /// How frequently health checks should be triggered.
    check_interval: Duration,
    /// Instant of the last `should_check()` return value of `true`.
    last_check: Option<Instant>,
}

#[cfg(feature = "bridge_debug")]
impl HealthChecker {
    /// Create a new checker with the given interval in seconds.
    ///
    /// Pass `0` to use the default of 30 seconds.
    pub fn new(interval_secs: u64) -> Self {
        let secs = if interval_secs == 0 { 30 } else { interval_secs };
        tracing::debug!("HealthChecker created (interval={}s)", secs);
        Self {
            last_pong: None,
            missed_pongs: 0,
            check_interval: Duration::from_secs(secs),
            last_check: None,
        }
    }

    /// Returns `true` if enough time has elapsed since the last check.
    pub fn should_check(&self) -> bool {
        match self.last_check {
            None => true,
            Some(t) => t.elapsed() >= self.check_interval,
        }
    }

    /// Record that a ping was sent (increments the missed-pong counter).
    pub fn record_ping(&mut self) {
        self.missed_pongs += 1;
        self.last_check = Some(Instant::now());
        tracing::debug!(
            "Bridge ping sent (missed_pongs={})",
            self.missed_pongs
        );
    }

    /// Record that a pong was received (resets the missed-pong counter).
    pub fn record_pong(&mut self) {
        self.missed_pongs = 0;
        self.last_pong = Some(Instant::now());
        tracing::debug!("Bridge pong received");
    }

    /// Returns `true` when the bridge is considered healthy (< 3 missed pongs).
    pub fn is_healthy(&self) -> bool {
        self.missed_pongs < 3
    }

    /// Number of pings sent without a pong response.
    pub fn missed_count(&self) -> u32 {
        self.missed_pongs
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "bridge_debug"))]
mod tests {
    use super::*;

    #[test]
    fn test_initial_healthy() {
        let checker = HealthChecker::new(30);
        assert!(checker.is_healthy());
        assert_eq!(checker.missed_count(), 0);
    }

    #[test]
    fn test_unhealthy_after_three_missed() {
        let mut checker = HealthChecker::new(30);
        checker.record_ping();
        checker.record_ping();
        assert!(checker.is_healthy()); // 2 missed — still healthy
        checker.record_ping();
        assert!(!checker.is_healthy()); // 3 missed — unhealthy
    }

    #[test]
    fn test_pong_resets_counter() {
        let mut checker = HealthChecker::new(0); // 0 → default 30s
        checker.record_ping();
        checker.record_ping();
        checker.record_ping();
        assert!(!checker.is_healthy());
        checker.record_pong();
        assert!(checker.is_healthy());
        assert_eq!(checker.missed_count(), 0);
    }
}
