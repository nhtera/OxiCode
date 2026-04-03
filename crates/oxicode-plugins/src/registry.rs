//! Remote plugin registry client.
//!
//! Fetches a JSON index from a remote URL (e.g. GitHub-hosted index file),
//! caches it locally with a configurable TTL, and provides search/filter APIs.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use oxicode_common::{OxiError, OxiResult};
use serde::{Deserialize, Serialize};

/// Default cache TTL: 1 hour.
const DEFAULT_CACHE_TTL_SECS: i64 = 3600;

/// A single plugin entry from the remote registry index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// Download URL for the plugin archive (tar.gz or zip).
    #[serde(default)]
    pub download_url: String,
    /// Minimum compatible OxiCode version (semver range).
    #[serde(default)]
    pub min_oxicode_version: String,
    /// Keywords for search.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Trust metadata: "verified", "community", or "unverified".
    #[serde(default = "default_trust")]
    pub trust: String,
    /// Permissions the plugin requests (tool names, hook events).
    #[serde(default)]
    pub permissions: Vec<String>,
}

fn default_trust() -> String {
    "unverified".to_string()
}

/// Cached index stored on disk.
#[derive(Debug, Serialize, Deserialize)]
struct CachedIndex {
    fetched_at: DateTime<Utc>,
    entries: Vec<PluginEntry>,
}

/// Remote plugin registry client with disk caching.
pub struct PluginRegistry {
    /// URL of the JSON index file.
    index_url: String,
    /// Local cache directory.
    cache_dir: PathBuf,
    /// Cache TTL in seconds.
    cache_ttl_secs: i64,
}

impl PluginRegistry {
    /// Create a new registry client.
    pub fn new(index_url: &str, cache_dir: &Path) -> Self {
        Self {
            index_url: index_url.to_string(),
            cache_dir: cache_dir.to_path_buf(),
            cache_ttl_secs: DEFAULT_CACHE_TTL_SECS,
        }
    }

    /// Create with a custom cache TTL (in seconds).
    pub fn with_ttl(index_url: &str, cache_dir: &Path, ttl_secs: i64) -> Self {
        Self {
            index_url: index_url.to_string(),
            cache_dir: cache_dir.to_path_buf(),
            cache_ttl_secs: ttl_secs,
        }
    }

    /// Fetch the plugin index (from cache if fresh, otherwise remote).
    pub async fn fetch_index(&self) -> OxiResult<Vec<PluginEntry>> {
        // Try cached index first.
        if let Some(cached) = self.load_cache() {
            let age = Utc::now()
                .signed_duration_since(cached.fetched_at)
                .num_seconds();
            if age < self.cache_ttl_secs {
                tracing::debug!("Using cached plugin index (age: {age}s)");
                return Ok(cached.entries);
            }
        }

        // Fetch from remote.
        let entries = self.fetch_remote().await?;

        // Save to cache (best-effort).
        if let Err(e) = self.save_cache(&entries) {
            tracing::warn!("Failed to cache plugin index: {e}");
        }

        Ok(entries)
    }

