//! Org policy limits with ETag-based HTTP caching and fail-open design.
//!
//! Fetches organizational restrictions from a configurable endpoint,
//! caches responses using ETag headers + local file, and polls in the
//! background every hour. If the service is unreachable, all policies
//! are allowed (fail-open).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// Cached policy limits with ETag for conditional requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyLimits {
    /// Map of policy name → allowed (true = allowed, absent = allowed).
    pub policies: HashMap<String, bool>,
    /// ETag from last successful fetch (for If-None-Match).
    #[serde(default)]
    pub etag: Option<String>,
    /// When this cache was last updated (epoch secs).
    #[serde(default)]
    pub last_updated: u64,
}

impl PolicyLimits {
    /// Check if a policy is allowed. Fail-open: missing policies → allowed.
    pub fn is_allowed(&self, policy: &str) -> bool {
        self.policies.get(policy).copied().unwrap_or(true)
    }
}

/// Known policy names.
pub mod policy {
    pub const ALLOW_WEB_SEARCH: &str = "allow_web_search";
    pub const ALLOW_TOOL_EXECUTION: &str = "allow_tool_execution";
    pub const ALLOW_MODEL_SELECTION: &str = "allow_model_selection";
    pub const ALLOW_MCP: &str = "allow_mcp";
    pub const ALLOW_COMPUTER_USE: &str = "allow_computer_use";
}

/// Client for fetching and caching org policy limits.
///
/// Thread-safe via `RwLock`. Call `fetch()` periodically from a
/// background tokio task to keep limits up-to-date.
pub struct PolicyLimitsClient {
    /// Cached limits (shared across threads).
    limits: Arc<RwLock<PolicyLimits>>,
    /// File path for persistent cache.
    cache_path: PathBuf,
    /// API endpoint URL (None = disabled).
    endpoint: Option<String>,
}

impl PolicyLimitsClient {
    /// Create a new client. Loads cached limits from disk if available.
    pub fn new(endpoint: Option<String>) -> Self {
        let cache_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".oxicode/policy-limits.json");

        let cached = load_from_disk(&cache_path).unwrap_or_default();

        Self {
            limits: Arc::new(RwLock::new(cached)),
            cache_path,
            endpoint,
        }
    }

    /// Check if a specific policy is allowed. Fail-open on any error.
    pub fn is_allowed(&self, policy_name: &str) -> bool {
        // Fail-open on lock poison.
        self.limits
            .read()
            .map_or(true, |l| l.is_allowed(policy_name))
    }

    /// Get a snapshot of all current policies.
    pub fn snapshot(&self) -> PolicyLimits {
        self.limits.read().map(|l| l.clone()).unwrap_or_default()
    }

    /// Update cached limits (called after successful fetch).
    pub fn update(&self, new_limits: PolicyLimits) {
        // Write to disk first
        if let Err(e) = save_to_disk(&self.cache_path, &new_limits) {
            tracing::warn!("Failed to persist policy limits: {}", e);
        }
        // Update in-memory
        if let Ok(mut guard) = self.limits.write() {
            *guard = new_limits;
        }
    }

    /// Fetch latest policy limits from the API endpoint.
    ///
    /// Uses ETag for conditional GET (304 = no change). Returns `None`
    /// if no endpoint configured, fetch failed, or no change.
    pub async fn fetch(&self) -> Option<PolicyLimits> {
        let url = self.endpoint.as_ref()?;
        let current_etag = self.limits.read().ok().and_then(|l| l.etag.clone());

        match fetch_policy_limits(url, current_etag.as_deref()).await {
            Ok(Some(limits)) => {
                tracing::info!("Policy limits updated from {}", url);
                self.update(limits.clone());
                Some(limits)
            }
            Ok(None) => {
                tracing::debug!("Policy limits unchanged (304)");
                None
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch policy limits: {} — using cached/fail-open",
                    e
                );
                None
            }
        }
    }

    /// Get the endpoint URL (for diagnostics).
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Get a thread-safe handle for sharing across async tasks.
    pub fn limits_handle(&self) -> Arc<RwLock<PolicyLimits>> {
        Arc::clone(&self.limits)
    }
}

