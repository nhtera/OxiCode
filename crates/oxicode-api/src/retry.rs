use std::time::Duration;

use oxicode_common::RateLimitInfo;

/// Retry policy with exponential backoff and jitter.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(120),
        }
    }
}

impl RetryPolicy {
    /// Calculate delay for the given retry attempt (1-indexed), without jitter.
    pub fn base_delay_for(&self, attempt: u32) -> Duration {
        let exp_delay = self.base_delay * 2u32.saturating_pow(attempt.saturating_sub(1));
        exp_delay.min(self.max_delay)
    }

    /// Calculate delay with jitter for the given retry attempt (1-indexed).
    /// Uses "decorrelated jitter": delay * (0.5 + random * 0.5).
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let base = self.base_delay_for(attempt);
        let jitter_factor = 0.5 + (cheap_random() * 0.5);
        let jittered = base.as_secs_f64() * jitter_factor;
        Duration::from_secs_f64(jittered).min(self.max_delay)
    }

    /// Calculate delay respecting a rate limit's `retry-after` header.
    /// Returns `max(retry_after, exponential_with_jitter(attempt))`.
    pub fn delay_for_rate_limit(&self, attempt: u32, info: &RateLimitInfo) -> Duration {
        let backoff = self.delay_for(attempt);
        let retry_after = info
            .retry_after_secs
            .map_or(Duration::ZERO, Duration::from_secs_f64);
        backoff.max(retry_after).min(self.max_delay)
    }

    /// Whether the given attempt number is within the retry budget.
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt <= self.max_retries
    }
}

/// Cheap pseudo-random number in [0.0, 1.0) using system time nanoseconds.
/// Not cryptographically secure — fine for jitter.
fn cheap_random() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(Duration::from_nanos(42))
        .subsec_nanos();
    // Use lower bits XORed with upper bits for better distribution.
    let mixed = nanos ^ (nanos >> 16);
    f64::from(mixed % 10000) / 10000.0
}

/// Format a human-readable message for rate limit retry notification.
pub fn format_rate_limit_message(attempt: u32, max_retries: u32, delay: Duration) -> String {
    format!(
        "Rate limited. Retrying in {:.0}s... ({}/{})",
        delay.as_secs_f64(),
        attempt,
        max_retries,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_backoff_base() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.base_delay_for(1), Duration::from_secs(1));
        assert_eq!(policy.base_delay_for(2), Duration::from_secs(2));
        assert_eq!(policy.base_delay_for(3), Duration::from_secs(4));
    }

    #[test]
    fn test_max_delay_cap() {
        let policy = RetryPolicy {
            max_retries: 10,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
        };
        assert_eq!(policy.base_delay_for(10), Duration::from_secs(5));
    }

    #[test]
    fn test_delay_with_jitter_within_range() {
        let policy = RetryPolicy::default();
        for attempt in 1..=3 {
            let delay = policy.delay_for(attempt);
            let base = policy.base_delay_for(attempt);
            // Jitter should be in [base * 0.5, base * 1.0], capped at max_delay.
            let min = Duration::from_secs_f64(base.as_secs_f64() * 0.5);
            assert!(delay >= min, "attempt {attempt}: {delay:?} < {min:?}");
            assert!(delay <= policy.max_delay);
        }
    }

    #[test]
    fn test_delay_for_rate_limit_respects_retry_after() {
        let policy = RetryPolicy::default();
        let info = RateLimitInfo {
            retry_after_secs: Some(30.0),
            ..Default::default()
        };
        let delay = policy.delay_for_rate_limit(1, &info);
        // retry_after=30s > backoff for attempt 1 (~0.5-1s), so delay >= 30s.
        assert!(delay.as_secs() >= 30);
    }

    #[test]
    fn test_delay_for_rate_limit_capped_at_max() {
        let policy = RetryPolicy {
            max_delay: Duration::from_secs(60),
            ..RetryPolicy::default()
        };
        let info = RateLimitInfo {
            retry_after_secs: Some(300.0),
            ..Default::default()
        };
        let delay = policy.delay_for_rate_limit(1, &info);
        assert_eq!(delay, Duration::from_secs(60));
    }

    #[test]
    fn test_should_retry() {
        let policy = RetryPolicy::default();
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(3));
        assert!(!policy.should_retry(4));
    }

    #[test]
    fn test_format_rate_limit_message() {
        let msg = format_rate_limit_message(1, 3, Duration::from_secs(5));
        assert_eq!(msg, "Rate limited. Retrying in 5s... (1/3)");
    }

    #[test]
    fn test_cheap_random_in_range() {
        for _ in 0..100 {
            let r = cheap_random();
            assert!((0.0..1.0).contains(&r), "random out of range: {r}");
        }
    }
}
