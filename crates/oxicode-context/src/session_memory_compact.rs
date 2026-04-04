//! Session memory compaction: persistent summaries with boundary tracking.
//!
//! After an LLM auto-compact (L3), saves the summary and a boundary marker
//! so subsequent compactions skip already-summarized content. Summaries
//! persist to `~/.oxicode/sessions/{id}/compact-summaries.json`.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Max summaries to keep on disk per session.
const MAX_SUMMARIES: usize = 20;

/// A single compaction summary with its boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactSummary {
    /// The LLM-generated summary text.
    pub summary: String,
    /// Number of messages that were summarized.
    pub messages_summarized: usize,
    /// When the compaction occurred.
    pub timestamp: DateTime<Utc>,
}

/// Tracks compaction history for a session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMemory {
    /// Session identifier.
    pub session_id: String,
    /// Ordered list of compaction summaries (oldest first).
    pub summaries: Vec<CompactSummary>,
    /// Total messages summarized across all compactions.
    pub total_summarized: usize,
}

impl SessionMemory {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            summaries: Vec::new(),
            total_summarized: 0,
        }
    }

    /// Record a new compaction summary.
    pub fn record_summary(&mut self, summary: String, messages_summarized: usize) {
        self.total_summarized += messages_summarized;
        self.summaries.push(CompactSummary {
            summary,
            messages_summarized,
            timestamp: Utc::now(),
        });

        // Cap stored summaries.
        if self.summaries.len() > MAX_SUMMARIES {
            // Keep the most recent summaries, drop oldest.
            let drain_count = self.summaries.len() - MAX_SUMMARIES;
            self.summaries.drain(..drain_count);
        }

        tracing::info!(
            total_summarized = self.total_summarized,
            summaries = self.summaries.len(),
            "session memory: recorded compaction summary"
        );
    }

    /// Build a preamble string from stored summaries for context injection.
    /// Returns None if no summaries exist.
    pub fn build_preamble(&self) -> Option<String> {
        if self.summaries.is_empty() {
            return None;
        }

        let mut preamble = String::from("[Previous conversation summaries]\n");
        for (i, s) in self.summaries.iter().enumerate() {
            let _ = write!(
                preamble,
                "\n--- Summary {} ({} messages) ---\n{}\n",
                i + 1,
                s.messages_summarized,
                s.summary
            );
        }
        Some(preamble)
    }

    /// Get the number of stored summaries.
    pub fn summary_count(&self) -> usize {
        self.summaries.len()
    }
}

/// Get the persistence path for a session's compact summaries.
pub fn session_memory_path(session_id: &str) -> PathBuf {
    let base = std::env::var("OXICODE_DATA_DIR").ok().map_or_else(
        || {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oxicode")
        },
        PathBuf::from,
    );
    base.join("sessions")
        .join(session_id)
        .join("compact-summaries.json")
}

/// Load session memory from disk.
pub fn load_session_memory(path: &Path) -> SessionMemory {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => SessionMemory::default(),
    }
}

/// Save session memory to disk.
pub fn save_session_memory(path: &Path, memory: &SessionMemory) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(memory).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_memory_empty() {
        let mem = SessionMemory::new("test-session");
        assert_eq!(mem.session_id, "test-session");
        assert!(mem.summaries.is_empty());
        assert_eq!(mem.total_summarized, 0);
    }

    #[test]
    fn record_summary_increments() {
        let mut mem = SessionMemory::new("s1");
        mem.record_summary("Summary 1".to_string(), 10);
        assert_eq!(mem.summary_count(), 1);
        assert_eq!(mem.total_summarized, 10);

        mem.record_summary("Summary 2".to_string(), 5);
        assert_eq!(mem.summary_count(), 2);
        assert_eq!(mem.total_summarized, 15);
    }

    #[test]
    fn caps_at_max_summaries() {
        let mut mem = SessionMemory::new("s1");
        for i in 0..25 {
            mem.record_summary(format!("Summary {i}"), 1);
        }
        assert_eq!(mem.summary_count(), MAX_SUMMARIES);
        // Most recent should be preserved.
        assert!(mem.summaries.last().unwrap().summary.contains("24"));
    }

    #[test]
    fn build_preamble_none_when_empty() {
        let mem = SessionMemory::new("s1");
        assert!(mem.build_preamble().is_none());
    }

    #[test]
    fn build_preamble_contains_summaries() {
        let mut mem = SessionMemory::new("s1");
        mem.record_summary("First summary".to_string(), 10);
        mem.record_summary("Second summary".to_string(), 5);

        let preamble = mem.build_preamble().unwrap();
        assert!(preamble.contains("Previous conversation summaries"));
        assert!(preamble.contains("First summary"));
        assert!(preamble.contains("Second summary"));
        assert!(preamble.contains("10 messages"));
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("summaries.json");

        let mut mem = SessionMemory::new("s1");
        mem.record_summary("Test summary".to_string(), 8);
        save_session_memory(&path, &mem).unwrap();

        let loaded = load_session_memory(&path);
        assert_eq!(loaded.session_id, "s1");
        assert_eq!(loaded.summary_count(), 1);
        assert_eq!(loaded.total_summarized, 8);
        assert_eq!(loaded.summaries[0].summary, "Test summary");
    }

    #[test]
    fn load_missing_file_returns_default() {
        let mem = load_session_memory(Path::new("/nonexistent/path.json"));
        assert!(mem.summaries.is_empty());
    }

    #[test]
    fn load_corrupt_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();

        let mem = load_session_memory(&path);
        assert!(mem.summaries.is_empty());
    }

    #[test]
    fn session_memory_path_structure() {
        let path = session_memory_path("abc-123");
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("sessions"));
        assert!(path_str.contains("abc-123"));
        assert!(path_str.ends_with("compact-summaries.json"));
    }
}
