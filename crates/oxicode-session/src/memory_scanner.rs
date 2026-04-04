//! Scan memory directories for markdown files with frontmatter.
//!
//! Walks a memory directory (max depth 3), parses YAML frontmatter from
//! `.md` files, sorts by modification time (newest first), and caps at
//! 200 files. Skips files > 50KB for safety.

use std::path::Path;
use std::time::SystemTime;

use crate::memdir::ENTRYPOINT_NAME;
use crate::memory_types::{parse_frontmatter, MemoryHeader};

/// Max files to return from a scan.
const MAX_FILES: usize = 200;

/// Max directory depth to walk.
const MAX_DEPTH: usize = 3;

/// Skip files larger than this (50KB).
const MAX_FILE_BYTES: u64 = 50 * 1024;

/// Scan a memory directory for `.md` files with parsed frontmatter.
/// Returns headers sorted by mtime (newest first), capped at 200.
pub fn scan_memory_files(dir: &Path) -> Result<Vec<MemoryHeader>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut headers = Vec::new();
    walk_dir(dir, 0, &mut headers)?;

    // Sort newest first.
    headers.sort_by(|a, b| b.mtime.cmp(&a.mtime));

    // Cap at MAX_FILES.
    headers.truncate(MAX_FILES);
    Ok(headers)
}

