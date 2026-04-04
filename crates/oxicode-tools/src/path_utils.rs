use std::path::{Path, PathBuf};

use crate::tool_trait::ToolResult;

/// Resolve a path, making relative paths absolute against working_dir.
/// Canonicalizes the result to resolve symlinks and `..` components.
pub fn resolve_path(file_path: &str, working_dir: &Path) -> PathBuf {
    let p = Path::new(file_path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    };
    // Canonicalize if the path exists (resolves symlinks + ..)
    // For non-existent paths, canonicalize the parent and append the filename
    if let Ok(canonical) = joined.canonicalize() {
        canonical
    } else if let (Some(parent), Some(name)) = (joined.parent(), joined.file_name()) {
        if let Ok(canonical_parent) = parent.canonicalize() {
            canonical_parent.join(name)
        } else {
            joined
        }
    } else {
        joined
    }
}

/// Sensitive paths that tools should never write to.
const SENSITIVE_PATHS: &[&str] = &[
    "/etc/passwd",
    "/etc/shadow",
    "/etc/sudoers",
    "/etc/hosts",
    "/.ssh/",
    "/.gnupg/",
    "/.aws/",
    "/.kube/",
    "/.npmrc",
    "/.docker/",
    "/proc/",
    "/sys/",
    "/dev/",
    "/boot/",
    "/var/run/",
    "/var/spool/cron",
];

/// Check if a resolved path appears to be targeting a sensitive system location.
/// Returns an error ToolResult if the path is unsafe.
pub fn check_path_safety(path: &Path) -> Option<ToolResult> {
    let path_str = path.to_string_lossy();

    for sensitive in SENSITIVE_PATHS {
        if path_str.contains(sensitive) {
            return Some(ToolResult::error(format!(
                "Access denied: path targets sensitive location ({sensitive})"
            )));
        }
    }

    // Reject any remaining .. components after canonicalization
    // (only present if canonicalize() failed for non-existent paths)
    let has_parent_dir = path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    if has_parent_dir {
        return Some(ToolResult::error(
            "Access denied: path traversal detected".to_string(),
        ));
    }

    None
}

/// Check if a path is within the working directory boundary.
/// Returns an error ToolResult if the path escapes the workspace.
pub fn check_workspace_boundary(path: &Path, working_dir: &Path) -> Option<ToolResult> {
    let canonical_wd = working_dir
        .canonicalize()
        .unwrap_or_else(|_| working_dir.to_path_buf());
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    // Allow paths within the working directory
    if canonical_path.starts_with(&canonical_wd) {
        return None;
    }

    // Allow common system temp directories (including macOS canonical paths)
    let mut allowed_prefixes = vec!["/tmp", "/var/tmp", "/private/tmp", "/private/var/folders"];
    // Also allow the OS-reported temp dir
    let sys_tmp = std::env::temp_dir();
    let sys_tmp_str = sys_tmp.to_string_lossy().to_string();
    allowed_prefixes.push(&sys_tmp_str);
    for prefix in &allowed_prefixes {
        if canonical_path.starts_with(prefix) {
            return None;
        }
    }

    // Allow home directory (for config files like ~/.oxicode)
    // Sensitive sub-paths are already caught by check_path_safety above.
    if let Some(home) = dirs::home_dir() {
        if canonical_path.starts_with(&home) {
            return None;
        }
    }

    Some(ToolResult::error(format!(
        "Access denied: path {} is outside the working directory",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_absolute() {
        let result = resolve_path("/tmp/test.txt", Path::new("/home/user"));
        // On macOS, /tmp canonicalizes to /private/tmp
        let expected_suffix = "tmp/test.txt";
        assert!(
            result.to_string_lossy().ends_with(expected_suffix),
            "Expected path ending with {expected_suffix}, got {}",
            result.display()
        );
    }

    #[test]
    fn test_resolve_relative() {
        // Use a real directory so canonicalize works
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        let result = resolve_path("main.rs", dir.path());
        assert!(result.ends_with("main.rs"));
        // Should be canonicalized (absolute, no ..)
        assert!(result.is_absolute());
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
    fn test_aws_path_detected() {
        let result = check_path_safety(Path::new("/home/user/.aws/credentials"));
        assert!(result.is_some());
    }

    #[test]
    fn test_proc_path_detected() {
        let result = check_path_safety(Path::new("/proc/self/environ"));
        assert!(result.is_some());
    }

    #[test]
    fn test_traversal_detected() {
        // After canonicalization, .. should be resolved. But if canonicalize
        // fails (non-existent path), remaining .. is caught.
        let result = check_path_safety(Path::new("/nonexistent/../../../etc/passwd"));
        // Contains /etc/passwd — caught by sensitive path check
        assert!(result.is_some());
    }

    #[test]
    fn test_safe_path_ok() {
        let result = check_path_safety(Path::new("/home/user/project/src/main.rs"));
        assert!(result.is_none());
    }

    #[test]
    fn test_workspace_boundary_inside() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("src/main.rs");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(&file, "").unwrap();
        let result = check_workspace_boundary(&file, dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_workspace_boundary_tmp_allowed() {
        let result = check_workspace_boundary(Path::new("/tmp/test.txt"), Path::new("/home/user"));
        assert!(result.is_none());
    }
}
