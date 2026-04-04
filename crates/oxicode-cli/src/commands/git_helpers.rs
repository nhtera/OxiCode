//! Shared helpers for git slash commands.

use std::process::Command;

/// Run a git command and return trimmed stdout on success, stderr on failure.
pub fn run_git(args: &[&str]) -> Result<String, String> {
    match Command::new("git").args(args).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if output.status.success() {
                Ok(stdout)
            } else {
                Err(if stderr.is_empty() { stdout } else { stderr })
            }
        }
        Err(_) => Err("git not found. Install git to use this command.".into()),
    }
}

/// Run an arbitrary CLI command (e.g. `gh`).
pub fn run_command(cmd: &str, args: &[&str]) -> Result<String, String> {
    match Command::new(cmd).args(args).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if output.status.success() {
                Ok(stdout)
            } else {
                Err(if stderr.is_empty() { stdout } else { stderr })
            }
        }
        Err(_) => Err(format!("{cmd} not found. Is it installed and in PATH?")),
    }
}

/// Truncate a string to max chars, appending "..." if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{t}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate("hello world!", 8);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn test_run_git_version() {
        // git should be available in CI/dev environments
        let result = run_git(&["--version"]);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("git version"));
    }
}
