//! WebSocket reconnection manager with exponential backoff + jitter.
//!
//! Backoff schedule: 1s → 2s → 4s → 8s → 16s → 30s (capped).
//! Jitter: ±10% random to avoid thundering herd.
//! Max attempts: configurable (default 100), then give up and notify user.

use std::time::Duration;

use rand::Rng;

/// Default values.
const DEFAULT_BASE_DELAY_MS: u64 = 1000;
const DEFAULT_MAX_DELAY_SECS: u64 = 30;
const DEFAULT_MAX_ATTEMPTS: u32 = 100;
const JITTER_PERCENT: f64 = 0.10;

/// Reconnection state and configuration.
pub struct ReconnectionManager {
    /// Current consecutive attempt number (0 = not reconnecting).
    attempt: u32,
    /// Base delay for first attempt.
    base_delay: Duration,
    /// Maximum backoff delay.
    max_delay: Duration,
    /// Maximum total attempts before giving up.
    max_attempts: u32,
    /// Whether the manager has given up.
    gave_up: bool,
}

/// Result of a reconnection attempt decision.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconnectAction {
    /// Wait this duration, then attempt reconnection.
    RetryAfter(Duration),
    /// Maximum attempts exceeded — give up.
    GiveUp { attempts: u32 },
}

impl ReconnectionManager {
    /// Create with custom limits.
    pub fn new(max_delay_secs: u64, max_attempts: u32) -> Self {
        Self {
            attempt: 0,
            base_delay: Duration::from_millis(DEFAULT_BASE_DELAY_MS),
            max_delay: Duration::from_secs(max_delay_secs),
            max_attempts,
            gave_up: false,
        }
    }

    /// Create with default settings (30s max, 100 attempts).
    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_MAX_DELAY_SECS, DEFAULT_MAX_ATTEMPTS)
    }

    /// Call on disconnect — returns the action to take.
    ///
    /// Each call increments the attempt counter and computes the next delay.
    pub fn on_disconnect(&mut self) -> ReconnectAction {
        if self.gave_up {
            return ReconnectAction::GiveUp {
                attempts: self.attempt,
            };
        }

        self.attempt += 1;

        if self.attempt > self.max_attempts {
            self.gave_up = true;
            return ReconnectAction::GiveUp {
                attempts: self.attempt - 1,
            };
        }

        let delay = self.compute_delay();
        ReconnectAction::RetryAfter(delay)
    }

    /// Call on successful connection — resets all state.
    pub fn on_connected(&mut self) {
        self.attempt = 0;
        self.gave_up = false;
    }

    /// Current attempt number.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Whether the manager has given up reconnecting.
    pub fn has_given_up(&self) -> bool {
        self.gave_up
    }

    /// Reset state (e.g., when user manually triggers reconnect).
    pub fn reset(&mut self) {
        self.attempt = 0;
        self.gave_up = false;
    }

    /// Compute delay with exponential backoff + jitter.
    fn compute_delay(&self) -> Duration {
        // Exponential: base * 2^(attempt-1), capped at max_delay.
        let exp = (self.attempt - 1).min(20); // cap exponent to avoid overflow
        let base_ms = self.base_delay.as_millis() as u64;
        let delay_ms = base_ms.saturating_mul(1u64 << exp);
        let delay = Duration::from_millis(delay_ms).min(self.max_delay);

        // Apply ±10% jitter.
        apply_jitter(delay)
    }
}

