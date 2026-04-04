//! GitHub service integration via `gh` CLI.
//!
//! Provides structured access to GitHub repositories: issues, PRs, comments,
//! and repository metadata. Falls back gracefully when `gh` CLI is unavailable.

use std::process::Command;

/// Result of a GitHub API operation.
#[derive(Debug)]
pub enum GhResult {
    /// Successful operation with output text.
    Ok(String),
    /// GitHub CLI not installed.
    NotInstalled,
    /// Not authenticated with GitHub.
    NotAuthenticated,
    /// Operation-specific error.
    Error(String),
}

/// Check if `gh` CLI is available and authenticated.
pub fn check_gh_status() -> GhResult {
    match Command::new("gh").args(["auth", "status"]).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                GhResult::Ok(format!("{stdout}{stderr}").trim().to_string())
            } else if stderr.contains("not logged") || stderr.contains("no oauth") {
                GhResult::NotAuthenticated
            } else {
                GhResult::Error(stderr)
            }
        }
        Err(_) => GhResult::NotInstalled,
    }
}

/// Create a GitHub issue with title and body.
pub fn create_issue(title: &str, body: &str) -> GhResult {
    run_gh(&["issue", "create", "--title", title, "--body", body])
}

/// List open issues (up to `limit`).
pub fn list_issues(limit: u32) -> GhResult {
    let limit_str = limit.to_string();
    run_gh(&["issue", "list", "--limit", &limit_str])
}

/// Get a specific issue by number.
pub fn get_issue(number: u32) -> GhResult {
    let num_str = number.to_string();
    run_gh(&["issue", "view", &num_str])
}

/// Get PR review comments.
pub fn get_pr_comments(pr_number: u32) -> GhResult {
    let num_str = pr_number.to_string();
    run_gh(&["pr", "view", &num_str, "--comments"])
}

/// Get PR diff (review changes).
pub fn get_pr_diff(pr_number: u32) -> GhResult {
    let num_str = pr_number.to_string();
    run_gh(&["pr", "diff", &num_str])
}

/// List open PRs.
pub fn list_prs(limit: u32) -> GhResult {
    let limit_str = limit.to_string();
    run_gh(&["pr", "list", "--limit", &limit_str])
}

/// Get repository metadata (name, description, visibility).
pub fn repo_info() -> GhResult {
    run_gh(&[
        "repo",
        "view",
        "--json",
        "name,description,visibility,defaultBranchRef",
    ])
}

/// List repository branches (requires `gh` CLI with repo context).
pub fn list_branches() -> GhResult {
    run_gh(&["api", "repos/{owner}/{repo}/branches", "--jq", ".[].name"])
}

/// Create a PR with title and body.
pub fn create_pr(title: &str, body: &str) -> GhResult {
    run_gh(&["pr", "create", "--title", title, "--body", body])
}

/// Run a `gh` CLI command and classify the result.
fn run_gh(args: &[&str]) -> GhResult {
    match Command::new("gh").args(args).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

            if output.status.success() {
                GhResult::Ok(if stdout.is_empty() { stderr } else { stdout })
            } else if stderr.contains("not logged")
                || stderr.contains("auth login")
                || stderr.contains("no oauth")
            {
                GhResult::NotAuthenticated
            } else {
                let msg = if stderr.is_empty() { stdout } else { stderr };
                GhResult::Error(msg)
            }
        }
        Err(_) => GhResult::NotInstalled,
    }
}

/// Format a `GhResult` into a user-friendly message.
pub fn format_result(result: GhResult, success_prefix: &str) -> Result<String, String> {
    match result {
        GhResult::Ok(output) => Ok(format!("{success_prefix}{output}")),
        GhResult::NotInstalled => Err("gh CLI not found. Install: https://cli.github.com".into()),
        GhResult::NotAuthenticated => {
            Err("Not authenticated with GitHub. Run: gh auth login".into())
        }
        GhResult::Error(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_result_ok() {
        let result = GhResult::Ok("https://github.com/user/repo/issues/42".into());
        let formatted = format_result(result, "Created: ");
        assert!(formatted.is_ok());
        assert!(formatted.unwrap().starts_with("Created: "));
    }

    #[test]
    fn test_format_result_not_installed() {
        let result = GhResult::NotInstalled;
        let formatted = format_result(result, "");
        assert!(formatted.is_err());
        assert!(formatted.unwrap_err().contains("gh CLI not found"));
    }

    #[test]
    fn test_format_result_not_authenticated() {
        let result = GhResult::NotAuthenticated;
        let formatted = format_result(result, "");
        assert!(formatted.is_err());
        assert!(formatted.unwrap_err().contains("Not authenticated"));
    }

    #[test]
    fn test_format_result_error() {
        let result = GhResult::Error("some error".into());
        let formatted = format_result(result, "");
        assert!(formatted.is_err());
        assert_eq!(formatted.unwrap_err(), "some error");
    }
}
