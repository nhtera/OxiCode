//! Team memory synchronization (feature-gated: `team_memory_sync`).
//!
//! Delta-syncs local project memories with a remote team endpoint using
//! SHA-256 checksums. Only transmits new/changed memories, minimizing
//! bandwidth. The sync URL is configured in `settings.toml`:
//!
//! ```toml
//! [team]
//! memory_sync_url = "https://team.example.com/api/team_memory/sync"
//! ```

#[cfg(feature = "team_memory_sync")]
mod inner {
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};

    use crate::memory::MemoryEntry;

    /// Checksum entry: memory ID + SHA-256 of content.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MemoryChecksum {
        pub id: String,
        pub checksum: String,
    }

    /// Request payload for delta sync.
    #[derive(Debug, Serialize)]
    pub struct SyncRequest {
        pub project_id: String,
        pub checksums: Vec<MemoryChecksum>,
    }

    /// Response from the sync endpoint.
    #[derive(Debug, Deserialize)]
    pub struct SyncResponse {
        /// Memory IDs the server doesn't have (we should upload).
        #[serde(default)]
        pub upload_ids: Vec<String>,
        /// Memories the server has that we don't.
        #[serde(default)]
        pub download: Vec<MemoryEntry>,
    }

    /// Result of a sync operation.
    #[derive(Debug)]
    pub struct SyncResult {
        pub uploaded: usize,
        pub downloaded: usize,
        pub errors: Vec<String>,
    }

    /// Compute SHA-256 hex digest for a memory entry's content.
    pub fn compute_checksum(entry: &MemoryEntry) -> String {
        let mut hasher = Sha256::new();
        hasher.update(entry.content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Build checksums for all local memories.
    pub fn build_checksums(memories: &[MemoryEntry]) -> Vec<MemoryChecksum> {
        memories
            .iter()
            .map(|m| MemoryChecksum {
                id: m.id.clone(),
                checksum: compute_checksum(m),
            })
            .collect()
    }

    /// Perform a delta sync with the remote team memory endpoint.
    ///
    /// 1. Validates URL uses HTTPS scheme.
    /// 2. Sends local checksums to the server.
    /// 3. Server responds with IDs to upload and memories to download.
    /// 4. Uploads requested memories, saves downloaded ones locally.
    pub async fn sync(
        sync_url: &str,
        project_id: &str,
        local_memories: &[MemoryEntry],
    ) -> SyncResult {
        let mut result = SyncResult {
            uploaded: 0,
            downloaded: 0,
            errors: Vec::new(),
        };

        // Security: validate URL scheme to prevent SSRF.
        if !sync_url.starts_with("https://") {
            result
                .errors
                .push("Sync URL must use HTTPS scheme".to_string());
            return result;
        }

        let checksums = build_checksums(local_memories);
        let request = SyncRequest {
            project_id: project_id.to_string(),
            checksums,
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        // Step 1: Send checksums, get diff.
        let response = match client.post(sync_url).json(&request).send().await {
            Ok(resp) => resp,
            Err(e) => {
                result.errors.push(format!("Sync request failed: {e}"));
                return result;
            }
        };

        if !response.status().is_success() {
            result.errors.push(format!(
                "Sync endpoint returned {}",
                response.status()
            ));
            return result;
        }

        let sync_response: SyncResponse = match response.json().await {
            Ok(r) => r,
            Err(e) => {
                result.errors.push(format!("Failed to parse sync response: {e}"));
                return result;
            }
        };

        // Step 2: Upload memories the server needs.
        let to_upload: Vec<&MemoryEntry> = local_memories
            .iter()
            .filter(|m| sync_response.upload_ids.contains(&m.id))
            .collect();

        if !to_upload.is_empty() {
            let upload_url = format!("{sync_url}/upload");
            match client.post(&upload_url).json(&to_upload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    result.uploaded = to_upload.len();
                }
                Ok(resp) => {
                    result
                        .errors
                        .push(format!("Upload failed with status {}", resp.status()));
                }
                Err(e) => {
                    result.errors.push(format!("Upload request failed: {e}"));
                }
            }
        }

        // Step 3: Save downloaded memories locally.
        for entry in &sync_response.download {
            match crate::memory::save_memory(entry) {
                Ok(_) => result.downloaded += 1,
                Err(e) => result.errors.push(format!("Save downloaded memory: {e}")),
            }
        }

        if result.uploaded > 0 || result.downloaded > 0 {
            tracing::info!(
                "Team memory sync: uploaded {}, downloaded {}",
                result.uploaded,
                result.downloaded
            );
        }

        result
    }
}

#[cfg(feature = "team_memory_sync")]
pub use inner::*;

// When the feature is disabled, provide stub types so code that references
// this module can still compile with cfg-gated usage.

#[cfg(test)]
#[cfg(feature = "team_memory_sync")]
mod tests {
    use super::*;
    use crate::memory::MemoryEntry;
    use chrono::Utc;

    fn make_memory(content: &str) -> MemoryEntry {
        MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            tags: vec![],
            source: "manual".to_string(),
            session_id: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn checksum_deterministic() {
        let entry = make_memory("Use Rust for CLI");
        let c1 = compute_checksum(&entry);
        let c2 = compute_checksum(&entry);
        assert_eq!(c1, c2);
    }

    #[test]
    fn checksum_different_content() {
        let a = make_memory("Use Rust");
        let b = make_memory("Use Python");
        assert_ne!(compute_checksum(&a), compute_checksum(&b));
    }

    #[test]
    fn build_checksums_matches_count() {
        let memories = vec![make_memory("A"), make_memory("B"), make_memory("C")];
        let checksums = build_checksums(&memories);
        assert_eq!(checksums.len(), 3);
        assert_eq!(checksums[0].id, memories[0].id);
    }

    #[test]
    fn checksum_hex_format() {
        let entry = make_memory("test content");
        let checksum = compute_checksum(&entry);
        // SHA-256 produces 64 hex chars.
        assert_eq!(checksum.len(), 64);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
