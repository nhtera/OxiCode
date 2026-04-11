//! Persistent prompt history stored as append-only JSONL at `~/.oxicode/history.jsonl`.
//!
//! Each line is a JSON object with timestamp and content. Max 1000 entries
//! with FIFO eviction. Consecutive duplicate entries are deduplicated.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use oxicode_common::constants::CONFIG_DIR_NAME;
use serde::{Deserialize, Serialize};

/// Maximum entries retained in the history file.
const MAX_ENTRIES: usize = 1000;

/// A single history entry persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// When the entry was submitted.
    #[serde(rename = "ts")]
    pub timestamp: DateTime<Utc>,
    /// The user input text.
    #[serde(rename = "c")]
    pub content: String,
    /// Working directory at time of submission (for project-scoped filtering).
    #[serde(rename = "d", skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
}

/// Append-only persistent history backed by a JSONL file.
pub struct PersistentHistory {
    entries: Vec<HistoryEntry>,
    file_path: PathBuf,
}

impl PersistentHistory {
    /// Load history from `~/.oxicode/history.jsonl` (or custom config dir).
    pub fn load(config_dir_override: Option<&Path>) -> Self {
        let base = config_dir_override.map_or_else(
            || {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(CONFIG_DIR_NAME)
            },
            PathBuf::from,
        );
        let file_path = base.join("history.jsonl");
        let entries = Self::read_file(&file_path);
        Self { entries, file_path }
    }

    /// Read and parse the JSONL file, skipping corrupted lines.
    fn read_file(path: &Path) -> Vec<HistoryEntry> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let reader = BufReader::new(file);
        let mut entries: Vec<HistoryEntry> = reader
            .lines()
            .filter_map(|line| {
                let line = line.ok()?;
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return None;
                }
                serde_json::from_str(trimmed).ok()
            })
            .collect();

        // Retain only the newest MAX_ENTRIES.
        if entries.len() > MAX_ENTRIES {
            entries.drain(..entries.len() - MAX_ENTRIES);
        }
        entries
    }

    /// Add an entry, deduplicating consecutive duplicates. Appends to file.
    pub fn add(&mut self, content: &str, project_dir: Option<&str>) {
        // Skip consecutive duplicates.
        if self
            .entries
            .last()
            .is_some_and(|last| last.content == content)
        {
            return;
        }

        let entry = HistoryEntry {
            timestamp: Utc::now(),
            content: content.to_string(),
            project_dir: project_dir.map(String::from),
        };

        // Append to file (create parent dirs if needed).
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
        {
            if let Ok(json) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{json}");
            }
            // Set restrictive permissions on Unix.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(
                    &self.file_path,
                    fs::Permissions::from_mode(0o600),
                );
            }
        }

        self.entries.push(entry);

        // FIFO eviction.
        if self.entries.len() > MAX_ENTRIES {
            self.entries.drain(..self.entries.len() - MAX_ENTRIES);
            // Rewrite the truncated file.
            self.rewrite_file();
        }
    }

    /// Rewrite the full file from in-memory entries (after eviction).
    fn rewrite_file(&self) {
        if let Ok(mut file) = File::create(&self.file_path) {
            for entry in &self.entries {
                if let Ok(json) = serde_json::to_string(entry) {
                    let _ = writeln!(file, "{json}");
                }
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(
                    &self.file_path,
                    fs::Permissions::from_mode(0o600),
                );
            }
        }
    }

    /// All entries (oldest first, newest last).
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Number of history entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get entry content by index (0 = oldest).
    pub fn get(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(|e| e.content.as_str())
    }

    /// Case-insensitive substring search, newest-first. Returns matching indices.
    pub fn search(&self, query: &str) -> Vec<usize> {
        if query.is_empty() {
            // Return all indices newest-first.
            return (0..self.entries.len()).rev().collect();
        }
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, e)| e.content.to_lowercase().contains(&query_lower))
            .map(|(i, _)| i)
            .collect()
    }

    /// Remove the last entry (undo). Also rewrites file.
    pub fn remove_last(&mut self) {
        if self.entries.pop().is_some() {
            self.rewrite_file();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_history(dir: &Path) -> PersistentHistory {
        PersistentHistory::load(Some(dir))
    }

    #[test]
    fn load_empty_file() {
        let dir = TempDir::new().unwrap();
        let hist = make_history(dir.path());
        assert!(hist.is_empty());
    }

    #[test]
    fn add_and_retrieve() {
        let dir = TempDir::new().unwrap();
        let mut hist = make_history(dir.path());
        hist.add("hello", None);
        hist.add("world", None);
        assert_eq!(hist.len(), 2);
        assert_eq!(hist.get(0).unwrap(), "hello");
        assert_eq!(hist.get(1).unwrap(), "world");
    }

    #[test]
    fn consecutive_dedup() {
        let dir = TempDir::new().unwrap();
        let mut hist = make_history(dir.path());
        hist.add("hello", None);
        hist.add("hello", None);
        hist.add("hello", None);
        assert_eq!(hist.len(), 1);
    }

    #[test]
    fn non_consecutive_duplicates_kept() {
        let dir = TempDir::new().unwrap();
        let mut hist = make_history(dir.path());
        hist.add("hello", None);
        hist.add("world", None);
        hist.add("hello", None);
        assert_eq!(hist.len(), 3);
    }

    #[test]
    fn search_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let mut hist = make_history(dir.path());
        hist.add("Fix the bug", None);
        hist.add("add feature", None);
        hist.add("fix the tests", None);
        let results = hist.search("fix");
        assert_eq!(results, vec![2, 0]); // newest first
    }

    #[test]
    fn search_empty_query_returns_all() {
        let dir = TempDir::new().unwrap();
        let mut hist = make_history(dir.path());
        hist.add("one", None);
        hist.add("two", None);
        let results = hist.search("");
        assert_eq!(results, vec![1, 0]);
    }

    #[test]
    fn persistence_across_loads() {
        let dir = TempDir::new().unwrap();
        {
            let mut hist = make_history(dir.path());
            hist.add("persistent entry", None);
        }
        // Load again from same dir.
        let hist = make_history(dir.path());
        assert_eq!(hist.len(), 1);
        assert_eq!(hist.get(0).unwrap(), "persistent entry");
    }

    #[test]
    fn corrupted_lines_skipped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("history.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(file, r#"{{"ts":"2026-01-01T00:00:00Z","c":"good"}}"#).unwrap();
        writeln!(file, "this is not json").unwrap();
        writeln!(file, r#"{{"ts":"2026-01-02T00:00:00Z","c":"also good"}}"#).unwrap();
        drop(file);

        let hist = PersistentHistory::load(Some(dir.path()));
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn remove_last() {
        let dir = TempDir::new().unwrap();
        let mut hist = make_history(dir.path());
        hist.add("one", None);
        hist.add("two", None);
        hist.remove_last();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist.get(0).unwrap(), "one");
    }
}
