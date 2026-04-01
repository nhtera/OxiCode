use std::path::{Path, PathBuf};

use crate::tool_trait::ToolResult;

/// Resolve a path, making relative paths absolute against working_dir.
pub fn resolve_path(file_path: &str, working_dir: &Path) -> PathBuf {
    let p = Path::new(file_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    }
}

/// Sensitive paths that tools should never write to.
const SENSITIVE_PATHS: &[&str] = &[
    "/etc/passwd",
    "/etc/shadow",
    "/etc/sudoers",
    "/.ssh/",
    "/.gnupg/",
];

/// Check if a resolved path appears to be targeting a sensitive system location.
/// Returns an error ToolResult if the path is sensitive.
pub fn check_path_safety(path: &Path) -> Option<ToolResult> {
    let path_str = path.to_string_lossy();

    for sensitive in SENSITIVE_PATHS {
        if path_str.contains(sensitive) {
            return Some(ToolResult::error(format!(
                "Access denied: path targets sensitive location ({sensitive})"
            )));
        }
    }

    // Check for path traversal attempts (../../../ etc).
    let components: Vec<_> = path.components().collect();
    let parent_count = components
        .iter()
        .filter(|c| matches!(c, std::path::Component::ParentDir))
        .count();

    if parent_count >= 3 {
        return Some(ToolResult::error(
            "Access denied: excessive path traversal detected".to_string(),
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_absolute() {
        let result = resolve_path("/tmp/test.txt", Path::new("/home/user"));
        assert_eq!(result, PathBuf::from("/tmp/test.txt"));
    }

    #[test]
    fn test_resolve_relative() {
        let result = resolve_path("src/main.rs", Path::new("/home/user/project"));
        assert_eq!(result, PathBuf::from("/home/user/project/src/main.rs"));
    }

    #[test]
    fn test_sensitive_path_detected() {
        let result = check_path_safety(Path::new("/etc/passwd"));
        assert!(result.is_some());
        assert!(result.unwrap().is_error);
    }

    #[test]
    fn test_ssh_path_detected() {
        let result = check_path_safety(Path::new("/home/user/.ssh/id_rsa"));
        assert!(result.is_some());
    }

    #[test]
    fn test_traversal_detected() {
        let result = check_path_safety(Path::new("../../../etc/passwd"));
        assert!(result.is_some());
    }

    #[test]
    fn test_safe_path_ok() {
        let result = check_path_safety(Path::new("/home/user/project/src/main.rs"));
        assert!(result.is_none());
    }
}
