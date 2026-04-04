//! GitHub App integration — OAuth, repository selection, workflow generation.
//!
//! Provides a guided wizard for installing the OxiCode GitHub App:
//! 1. GitHub OAuth login (separate from Anthropic OAuth)
//! 2. List user repos via GitHub API
//! 3. Generate `.github/workflows/oxicode.yml`
//! 4. Commit workflow via GitHub API

pub mod app_install;
pub mod workflow_gen;

/// GitHub OAuth scopes needed for App installation.
pub const GITHUB_SCOPES: &[&str] = &["repo", "workflow"];

/// GitHub API base URL.
const GITHUB_API_BASE: &str = "https://api.github.com";

/// Check if a GitHub token is available (env var or stored credential).
pub fn get_github_token() -> Option<String> {
    // 1. Environment variable.
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }

    // 2. Stored credential file.
    load_stored_github_token()
}

/// Load GitHub token from credential store.
fn load_stored_github_token() -> Option<String> {
    let cred_path = dirs::home_dir()?.join(".oxicode").join("credentials.toml");
    let content = std::fs::read_to_string(cred_path).ok()?;
    let table: toml::Table = content.parse().ok()?;
    table
        .get("github")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Save GitHub token to credential store.
pub fn save_github_token(token: &str) -> Result<(), String> {
    let dir = dirs::home_dir()
        .ok_or("Cannot determine home directory")?
        .join(".oxicode");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;

    let cred_path = dir.join("credentials.toml");

    // Load existing or start fresh.
    let mut table: toml::Table = if let Ok(content) = std::fs::read_to_string(&cred_path) {
        content.parse().unwrap_or_default()
    } else {
        toml::Table::new()
    };

    table.insert("github".to_string(), toml::Value::String(token.to_string()));

    let content = toml::to_string_pretty(&table)
        .map_err(|e| format!("Failed to serialize credentials: {e}"))?;
    std::fs::write(&cred_path, content).map_err(|e| format!("Failed to write credentials: {e}"))?;

    // Restrict file permissions to owner-only on Unix (0600).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(&cred_path, perms);
    }

    Ok(())
}

/// List user repositories via GitHub API (requires token).
pub async fn list_user_repos(token: &str) -> Result<Vec<RepoInfo>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{GITHUB_API_BASE}/user/repos"))
        .query(&[("sort", "updated"), ("per_page", "30")])
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "oxicode-cli")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub API error ({status}): {body}"));
    }

    let repos: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse repos: {e}"))?;

    Ok(repos
        .into_iter()
        .filter_map(|r| {
            Some(RepoInfo {
                full_name: r["full_name"].as_str()?.to_string(),
                name: r["name"].as_str()?.to_string(),
                default_branch: r["default_branch"].as_str().unwrap_or("main").to_string(),
                private: r["private"].as_bool().unwrap_or(false),
            })
        })
        .collect())
}

/// Basic repository information.
#[derive(Debug, Clone)]
pub struct RepoInfo {
    /// Full name (e.g. "user/repo").
    pub full_name: String,
    /// Short name (e.g. "repo").
    pub name: String,
    /// Default branch name.
    pub default_branch: String,
    /// Whether the repo is private.
    pub private: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_scopes() {
        assert!(GITHUB_SCOPES.contains(&"repo"));
        assert!(GITHUB_SCOPES.contains(&"workflow"));
    }

    #[test]
    fn test_repo_info() {
        let repo = RepoInfo {
            full_name: "user/repo".to_string(),
            name: "repo".to_string(),
            default_branch: "main".to_string(),
            private: false,
        };
        assert_eq!(repo.full_name, "user/repo");
        assert!(!repo.private);
    }

    #[test]
    fn test_get_github_token_returns_none_when_unset() {
        // In test environment, GITHUB_TOKEN is typically not set.
        // This test just verifies the function doesn't panic.
        let _ = get_github_token();
    }
}
