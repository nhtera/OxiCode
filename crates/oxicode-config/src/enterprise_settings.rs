//! Enterprise managed settings fetched from a remote admin endpoint.
//!
//! Distinct from `mdm.rs` (platform MDM: plist/registry/TOML). This module
//! fetches settings from a custom HTTP endpoint used by enterprise admins.
//! Settings are validated with HMAC-SHA256 signature and cached with staleness check.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::mdm::{ManagedSetting, ManagedSettings};

/// Default cache staleness: 1 hour.
const DEFAULT_CACHE_TTL_SECS: i64 = 3600;

/// Enterprise settings response from the admin endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterpriseSettingsResponse {
    /// Settings key-value pairs.
    pub settings: HashMap<String, String>,
    /// Which keys are locked (cannot be overridden by user).
    #[serde(default)]
    pub locked: HashMap<String, bool>,
    /// HMAC-SHA256 signature of the settings payload (hex-encoded).
    #[serde(default)]
    pub signature: String,
    /// Timestamp of the settings version.
    #[serde(default)]
    pub version_ts: Option<String>,
}

/// Cached enterprise settings stored on disk.
#[derive(Debug, Serialize, Deserialize)]
struct CachedEnterprise {
    fetched_at: DateTime<Utc>,
    response: EnterpriseSettingsResponse,
}

/// Client for fetching enterprise managed settings from a remote endpoint.
pub struct EnterpriseSettingsClient {
    /// Admin endpoint URL.
    endpoint: String,
    /// HMAC signing key (shared secret for signature validation).
    signing_key: Option<String>,
    /// Cache directory.
    cache_dir: PathBuf,
    /// Cache TTL in seconds.
    cache_ttl_secs: i64,
}

impl EnterpriseSettingsClient {
    /// Create a new enterprise settings client.
    pub fn new(endpoint: &str, signing_key: Option<&str>, cache_dir: &Path) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            signing_key: signing_key.map(String::from),
            cache_dir: cache_dir.to_path_buf(),
            cache_ttl_secs: DEFAULT_CACHE_TTL_SECS,
        }
    }

    /// Fetch enterprise settings (from cache if fresh, otherwise remote).
    /// Returns `ManagedSettings` compatible with the existing MDM system.
    pub async fn fetch_managed(&self) -> Result<ManagedSettings, String> {
        // Try cached settings first.
        if let Some(cached) = self.load_cache() {
            let age = Utc::now()
                .signed_duration_since(cached.fetched_at)
                .num_seconds();
            if age < self.cache_ttl_secs {
                tracing::debug!("Using cached enterprise settings (age: {age}s)");
                return Ok(self.to_managed_settings(&cached.response));
            }
        }

        // Fetch from remote endpoint.
        let response = self.fetch_remote().await?;

        // SECURITY: Always validate signature. Reject unsigned responses.
        // If no signing key is configured, enterprise settings are rejected
        // to prevent accepting tampered/unsigned payloads.
        self.validate_signature(&response)?;

        // Cache the response (best-effort).
        if let Err(e) = self.save_cache(&response) {
            tracing::warn!("Failed to cache enterprise settings: {e}");
        }

        Ok(self.to_managed_settings(&response))
    }

    /// Fetch from the remote admin endpoint.
    async fn fetch_remote(&self) -> Result<EnterpriseSettingsResponse, String> {
        tracing::info!("Fetching enterprise settings from {}", self.endpoint);

        let client = reqwest::Client::new();
        let resp = client
            .get(&self.endpoint)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("Enterprise settings fetch failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Enterprise settings endpoint returned HTTP {}",
                resp.status()
            ));
        }

        resp.json::<EnterpriseSettingsResponse>()
            .await
            .map_err(|e| format!("Invalid enterprise settings response: {e}"))
    }

    /// Validate HMAC-SHA256 signature of the settings payload.
    fn validate_signature(&self, response: &EnterpriseSettingsResponse) -> Result<(), String> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let key = self.signing_key.as_deref().unwrap_or("");
        if key.is_empty() {
            return Err("Signing key required for enterprise settings validation".into());
        }

        if response.signature.is_empty() {
            return Err("Enterprise settings response missing signature".into());
        }

        // Build canonical payload: sorted key=value pairs joined by newlines.
        let mut pairs: Vec<String> = response
            .settings
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        pairs.sort();
        let payload = pairs.join("\n");

        let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
            .map_err(|e| format!("HMAC key error: {e}"))?;
        mac.update(payload.as_bytes());

        let expected_sig = hex::decode(&response.signature)
            .map_err(|e| format!("Invalid signature hex: {e}"))?;

        mac.verify_slice(&expected_sig)
            .map_err(|_| "Enterprise settings signature verification failed".to_string())
    }

    /// Convert enterprise response to ManagedSettings for merge.
    fn to_managed_settings(&self, response: &EnterpriseSettingsResponse) -> ManagedSettings {
        let mut settings = HashMap::new();

        for (key, value) in &response.settings {
            let locked = response
                .locked
                .get(key)
                .copied()
                .unwrap_or(true); // Default: locked.

            settings.insert(
                key.clone(),
                ManagedSetting {
                    key: key.clone(),
                    value: value.clone(),
                    locked,
                },
            );
        }

        ManagedSettings {
            settings,
            source: Some(format!("enterprise endpoint: {}", self.endpoint)),
        }
    }

    /// Load cached enterprise settings from disk.
    fn load_cache(&self) -> Option<CachedEnterprise> {
        let path = self.cache_path();
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save enterprise settings to disk cache.
    fn save_cache(&self, response: &EnterpriseSettingsResponse) -> Result<(), String> {
        std::fs::create_dir_all(&self.cache_dir)
            .map_err(|e| format!("Cache dir creation failed: {e}"))?;
        let cached = CachedEnterprise {
            fetched_at: Utc::now(),
            response: response.clone(),
        };
        let json = serde_json::to_string_pretty(&cached)
            .map_err(|e| format!("Cache serialization failed: {e}"))?;
        std::fs::write(self.cache_path(), json)
            .map_err(|e| format!("Cache write failed: {e}"))
    }

    /// Path to the cache file.
    fn cache_path(&self) -> PathBuf {
        self.cache_dir.join("enterprise-settings-cache.json")
    }
}