/// Apply ±`JITTER_PERCENT` random jitter to a duration.
#[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn apply_jitter(delay: Duration) -> Duration {
    let ms = delay.as_millis() as f64;
    let jitter_range = ms * JITTER_PERCENT;
    let mut rng = rand::rng();
    let jitter = rng.random_range(-jitter_range..=jitter_range);
    let jittered_ms = (ms + jitter).max(1.0) as u64;
    Duration::from_millis(jittered_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_attempt_delay_near_1s() {
        let mut mgr = ReconnectionManager::with_defaults();
        let action = mgr.on_disconnect();
        match action {
            ReconnectAction::RetryAfter(d) => {
                // 1s ± 10% → 900ms..1100ms.
                assert!(d.as_millis() >= 900, "delay too low: {d:?}");
                assert!(d.as_millis() <= 1100, "delay too high: {d:?}");
            }
            ReconnectAction::GiveUp { .. } => panic!("Expected RetryAfter"),
        }
    }

    #[test]
    fn test_exponential_growth() {
        let mut mgr = ReconnectionManager::new(60, 100);
        let delays: Vec<Duration> = (0..6)
            .map(|_| match mgr.on_disconnect() {
                ReconnectAction::RetryAfter(d) => d,
                ReconnectAction::GiveUp { .. } => panic!("Expected RetryAfter"),
            })
            .collect();

        // Each delay should be roughly double the previous (within jitter).
        for i in 1..delays.len() {
            #[allow(clippy::cast_precision_loss)]
            let ratio = delays[i].as_millis() as f64 / delays[i - 1].as_millis() as f64;
            // Allow wide range due to jitter: 1.5..2.8.
            assert!(
                (1.3..3.0).contains(&ratio),
                "Unexpected ratio {ratio} between attempt {} and {}",
                i,
                i + 1
            );
        }
    }

    #[test]
    fn test_delay_capped_at_max() {
        let mut mgr = ReconnectionManager::new(30, 100);
        // Run 20 attempts — should cap at 30s.
        for _ in 0..20 {
            mgr.on_disconnect();
        }
        let action = mgr.on_disconnect();
        match action {
            ReconnectAction::RetryAfter(d) => {
                // 30s ± 10% → max ~33s.
                assert!(d.as_secs() <= 34, "delay exceeds cap: {d:?}");
            }
            ReconnectAction::GiveUp { .. } => panic!("Expected RetryAfter"),
        }
    }

    #[test]
    fn test_max_attempts_gives_up() {
        let mut mgr = ReconnectionManager::new(1, 3);
        assert!(matches!(
            mgr.on_disconnect(),
            ReconnectAction::RetryAfter(_)
        ));
        assert!(matches!(
            mgr.on_disconnect(),
            ReconnectAction::RetryAfter(_)
        ));
        assert!(matches!(
            mgr.on_disconnect(),
            ReconnectAction::RetryAfter(_)
        ));
        // 4th attempt exceeds max=3.
        assert!(matches!(
            mgr.on_disconnect(),
            ReconnectAction::GiveUp { attempts: 3 }
        ));
        assert!(mgr.has_given_up());
    }

    #[test]
    fn test_connected_resets() {
        let mut mgr = ReconnectionManager::with_defaults();
        mgr.on_disconnect();
        mgr.on_disconnect();
        assert_eq!(mgr.attempt(), 2);

        mgr.on_connected();
        assert_eq!(mgr.attempt(), 0);
        assert!(!mgr.has_given_up());
    }

    #[test]
    fn test_reset_after_give_up() {
        let mut mgr = ReconnectionManager::new(1, 1);
        mgr.on_disconnect(); // attempt 1, OK
        mgr.on_disconnect(); // attempt 2, give up
        assert!(mgr.has_given_up());

        mgr.reset();
        assert!(!mgr.has_given_up());
        assert_eq!(mgr.attempt(), 0);
        // Can try again.
        assert!(matches!(
            mgr.on_disconnect(),
            ReconnectAction::RetryAfter(_)
        ));
    }

    #[test]
    fn test_give_up_stays_given_up() {
        let mut mgr = ReconnectionManager::new(1, 1);
        mgr.on_disconnect();
        mgr.on_disconnect(); // give up
                             // Subsequent calls also return GiveUp.
        assert!(matches!(
            mgr.on_disconnect(),
            ReconnectAction::GiveUp { .. }
        ));
    }

    #[test]
    fn test_jitter_varies() {
        // Run apply_jitter many times — values should not all be identical.
        let base = Duration::from_secs(10);
        let values: Vec<u64> = (0..20)
            .map(|_| apply_jitter(base).as_millis() as u64)
            .collect();
        let unique: std::collections::HashSet<_> = values.iter().collect();
        // With ±10% jitter on 10s, expect at least some variation.
        assert!(unique.len() > 1, "Jitter should produce varied values");
    }
}
