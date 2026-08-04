//! Memory service: extract and persist conversation memories.
//!
//! Stores JSON memory entries in `~/.oxicode/memory/` with tags for searchability.
//! Memories are auto-extracted from conversations or manually added via `/memory add`.

use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use oxicode_common::constants::CONFIG_DIR_NAME;

/// Directory name for memory storage inside config dir.
const MEMORY_DIR_NAME: &str = "memory";

/// A single memory entry persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    /// The memory content (fact, preference, or instruction).
    pub content: String,
    /// Tags for categorization and search.
    pub tags: Vec<String>,
    /// Source: "auto" (extracted) or "manual" (user-added).
    pub source: String,
    /// Session ID where this memory was created.
    pub session_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl MemoryEntry {
    /// Create a new manual memory entry.
    pub fn manual(content: impl Into<String>, tags: Vec<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.into(),
            tags,
            source: "manual".to_string(),
            session_id: None,
            created_at: Utc::now(),
        }
    }

    /// Create an auto-extracted memory entry.
    pub fn auto_extracted(content: impl Into<String>, tags: Vec<String>, session_id: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.into(),
            tags,
            source: "auto".to_string(),
            session_id: Some(session_id.to_string()),
            created_at: Utc::now(),
        }
    }

    /// Check if this memory matches a search query (case-insensitive).
    pub fn matches(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.content.to_lowercase().contains(&q)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&q))
    }
}

/// Get the memory directory path.
pub fn memory_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CONFIG_DIR_NAME)
        .join(MEMORY_DIR_NAME)
}

/// Save a memory entry to disk.
pub fn save_memory(entry: &MemoryEntry) -> Result<PathBuf, String> {
    let dir = memory_dir();

    // Create dir with restricted permissions on Unix (0o700).
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(&dir)
            .map_err(|e| format!("Failed to create memory dir: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create memory dir: {e}"))?;
    }

    let path = dir.join(format!("{}.json", entry.id));
    let json =
        serde_json::to_string_pretty(entry).map_err(|e| format!("Failed to serialize: {e}"))?;

    write_with_permissions(&path, &json)?;
    Ok(path)
}

/// Write file with restricted permissions on Unix.
fn write_with_permissions(path: &Path, content: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("Failed to write memory file: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("Failed to write: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        fs::write(path, content).map_err(|e| format!("Failed to write memory file: {e}"))?;
    }
    Ok(())
}

/// Load all memory entries from disk.
pub fn load_all_memories() -> Result<Vec<MemoryEntry>, String> {
    let dir = memory_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    let read_dir = fs::read_dir(&dir).map_err(|e| format!("Failed to read memory dir: {e}"))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Dir entry error: {e}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mem) = serde_json::from_str::<MemoryEntry>(&content) {
                    entries.push(mem);
                }
            }
        }
    }

    entries.sort_by_key(|e| Reverse(e.created_at));
    Ok(entries)
}

/// Search memories by query string (case-insensitive content + tag match).
pub fn search_memories(query: &str) -> Result<Vec<MemoryEntry>, String> {
    let all = load_all_memories()?;
    Ok(all.into_iter().filter(|m| m.matches(query)).collect())
}

/// Delete a memory entry by ID.
pub fn delete_memory(id: &str) -> Result<(), String> {
    // Reject path traversal.
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err("Invalid memory ID".to_string());
    }

    let path = memory_dir().join(format!("{id}.json"));
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to delete: {e}"))?;
    }
    Ok(())
}

/// Count total memories on disk.
pub fn memory_count() -> usize {
    load_all_memories().map_or(0, |m| m.len())
}

/// Extract memory-worthy content from conversation text.
///
/// Simple heuristic: looks for explicit memory markers like "remember that",
/// "note that", "my preference is", etc.
pub fn extract_memories_from_text(text: &str, session_id: &str) -> Vec<MemoryEntry> {
    let mut memories = Vec::new();

    // Memory marker patterns.
    let markers = [
        "remember that ",
        "note that ",
        "my preference is ",
        "i always ",
        "i prefer ",
        "i never ",
        "always use ",
        "never use ",
    ];

    for line in text.lines() {
        let line_lower = line.trim().to_lowercase();
        for marker in &markers {
            if line_lower.contains(marker) {
                let content = line.trim().to_string();
                if !content.is_empty() && content.len() > 10 {
                    let tags = infer_tags(&content);
                    memories.push(MemoryEntry::auto_extracted(content, tags, session_id));
                    break;
                }
            }
        }
    }

    memories
}

/// Infer tags from memory content using simple keyword detection.
fn infer_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let lower = content.to_lowercase();

    let tag_map = [
        (
            &["prefer", "preference", "always", "never"][..],
            "preference",
        ),
        (&["code", "function", "variable", "class"][..], "code"),
        (&["project", "repo", "repository"][..], "project"),
        (&["tool", "editor", "ide"][..], "tooling"),
        (&["style", "format", "naming"][..], "style"),
    ];

    for (keywords, tag) in &tag_map {
        if keywords.iter().any(|kw| lower.contains(kw)) {
            tags.push((*tag).to_string());
        }
    }

    if tags.is_empty() {
        tags.push("general".to_string());
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_entry_manual() {
        let entry = MemoryEntry::manual("test fact", vec!["tag1".to_string()]);
        assert_eq!(entry.source, "manual");
        assert_eq!(entry.content, "test fact");
        assert_eq!(entry.tags, vec!["tag1"]);
    }

    #[test]
    fn test_memory_entry_auto() {
        let entry = MemoryEntry::auto_extracted("auto fact", vec![], "session-123");
        assert_eq!(entry.source, "auto");
        assert_eq!(entry.session_id, Some("session-123".to_string()));
    }

    #[test]
    fn test_memory_matches() {
        let entry = MemoryEntry::manual("I prefer Rust", vec!["preference".to_string()]);
        assert!(entry.matches("rust"));
        assert!(entry.matches("prefer"));
        assert!(entry.matches("preference"));
        assert!(!entry.matches("python"));
    }

    #[test]
    fn test_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let entry = MemoryEntry::manual("test memory", vec!["test".to_string()]);
        let path = tmp.path().join(format!("{}.json", entry.id));
        let json = serde_json::to_string_pretty(&entry).unwrap();
        std::fs::write(&path, json).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: MemoryEntry = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.content, "test memory");
    }

    #[test]
    fn test_extract_memories() {
        let text = "I prefer using snake_case for variables\nSome random text\nAlways use Rust for CLI tools";
        let memories = extract_memories_from_text(text, "s1");
        assert!(!memories.is_empty());
        assert!(memories.iter().all(|m| m.source == "auto"));
    }

    #[test]
    fn test_infer_tags() {
        let tags = infer_tags("I prefer using my editor for editing code");
        assert!(tags.contains(&"preference".to_string()));
        assert!(tags.contains(&"code".to_string()));
        assert!(tags.contains(&"tooling".to_string()));
    }

    #[test]
    fn test_delete_rejects_path_traversal() {
        let result = delete_memory("../../../etc/passwd");
        assert!(result.is_err());
    }
}