/// Recursive directory walker with depth limit.
fn walk_dir(current: &Path, depth: usize, results: &mut Vec<MemoryHeader>) -> Result<(), String> {
    if depth > MAX_DEPTH || results.len() >= MAX_FILES {
        return Ok(());
    }

    let entries = std::fs::read_dir(current).map_err(|e| format!("read_dir: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();

        if path.is_dir() {
            walk_dir(&path, depth + 1, results)?;
        } else if path.extension().is_some_and(|ext| ext == "md") {
            // Skip entrypoint — it's handled separately.
            if path.file_name().is_some_and(|n| n == ENTRYPOINT_NAME) && depth == 0 {
                continue;
            }

            // Skip oversized files.
            let metadata = std::fs::metadata(&path).map_err(|e| format!("metadata: {e}"))?;
            if metadata.len() > MAX_FILE_BYTES {
                continue;
            }

            let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Parse frontmatter.
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let (fm, _body) = parse_frontmatter(&content);

            let fields = fm.unwrap_or_default();

            results.push(MemoryHeader {
                filename,
                filepath: path,
                memory_type: fields.memory_type,
                description: fields.description,
                tags: fields.tags,
                mtime,
            });
        }
    }
    Ok(())
}

/// Build a summary index of memory headers for system prompt injection.
/// Format: one line per file with type, description, and filename.
pub fn build_memory_index(headers: &[MemoryHeader]) -> String {
    use std::fmt::Write;

    if headers.is_empty() {
        return String::new();
    }

    let mut index = String::new();
    for h in headers {
        let type_str = h.memory_type.map_or("note", |t| t.as_str());
        let desc = h.description.as_deref().unwrap_or(&h.filename);
        let _ = writeln!(index, "- [{type_str}] {desc}");
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_memory_file(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn scan_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let results = scan_memory_files(dir.path()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn scan_nonexistent_dir() {
        let results = scan_memory_files(Path::new("/nonexistent/dir")).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn scan_finds_md_files() {
        let dir = tempfile::tempdir().unwrap();
        create_memory_file(
            dir.path(),
            "decision.md",
            "---\ntype: decision\ndescription: \"Use Rust\"\n---\nBody.",
        );
        create_memory_file(dir.path(), "note.md", "Plain note without frontmatter.");

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn scan_parses_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        create_memory_file(
            dir.path(),
            "pref.md",
            "---\ntype: preference\ndescription: \"Snake case\"\ntags: [style]\n---\nBody.",
        );

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 1);

        let h = &results[0];
        assert_eq!(
            h.memory_type,
            Some(crate::memory_types::MemoryType::Preference)
        );
        assert_eq!(h.description.as_deref(), Some("Snake case"));
        assert_eq!(h.tags, vec!["style"]);
    }

    #[test]
    fn scan_skips_entrypoint() {
        let dir = tempfile::tempdir().unwrap();
        create_memory_file(dir.path(), "MEMORY.md", "# Entrypoint");
        create_memory_file(dir.path(), "note.md", "A note.");

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "note.md");
    }

    #[test]
    fn scan_skips_non_md_files() {
        let dir = tempfile::tempdir().unwrap();
        create_memory_file(dir.path(), "note.md", "A note.");
        create_memory_file(dir.path(), "data.json", "{}");
        create_memory_file(dir.path(), "script.sh", "#!/bin/bash");

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn scan_respects_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        // Create nested dirs deeper than MAX_DEPTH.
        let deep = dir.path().join("a/b/c/d/e");
        std::fs::create_dir_all(&deep).unwrap();
        create_memory_file(&deep, "deep.md", "Too deep.");
        create_memory_file(dir.path(), "top.md", "Top level.");

        let results = scan_memory_files(dir.path()).unwrap();
        // Only top.md should be found (deep.md is at depth 5).
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "top.md");
    }

    #[test]
    fn scan_skips_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let big_content = "x".repeat(60 * 1024); // 60KB > 50KB limit
        create_memory_file(dir.path(), "big.md", &big_content);
        create_memory_file(dir.path(), "small.md", "Small file.");

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "small.md");
    }

    #[test]
    fn scan_sorted_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        create_memory_file(dir.path(), "old.md", "Old note.");
        // Small delay to ensure different mtimes.
        std::thread::sleep(std::time::Duration::from_millis(20));
        create_memory_file(dir.path(), "new.md", "New note.");

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].filename, "new.md");
        assert_eq!(results[1].filename, "old.md");
    }

    #[test]
    fn build_index_empty() {
        let index = build_memory_index(&[]);
        assert!(index.is_empty());
    }

    #[test]
    fn build_index_with_headers() {
        use crate::memory_types::MemoryType;
        let headers = vec![
            MemoryHeader {
                filename: "arch.md".to_string(),
                filepath: Path::new("arch.md").to_path_buf(),
                memory_type: Some(MemoryType::Decision),
                description: Some("Architecture decisions".to_string()),
                tags: vec![],
                mtime: SystemTime::now(),
            },
            MemoryHeader {
                filename: "note.md".to_string(),
                filepath: Path::new("note.md").to_path_buf(),
                memory_type: None,
                description: None,
                tags: vec![],
                mtime: SystemTime::now(),
            },
        ];

        let index = build_memory_index(&headers);
        assert!(index.contains("[decision] Architecture decisions"));
        assert!(index.contains("[note] note.md")); // Fallback to filename
    }

    #[test]
    fn scan_within_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        // Depth 1 — should be found.
        let sub = dir.path().join("subdir");
        std::fs::create_dir_all(&sub).unwrap();
        create_memory_file(&sub, "nested.md", "In subdir.");

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "nested.md");
    }

    #[test]
    fn scan_max_depth_boundary() {
        let dir = tempfile::tempdir().unwrap();
        // Depth 3 exactly — should still be found (depth <= MAX_DEPTH).
        let d3 = dir.path().join("a/b/c");
        std::fs::create_dir_all(&d3).unwrap();
        create_memory_file(&d3, "d3.md", "At depth 3.");

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn scan_with_frontmatter_tags() {
        let dir = tempfile::tempdir().unwrap();
        create_memory_file(
            dir.path(),
            "tagged.md",
            "---\ntype: preference\ntags: [rust, coding]\n---\nPrefer snake_case.",
        );

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tags, vec!["rust", "coding"]);
    }

    #[test]
    fn scan_with_no_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        create_memory_file(dir.path(), "plain.md", "Just plain text, no frontmatter.");

        let results = scan_memory_files(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].memory_type.is_none());
        assert!(results[0].description.is_none());
    }

    #[test]
    fn build_index_all_memory_types() {
        use crate::memory_types::MemoryType;
        let types = [
            MemoryType::Decision,
            MemoryType::Preference,
            MemoryType::Context,
            MemoryType::Task,
        ];
        let headers: Vec<MemoryHeader> = types
            .iter()
            .map(|t: &MemoryType| MemoryHeader {
                filename: format!("{}.md", t.as_str()),
                filepath: Path::new("test.md").to_path_buf(),
                memory_type: Some(*t),
                description: Some(format!("A {} entry", t.as_str())),
                tags: vec![],
                mtime: SystemTime::now(),
            })
            .collect();

        let index = build_memory_index(&headers);
        for t in &types {
            assert!(index.contains(t.as_str()));
        }
    }
}
