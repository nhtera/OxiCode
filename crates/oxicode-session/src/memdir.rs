//! Project-based memory directory management.
//!
//! Each git project gets its own memory directory under
//! `~/.oxicode/projects/{key}/memory/`. The key is derived from the
//! git root path. MEMORY.md is the primary entrypoint file with
//! 200-line / 25KB truncation limits.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use oxicode_common::constants::CONFIG_DIR_NAME;

/// Max lines in MEMORY.md before truncation warning.
const MAX_ENTRYPOINT_LINES: usize = 200;

/// Max bytes in MEMORY.md before truncation.
const MAX_ENTRYPOINT_BYTES: usize = 25 * 1024;

/// Entrypoint filename.
pub const ENTRYPOINT_NAME: &str = "MEMORY.md";

/// Get the base directory for all project memory dirs.
pub fn memory_base_dir() -> PathBuf {
    let base = std::env::var("OXICODE_MEMORY_DIR").ok().map_or_else(
        || {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(CONFIG_DIR_NAME)
        },
        PathBuf::from,
    );
    base.join("projects")
}

/// Generate a stable key from a git root path.
/// Format: `{dirname}-{hash8}` for human readability + uniqueness.
pub fn project_key(git_root: &Path) -> String {
    let name = git_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    // Sanitize: keep alphanumeric, dash, underscore.
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut hasher = DefaultHasher::new();
    git_root.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();
    format!("{sanitized}-{hash:016x}")
}

/// Get the memory directory for a project.
pub fn project_memory_dir(git_root: &Path) -> PathBuf {
    memory_base_dir().join(project_key(git_root)).join("memory")
}

/// Ensure the memory directory exists (create if needed).
pub fn ensure_memory_dir(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(dir).map_err(|e| format!("mkdir: {e}"))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir: {e}"))
    }
}

/// Read the MEMORY.md entrypoint, applying truncation limits.
/// Returns content + whether it was truncated.
pub fn read_entrypoint(memory_dir: &Path) -> Option<(String, bool)> {
    let path = memory_dir.join(ENTRYPOINT_NAME);
    let content = std::fs::read_to_string(&path).ok()?;
    if content.is_empty() {
        return None;
    }
    Some(truncate_entrypoint(&content))
}

/// Apply 200-line / 25KB truncation to entrypoint content.
fn truncate_entrypoint(content: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let mut truncated = false;

    // Line limit.
    let capped_lines = if lines.len() > MAX_ENTRYPOINT_LINES {
        truncated = true;
        &lines[..MAX_ENTRYPOINT_LINES]
    } else {
        &lines[..]
    };

    let mut result = capped_lines.join("\n");

    // Byte limit (snap to char boundary to avoid panicking on multi-byte UTF-8).
    if result.len() > MAX_ENTRYPOINT_BYTES {
        truncated = true;
        let mut end = MAX_ENTRYPOINT_BYTES;
        while end > 0 && !result.is_char_boundary(end) {
            end -= 1;
        }
        // Cut at last newline before byte limit.
        if let Some(pos) = result[..end].rfind('\n') {
            result.truncate(pos);
        } else {
            result.truncate(end);
        }
    }

    if truncated {
        result.push_str("\n\n<!-- Truncated: exceeds 200 lines or 25KB limit -->");
    }

    (result, truncated)
}

/// Write MEMORY.md entrypoint content.
pub fn write_entrypoint(memory_dir: &Path, content: &str) -> Result<(), String> {
    ensure_memory_dir(memory_dir)?;
    let path = memory_dir.join(ENTRYPOINT_NAME);
    std::fs::write(&path, content).map_err(|e| format!("write: {e}"))
}

