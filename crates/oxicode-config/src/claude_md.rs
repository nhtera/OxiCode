use std::path::{Path, PathBuf};

use oxicode_common::constants::CLAUDE_MD_FILES;

/// Discover CLAUDE.md or OXICODE.md by walking up from the given directory.
/// OXICODE.md takes precedence over CLAUDE.md.
/// H6 FIX: Stops at .git boundary (project root) or after max 20 levels.
pub fn discover_claude_md(start_dir: &Path) -> Option<(PathBuf, String)> {
    let mut dir = start_dir.to_path_buf();
    let max_depth = 20;

    for _ in 0..max_depth {
        for filename in CLAUDE_MD_FILES {
            let candidate = dir.join(filename);
            if candidate.is_file() {
                if let Ok(content) = std::fs::read_to_string(&candidate) {
                    tracing::debug!("Found {} at {}", filename, candidate.display());
                    return Some((candidate, content));
                }
            }
        }

        // Stop at project root (.git directory present)
        if dir.join(".git").exists() {
            break;
        }

        if !dir.pop() {
            break;
        }
    }

    None
}

/// Discover CLAUDE.md from the user's home directory (~/.claude/CLAUDE.md).
pub fn discover_global_claude_md() -> Option<String> {
    let home = dirs::home_dir()?;
    let path = home.join(".claude").join("CLAUDE.md");
    if path.is_file() {
        std::fs::read_to_string(&path).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_claude_md_in_current_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let md_path = tmp.path().join("CLAUDE.md");
        fs::write(&md_path, "# Test instructions").unwrap();

        let result = discover_claude_md(tmp.path());
        assert!(result.is_some());
        let (path, content) = result.unwrap();
        assert_eq!(path, md_path);
        assert!(content.contains("Test instructions"));
    }

    #[test]
    fn test_oxicode_md_takes_precedence() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("CLAUDE.md"), "claude").unwrap();
        fs::write(tmp.path().join("OXICODE.md"), "oxicode").unwrap();

        let (_, content) = discover_claude_md(tmp.path()).unwrap();
        assert_eq!(content, "oxicode");
    }

    #[test]
    fn test_discover_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("CLAUDE.md"), "root").unwrap();
        let sub = tmp.path().join("a").join("b");
        fs::create_dir_all(&sub).unwrap();

        let (_, content) = discover_claude_md(&sub).unwrap();
        assert_eq!(content, "root");
    }
}