    /// Search the index by name/description/keyword (case-insensitive substring match).
    pub fn search(entries: &[PluginEntry], query: &str) -> Vec<PluginEntry> {
        let q = query.to_lowercase();
        entries
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
                    || e.keywords.iter().any(|k| k.to_lowercase().contains(&q))
            })
            .cloned()
            .collect()
    }

    /// Filter entries compatible with the given OxiCode version.
    /// Entries with empty `min_oxicode_version` are always included.
    pub fn filter_compatible(entries: &[PluginEntry], version: &str) -> Vec<PluginEntry> {
        entries
            .iter()
            .filter(|e| {
                e.min_oxicode_version.is_empty()
                    || Self::version_satisfies(version, &e.min_oxicode_version)
            })
            .cloned()
            .collect()
    }

    /// Download a plugin archive from its download_url.
    /// Returns the bytes of the archive. Rejects downloads exceeding 100MB.
    pub async fn download_plugin(&self, entry: &PluginEntry) -> OxiResult<Vec<u8>> {
        const MAX_DOWNLOAD_SIZE: u64 = 100 * 1024 * 1024; // 100 MB

        if entry.download_url.is_empty() {
            return Err(OxiError::Config(format!(
                "Plugin '{}' has no download URL",
                entry.name
            )));
        }

        let client = reqwest::Client::new();
        let resp = client
            .get(&entry.download_url)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| OxiError::Other(format!("Download failed for '{}': {e}", entry.name)))?;

        if !resp.status().is_success() {
            return Err(OxiError::Other(format!(
                "Download failed for '{}': HTTP {}",
                entry.name,
                resp.status()
            )));
        }

        // Check Content-Length header before downloading body.
        if let Some(len) = resp.content_length() {
            if len > MAX_DOWNLOAD_SIZE {
                return Err(OxiError::Other(format!(
                    "Plugin '{}' archive too large: {} bytes (max: {MAX_DOWNLOAD_SIZE})",
                    entry.name, len
                )));
            }
        }

        let bytes = resp
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| OxiError::Other(format!("Failed to read download body: {e}")))?;

        // Also check actual body size (Content-Length may be missing).
        if bytes.len() as u64 > MAX_DOWNLOAD_SIZE {
            return Err(OxiError::Other(format!(
                "Plugin '{}' archive too large: {} bytes (max: {MAX_DOWNLOAD_SIZE})",
                entry.name,
                bytes.len()
            )));
        }

        Ok(bytes)
    }

    /// Fetch the remote index JSON.
    async fn fetch_remote(&self) -> OxiResult<Vec<PluginEntry>> {
        tracing::info!("Fetching plugin index from {}", self.index_url);

        let client = reqwest::Client::new();
        let resp = client
            .get(&self.index_url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| OxiError::Other(format!("Failed to fetch plugin index: {e}")))?;

        if !resp.status().is_success() {
            return Err(OxiError::Other(format!(
                "Plugin index returned HTTP {}",
                resp.status()
            )));
        }

        let entries: Vec<PluginEntry> = resp
            .json()
            .await
            .map_err(|e| OxiError::Other(format!("Invalid plugin index JSON: {e}")))?;

        Ok(entries)
    }

    /// Load cached index from disk.
    fn load_cache(&self) -> Option<CachedIndex> {
        let path = self.cache_path();
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save index to disk cache.
    fn save_cache(&self, entries: &[PluginEntry]) -> OxiResult<()> {
        std::fs::create_dir_all(&self.cache_dir)?;
        let cached = CachedIndex {
            fetched_at: Utc::now(),
            entries: entries.to_vec(),
        };
        let json = serde_json::to_string_pretty(&cached)
            .map_err(|e| OxiError::Other(format!("Cache serialization failed: {e}")))?;
        std::fs::write(self.cache_path(), json)?;
        Ok(())
    }

    /// Path to the cache file.
    fn cache_path(&self) -> PathBuf {
        self.cache_dir.join("plugin-registry-cache.json")
    }

    /// Simple semver comparison: check if `current >= required`.
    /// Only compares major.minor.patch (ignores pre-release).
    fn version_satisfies(current: &str, required: &str) -> bool {
        let parse = |v: &str| -> (u32, u32, u32) {
            let parts: Vec<u32> = v
                .split('.')
                .filter_map(|p| p.parse().ok())
                .collect();
            (
                parts.first().copied().unwrap_or(0),
                parts.get(1).copied().unwrap_or(0),
                parts.get(2).copied().unwrap_or(0),
            )
        };
        parse(current) >= parse(required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<PluginEntry> {
        vec![
            PluginEntry {
                name: "code-formatter".into(),
                version: "1.0.0".into(),
                description: "Auto-format code in multiple languages".into(),
                author: "alice".into(),
                download_url: String::new(),
                min_oxicode_version: "0.1.0".into(),
                keywords: vec!["format".into(), "lint".into()],
                trust: "verified".into(),
                permissions: vec!["file_edit".into()],
            },
            PluginEntry {
                name: "git-helper".into(),
                version: "0.5.0".into(),
                description: "Extended git operations".into(),
                author: "bob".into(),
                download_url: String::new(),
                min_oxicode_version: "0.2.0".into(),
                keywords: vec!["git".into(), "vcs".into()],
                trust: "community".into(),
                permissions: vec![],
            },
            PluginEntry {
                name: "secret-scanner".into(),
                version: "0.1.0".into(),
                description: "Scan for leaked secrets".into(),
                author: "charlie".into(),
                download_url: String::new(),
                min_oxicode_version: String::new(),
                keywords: vec!["security".into()],
                trust: "unverified".into(),
                permissions: vec![],
            },
        ]
    }

    #[test]
    fn test_search_by_name() {
        let entries = sample_entries();
        let results = PluginRegistry::search(&entries, "git");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "git-helper");
    }

    #[test]
    fn test_search_by_description() {
        let entries = sample_entries();
        let results = PluginRegistry::search(&entries, "format");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "code-formatter");
    }

    #[test]
    fn test_search_by_keyword() {
        let entries = sample_entries();
        let results = PluginRegistry::search(&entries, "security");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "secret-scanner");
    }

    #[test]
    fn test_search_case_insensitive() {
        let entries = sample_entries();
        let results = PluginRegistry::search(&entries, "GIT");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_results() {
        let entries = sample_entries();
        let results = PluginRegistry::search(&entries, "nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn test_filter_compatible() {
        let entries = sample_entries();
        // Version 0.1.0 should include code-formatter (0.1.0) and secret-scanner (no min).
        let compat = PluginRegistry::filter_compatible(&entries, "0.1.0");
        assert_eq!(compat.len(), 2);
        assert!(compat.iter().any(|e| e.name == "code-formatter"));
        assert!(compat.iter().any(|e| e.name == "secret-scanner"));
    }

    #[test]
    fn test_filter_compatible_all() {
        let entries = sample_entries();
        let compat = PluginRegistry::filter_compatible(&entries, "1.0.0");
        assert_eq!(compat.len(), 3);
    }

    #[test]
    fn test_version_satisfies() {
        assert!(PluginRegistry::version_satisfies("0.2.0", "0.1.0"));
        assert!(PluginRegistry::version_satisfies("1.0.0", "0.1.0"));
        assert!(PluginRegistry::version_satisfies("0.1.0", "0.1.0"));
        assert!(!PluginRegistry::version_satisfies("0.0.9", "0.1.0"));
    }

    #[test]
    fn test_cache_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = PluginRegistry::new("https://example.com/index.json", tmp.path());
        let entries = sample_entries();

        reg.save_cache(&entries).unwrap();
        let cached = reg.load_cache().unwrap();
        assert_eq!(cached.entries.len(), 3);
        assert_eq!(cached.entries[0].name, "code-formatter");
    }
}