/// Create a default MEMORY.md template if none exists.
pub fn ensure_entrypoint(memory_dir: &Path) -> Result<PathBuf, String> {
    ensure_memory_dir(memory_dir)?;
    let path = memory_dir.join(ENTRYPOINT_NAME);
    if !path.exists() {
        let template = "# Project Memory\n\nAdd project-specific memories and context here.\n";
        std::fs::write(&path, template).map_err(|e| format!("write: {e}"))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_key_deterministic() {
        let root = Path::new("/home/user/projects/my-app");
        let key1 = project_key(root);
        let key2 = project_key(root);
        assert_eq!(key1, key2);
    }

    #[test]
    fn project_key_contains_dirname() {
        let root = Path::new("/home/user/projects/my-app");
        let key = project_key(root);
        assert!(key.starts_with("my-app-"));
    }

    #[test]
    fn project_key_different_for_different_roots() {
        let key1 = project_key(Path::new("/a/my-app"));
        let key2 = project_key(Path::new("/b/my-app"));
        assert_ne!(key1, key2);
    }

    #[test]
    fn project_key_sanitizes_special_chars() {
        let key = project_key(Path::new("/home/user/my project @2"));
        // Spaces and @ → dashes.
        assert!(!key.contains(' '));
        assert!(!key.contains('@'));
    }

    #[test]
    fn project_memory_dir_structure() {
        let root = Path::new("/home/user/my-app");
        let dir = project_memory_dir(root);
        assert!(dir.to_string_lossy().contains("projects"));
        assert!(dir.to_string_lossy().contains("memory"));
    }

    #[test]
    fn truncate_short_content() {
        let content = "Line 1\nLine 2\nLine 3";
        let (result, truncated) = truncate_entrypoint(content);
        assert!(!truncated);
        assert_eq!(result, content);
    }

    #[test]
    fn truncate_exceeds_line_limit() {
        let lines: Vec<String> = (0..250).map(|i| format!("Line {i}")).collect();
        let content = lines.join("\n");
        let (result, truncated) = truncate_entrypoint(&content);
        assert!(truncated);
        assert!(result.lines().count() <= MAX_ENTRYPOINT_LINES + 3); // +truncation notice
        assert!(result.contains("Truncated"));
    }

    #[test]
    fn truncate_exceeds_byte_limit() {
        // Create content > 25KB but < 200 lines.
        let line = "x".repeat(200);
        let lines: Vec<String> = (0..150).map(|_| line.clone()).collect();
        let content = lines.join("\n");
        assert!(content.len() > MAX_ENTRYPOINT_BYTES);

        let (result, truncated) = truncate_entrypoint(&content);
        assert!(truncated);
        assert!(result.len() < content.len());
        assert!(result.contains("Truncated"));
    }

    #[test]
    fn entrypoint_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        write_entrypoint(dir.path(), "# Memory\nFact 1").unwrap();
        let (content, truncated) = read_entrypoint(dir.path()).unwrap();
        assert!(!truncated);
        assert!(content.contains("Fact 1"));
    }

    #[test]
    fn ensure_entrypoint_creates_template() {
        let dir = tempfile::tempdir().unwrap();
        let mem_dir = dir.path().join("memory");
        let path = ensure_entrypoint(&mem_dir).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Project Memory"));
    }

    #[test]
    fn ensure_entrypoint_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let mem_dir = dir.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        std::fs::write(mem_dir.join("MEMORY.md"), "Custom content").unwrap();

        ensure_entrypoint(&mem_dir).unwrap();
        let content = std::fs::read_to_string(mem_dir.join("MEMORY.md")).unwrap();
        assert_eq!(content, "Custom content");
    }

    #[test]
    fn read_entrypoint_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_entrypoint(dir.path()).is_none());
    }

    #[test]
    fn read_entrypoint_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MEMORY.md"), "").unwrap();
        assert!(read_entrypoint(dir.path()).is_none());
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mem_dir = dir.path().join("mem");
        write_entrypoint(&mem_dir, "# Test Memory\n\nFact: Rust is fast.").unwrap();
        let (content, truncated) = read_entrypoint(&mem_dir).unwrap();
        assert!(!truncated);
        assert!(content.contains("Rust is fast"));
    }

    #[test]
    fn truncate_exactly_at_line_limit() {
        let lines: Vec<String> = (0..200).map(|i| format!("Line {i}")).collect();
        let content = lines.join("\n");
        let (_, truncated) = truncate_entrypoint(&content);
        assert!(!truncated);
    }

    #[test]
    fn truncate_one_over_line_limit() {
        let lines: Vec<String> = (0..201).map(|i| format!("Line {i}")).collect();
        let content = lines.join("\n");
        let (result, truncated) = truncate_entrypoint(&content);
        assert!(truncated);
        assert!(result.contains("Truncated"));
    }

    #[test]
    fn project_key_handles_root_path() {
        let key = project_key(Path::new("/"));
        // Should not panic. The dirname of "/" is empty/root.
        assert!(!key.is_empty());
    }

    #[test]
    fn ensure_memory_dir_creates_nested() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b/c");
        ensure_memory_dir(&nested).unwrap();
        assert!(nested.exists());
    }
}
