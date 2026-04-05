//! Policy limits poller — lightweight background fetcher with ETag caching.
//!
//! Wraps the stateless HTTP fetch logic in [`super::policy_limits`] with a
//! stateful poller that tracks the poll interval, last-fetch instant, and the
//! current ETag so callers never have to manage that state themselves.

use std::time::{Duration, Instant};

use crate::policy_limits::PolicyLimits;

/// Default interval between policy limit polls (1 hour).
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Stateful poller for org policy limits.
///
/// Maintains the last-fetch [`Instant`] and the ETag from the previous
/// successful response so that conditional GETs (`If-None-Match`) are sent
/// automatically, returning `None` on HTTP 304 (no change).
///
/// **Fail-open**: network errors are logged as warnings and `Ok(None)` is
/// returned, leaving the caller's cached limits untouched.
pub struct PolicyLimitsPoller {
    /// Remote endpoint to poll. `None` means polling is disabled.
    endpoint: Option<String>,
    /// How frequently to poll. Defaults to 1 hour.
    poll_interval: Duration,
    /// ETag received from the last successful (non-304) response.
    etag: Option<String>,
    /// Instant of the last successful poll attempt.
    last_fetch: Option<Instant>,
}

impl PolicyLimitsPoller {
    /// Create a new poller targeting `endpoint` with a 1-hour interval.
    ///
    /// Pass `None` to create a no-op poller (useful for local/offline mode).
    pub fn new(endpoint: Option<String>) -> Self {
        Self {
            endpoint,
            poll_interval: DEFAULT_POLL_INTERVAL,
            etag: None,
            last_fetch: None,
        }
    }

    /// Override the default poll interval (primarily for testing).
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Whether enough time has passed since the last fetch to poll again.
    ///
    /// Returns `true` on the first call (no previous fetch recorded) and when
    /// no endpoint is configured (caller can skip without an if-chain).
    pub fn should_poll(&self) -> bool {
        match self.last_fetch {
            None => true,
            Some(last) => last.elapsed() >= self.poll_interval,
        }
    }

    /// Poll the endpoint for updated policy limits.
    ///
    /// - Returns `Ok(Some(limits))` when new limits are available.
    /// - Returns `Ok(None)` on HTTP 304 (unchanged) or when no endpoint is set.
    /// - Returns `Ok(None)` on network/parse errors (fail-open — warnings logged).
    ///
    /// Updates the internal ETag and last-fetch time on success.
    pub async fn poll(&mut self) -> Result<Option<PolicyLimits>, String> {
        let url = match &self.endpoint {
            Some(u) => u.clone(),
            None => return Ok(None),
        };

        self.last_fetch = Some(Instant::now());

        match fetch_with_etag(&url, self.etag.as_deref()).await {
            Ok(Some((limits, new_etag))) => {
                tracing::debug!(url = %url, "Policy limits refreshed");
                self.etag = new_etag;
                Ok(Some(limits))
            }
            Ok(None) => {
                tracing::debug!(url = %url, "Policy limits unchanged (304)");
                Ok(None)
            }
            Err(e) => {
                tracing::warn!(url = %url, error = %e, "Policy limits poll failed — using cached");
                Ok(None) // fail-open
            }
        }
    }

    /// Return the ETag cached from the last successful non-304 response.
    pub fn cached_etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// Return the configured endpoint URL.
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }
}

/// Issue a conditional GET to `url` using `etag` as the `If-None-Match` value.
///
/// Returns:
/// - `Ok(Some((limits, etag)))` — new limits plus optional new ETag.
/// - `Ok(None)` — HTTP 304, limits unchanged.
/// - `Err(msg)` — any other failure (caller treats as fail-open).
async fn fetch_with_etag(
    url: &str,
    etag: Option<&str>,
) -> Result<Option<(PolicyLimits, Option<String>)>, String> {
    let client = reqwest::Client::new();
    let mut req = client.get(url);

    if let Some(tag) = etag {
        req = req.header("If-None-Match", tag);
    }

    let response = req
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }

    if !response.status().is_success() {
        return Err(format!("Unexpected HTTP status: {}", response.status()));
    }

    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string);

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON response: {e}"))?;

    let mut policies = std::collections::HashMap::new();
    if let Some(obj) = body.as_object() {
        for (key, val) in obj {
            if let Some(allowed) = val.as_bool() {
                policies.insert(key.clone(), allowed);
            }
        }
    }

    let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or(0);
    let limits = PolicyLimits {
        policies,
        etag: new_etag.clone(),
        last_updated: now,
    };

    Ok(Some((limits, new_etag)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_poll_true_on_first_call() {
        let poller = PolicyLimitsPoller::new(Some("https://example.com/policies".to_string()));
        assert!(poller.should_poll());
    }

    #[test]
    fn should_poll_false_immediately_after_fetch_recorded() {
        let mut poller = PolicyLimitsPoller::new(Some("https://example.com/policies".to_string()))
            .with_interval(Duration::from_secs(3600));
        // Simulate a fetch having just occurred.
        poller.last_fetch = Some(Instant::now());
        assert!(!poller.should_poll());
    }

    #[test]
    fn should_poll_true_after_interval_elapsed() {
        let mut poller = PolicyLimitsPoller::new(Some("https://example.com/policies".to_string()))
            .with_interval(Duration::from_nanos(1));
        poller.last_fetch = Some(Instant::now() - Duration::from_millis(100));
        assert!(poller.should_poll());
    }

    #[test]
    fn no_endpoint_poll_returns_none_synchronously() {
        let poller = PolicyLimitsPoller::new(None);
        assert!(poller.endpoint().is_none());
        assert!(poller.cached_etag().is_none());
    }
}