/// Fetch policy limits from an HTTP endpoint with optional ETag.
async fn fetch_policy_limits(
    url: &str,
    etag: Option<&str>,
) -> Result<Option<PolicyLimits>, String> {
    let client = reqwest::Client::new();
    let mut request = client.get(url);

    if let Some(tag) = etag {
        request = request.header("If-None-Match", tag);
    }

    let response = request
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string);

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))?;

    // Parse response into policies map
    let mut policies = HashMap::new();
    if let Some(obj) = body.as_object() {
        for (key, val) in obj {
            if let Some(allowed) = val.as_bool() {
                policies.insert(key.clone(), allowed);
            }
        }
    }

    let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or(0);
    Ok(Some(PolicyLimits {
        policies,
        etag: new_etag,
        last_updated: now,
    }))
}

/// Load cached policy limits from disk.
fn load_from_disk(path: &std::path::Path) -> Option<PolicyLimits> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Save policy limits to disk.
fn save_to_disk(path: &std::path::Path, limits: &PolicyLimits) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(limits).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_open_missing_policy() {
        let limits = PolicyLimits::default();
        assert!(limits.is_allowed("nonexistent_policy"));
    }

    #[test]
    fn explicit_deny() {
        let mut policies = HashMap::new();
        policies.insert("allow_web_search".to_string(), false);
        let limits = PolicyLimits {
            policies,
            ..Default::default()
        };
        assert!(!limits.is_allowed("allow_web_search"));
    }

    #[test]
    fn explicit_allow() {
        let mut policies = HashMap::new();
        policies.insert("allow_tool_execution".to_string(), true);
        let limits = PolicyLimits {
            policies,
            ..Default::default()
        };
        assert!(limits.is_allowed("allow_tool_execution"));
    }

    #[test]
    fn client_fail_open_no_endpoint() {
        let client = PolicyLimitsClient::new(None);
        assert!(client.is_allowed(policy::ALLOW_WEB_SEARCH));
        assert!(client.is_allowed(policy::ALLOW_TOOL_EXECUTION));
        assert!(client.is_allowed(policy::ALLOW_MODEL_SELECTION));
    }

    #[test]
    fn client_update_and_read() {
        let client = PolicyLimitsClient::new(None);
        let mut policies = HashMap::new();
        policies.insert(policy::ALLOW_MCP.to_string(), false);

        client.update(PolicyLimits {
            policies,
            etag: Some("W/\"abc123\"".to_string()),
            last_updated: 1_234_567_890,
        });

        assert!(!client.is_allowed(policy::ALLOW_MCP));
        assert!(client.is_allowed(policy::ALLOW_WEB_SEARCH)); // Missing → allowed
    }

    #[test]
    fn snapshot_returns_current() {
        let client = PolicyLimitsClient::new(None);
        let snap = client.snapshot();
        // After construction, snapshot should reflect whatever is on disk (or empty).
        // The key invariant: is_allowed returns true for missing policies.
        assert!(snap.is_allowed("nonexistent_policy_xyz"));
    }

    #[test]
    fn load_save_roundtrip() {
        let dir = std::env::temp_dir().join(format!("oxi-policy-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("policy-limits.json");

        let mut policies = HashMap::new();
        policies.insert("allow_web_search".to_string(), false);
        policies.insert("allow_tool_execution".to_string(), true);

        let limits = PolicyLimits {
            policies,
            etag: Some("W/\"test\"".to_string()),
            last_updated: 9999,
        };

        save_to_disk(&path, &limits).expect("save should succeed");
        let loaded = load_from_disk(&path).expect("load should succeed");

        assert!(!loaded.is_allowed("allow_web_search"));
        assert!(loaded.is_allowed("allow_tool_execution"));
        assert_eq!(loaded.etag, Some("W/\"test\"".to_string()));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn policy_constants_valid() {
        // Sanity check that policy constants are non-empty strings
        assert!(!policy::ALLOW_WEB_SEARCH.is_empty());
        assert!(!policy::ALLOW_TOOL_EXECUTION.is_empty());
        assert!(!policy::ALLOW_MODEL_SELECTION.is_empty());
        assert!(!policy::ALLOW_MCP.is_empty());
        assert!(!policy::ALLOW_COMPUTER_USE.is_empty());
    }
}