/// Load enterprise managed settings from env-configured endpoint.
/// Returns None if no endpoint is configured.
pub fn load_enterprise_settings_from_env() -> Option<ManagedSettings> {
    let endpoint = std::env::var("OXICODE_ENTERPRISE_SETTINGS_URL").ok()?;
    if endpoint.is_empty() {
        return None;
    }

    let signing_key = std::env::var("OXICODE_ENTERPRISE_SIGNING_KEY").ok();
    let cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".oxicode")
        .join("cache");

    let client = EnterpriseSettingsClient::new(
        &endpoint,
        signing_key.as_deref(),
        &cache_dir,
    );

    // Block on async fetch — only called during startup.
    let rt = tokio::runtime::Handle::try_current();
    match rt {
        Ok(handle) => {
            // We're already in an async context, use block_in_place.
            match tokio::task::block_in_place(|| handle.block_on(client.fetch_managed())) {
                Ok(settings) => Some(settings),
                Err(e) => {
                    tracing::warn!("Failed to load enterprise settings: {e}");
                    None
                }
            }
        }
        Err(_) => {
            tracing::debug!("No tokio runtime for enterprise settings fetch");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_managed_settings() {
        let response = EnterpriseSettingsResponse {
            settings: HashMap::from([
                ("model".into(), "claude-opus-4-20250514".into()),
                ("permission_mode".into(), "default".into()),
            ]),
            locked: HashMap::from([
                ("model".into(), true),
                ("permission_mode".into(), false),
            ]),
            signature: String::new(),
            version_ts: None,
        };

        let cache_dir = tempfile::tempdir().unwrap();
        let client = EnterpriseSettingsClient::new(
            "https://example.com/settings",
            None,
            cache_dir.path(),
        );

        let managed = client.to_managed_settings(&response);
        assert_eq!(managed.get("model"), Some("claude-opus-4-20250514"));
        assert!(managed.is_locked("model"));
        assert!(!managed.is_locked("permission_mode"));
        assert!(managed.source.as_ref().unwrap().contains("enterprise"));
    }

    #[test]
    fn test_validate_signature_valid() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let key = "test-secret-key";
        let settings = HashMap::from([
            ("model".into(), "claude-opus-4-20250514".into()),
            ("theme".into(), "dark".into()),
        ]);

        // Generate valid signature.
        let mut pairs: Vec<String> = settings
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        pairs.sort();
        let payload = pairs.join("\n");

        let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
        mac.update(payload.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());

        let response = EnterpriseSettingsResponse {
            settings,
            locked: HashMap::new(),
            signature: sig,
            version_ts: None,
        };

        let cache_dir = tempfile::tempdir().unwrap();
        let client = EnterpriseSettingsClient::new(
            "https://example.com",
            Some(key),
            cache_dir.path(),
        );

        assert!(client.validate_signature(&response).is_ok());
    }

    #[test]
    fn test_validate_signature_invalid() {
        let response = EnterpriseSettingsResponse {
            settings: HashMap::from([("model".into(), "opus".into())]),
            locked: HashMap::new(),
            signature: "deadbeef".into(),
            version_ts: None,
        };

        let cache_dir = tempfile::tempdir().unwrap();
        let client = EnterpriseSettingsClient::new(
            "https://example.com",
            Some("secret"),
            cache_dir.path(),
        );

        assert!(client.validate_signature(&response).is_err());
    }

    #[test]
    fn test_validate_signature_missing() {
        let response = EnterpriseSettingsResponse {
            settings: HashMap::new(),
            locked: HashMap::new(),
            signature: String::new(),
            version_ts: None,
        };

        let cache_dir = tempfile::tempdir().unwrap();
        let client = EnterpriseSettingsClient::new(
            "https://example.com",
            Some("secret"),
            cache_dir.path(),
        );

        assert!(client.validate_signature(&response).is_err());
    }

    #[test]
    fn test_cache_roundtrip() {
        let cache_dir = tempfile::tempdir().unwrap();
        let client = EnterpriseSettingsClient::new(
            "https://example.com",
            None,
            cache_dir.path(),
        );

        let response = EnterpriseSettingsResponse {
            settings: HashMap::from([("model".into(), "opus".into())]),
            locked: HashMap::from([("model".into(), true)]),
            signature: String::new(),
            version_ts: None,
        };

        client.save_cache(&response).unwrap();
        let cached = client.load_cache().unwrap();
        assert_eq!(cached.response.settings.get("model").unwrap(), "opus");
    }

    #[test]
    fn test_enterprise_default_locked() {
        let response = EnterpriseSettingsResponse {
            settings: HashMap::from([("model".into(), "opus".into())]),
            locked: HashMap::new(), // No explicit lock — should default to true.
            signature: String::new(),
            version_ts: None,
        };

        let cache_dir = tempfile::tempdir().unwrap();
        let client = EnterpriseSettingsClient::new(
            "https://example.com",
            None,
            cache_dir.path(),
        );

        let managed = client.to_managed_settings(&response);
        assert!(managed.is_locked("model")); // Default: locked.
    }
}
