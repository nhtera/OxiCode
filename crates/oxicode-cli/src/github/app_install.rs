//! GitHub App install wizard — guided flow for installing OxiCode workflow.
//!
//! Steps: authenticate → list repos → select repo → check existing → generate → commit.

use super::workflow_gen;
use super::{get_github_token, list_user_repos, RepoInfo};

/// Result of the install wizard.
#[derive(Debug)]
pub enum InstallResult {
    /// Successfully installed workflow.
    Success { repo: String, workflow_path: String },
    /// User cancelled.
    Cancelled,
    /// Error during installation.
    Error(String),
}

/// Wizard step status for TUI display.
#[derive(Debug, Clone)]
pub struct WizardStep {
    pub label: String,
    pub status: StepStatus,
}

/// Status of a wizard step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Complete,
    Failed,
    Skipped,
}

impl StepStatus {
    /// Emoji indicator for TUI display.
    pub fn indicator(&self) -> &str {
        match self {
            Self::Pending => "  ",
            Self::InProgress => "> ",
            Self::Complete => "v ",
            Self::Failed => "x ",
            Self::Skipped => "- ",
        }
    }
}

/// Build the list of wizard steps for display.
pub fn wizard_steps() -> Vec<WizardStep> {
    vec![
        WizardStep {
            label: "Authenticate with GitHub".to_string(),
            status: StepStatus::Pending,
        },
        WizardStep {
            label: "List repositories".to_string(),
            status: StepStatus::Pending,
        },
        WizardStep {
            label: "Select target repository".to_string(),
            status: StepStatus::Pending,
        },
        WizardStep {
            label: "Check for existing workflows".to_string(),
            status: StepStatus::Pending,
        },
        WizardStep {
            label: "Generate oxicode.yml".to_string(),
            status: StepStatus::Pending,
        },
        WizardStep {
            label: "Commit workflow to repository".to_string(),
            status: StepStatus::Pending,
        },
    ]
}

/// Run the install wizard non-interactively (for CLI/bridge mode).
///
/// Uses the first available repo if `target_repo` is None.
/// Returns the result of the installation attempt.
pub async fn run_install(target_repo: Option<&str>) -> InstallResult {
    // Step 1: Check GitHub authentication.
    let Some(token) = get_github_token() else {
        return InstallResult::Error(
            "Not authenticated with GitHub.\n\
             Set GITHUB_TOKEN env var or run: gh auth login"
                .to_string(),
        );
    };

    // Step 2: List repos.
    let repos = match list_user_repos(&token).await {
        Ok(r) if r.is_empty() => {
            return InstallResult::Error("No repositories found.".to_string());
        }
        Ok(r) => r,
        Err(e) => return InstallResult::Error(format!("Failed to list repos: {e}")),
    };

    // Step 3: Select target repo.
    let repo = if let Some(name) = target_repo {
        match repos.iter().find(|r| r.full_name == name || r.name == name) {
            Some(r) => r.clone(),
            None => {
                return InstallResult::Error(format!(
                    "Repository '{name}' not found. Available: {}",
                    repos
                        .iter()
                        .map(|r| r.full_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    } else {
        // Non-interactive: use first repo.
        repos.into_iter().next().unwrap()
    };

    // Step 4: Check existing workflows.
    if let Ok(true) = check_existing_workflow(&token, &repo).await {
        tracing::info!(repo = %repo.full_name, "Workflow already exists");
    }

    // Step 5: Generate workflow content.
    let workflow_content = workflow_gen::generate_oxicode_workflow(&repo.default_branch);

    // Step 6: Commit via GitHub API.
    let workflow_path = ".github/workflows/oxicode.yml";
    match commit_workflow(&token, &repo, workflow_path, &workflow_content).await {
        Ok(()) => InstallResult::Success {
            repo: repo.full_name,
            workflow_path: workflow_path.to_string(),
        },
        Err(e) => InstallResult::Error(format!("Failed to commit workflow: {e}")),
    }
}

/// Check if a workflow file already exists in the repo.
async fn check_existing_workflow(token: &str, repo: &RepoInfo) -> Result<bool, String> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://api.github.com/repos/{}/contents/.github/workflows/oxicode.yml",
        repo.full_name
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "oxicode-cli")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("API error: {e}"))?;

    Ok(resp.status().is_success())
}

/// Commit a workflow file to the repo via GitHub Contents API.
async fn commit_workflow(
    token: &str,
    repo: &RepoInfo,
    path: &str,
    content: &str,
) -> Result<(), String> {
    use base64::Engine;

    let client = reqwest::Client::new();
    let url = format!(
        "https://api.github.com/repos/{}/contents/{path}",
        repo.full_name
    );

    let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());

    let body = serde_json::json!({
        "message": "ci: add OxiCode workflow",
        "content": encoded,
        "branch": repo.default_branch
    });

    let resp = client
        .put(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "oxicode-cli")
        .header("Accept", "application/vnd.github+json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {e}"))?;

    if resp.status().is_success() || resp.status().as_u16() == 201 {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("GitHub API ({status}): {body}"))
    }
}

/// Format the install result as a user-friendly message.
pub fn format_install_result(result: &InstallResult) -> String {
    match result {
        InstallResult::Success {
            repo,
            workflow_path,
        } => {
            format!(
                "GitHub App installed successfully!\n\
                 Repository: {repo}\n\
                 Workflow: {workflow_path}\n\
                 The workflow will run on push to the default branch."
            )
        }
        InstallResult::Cancelled => "Installation cancelled.".to_string(),
        InstallResult::Error(e) => format!("Installation failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wizard_steps() {
        let steps = wizard_steps();
        assert_eq!(steps.len(), 6);
        assert!(steps.iter().all(|s| s.status == StepStatus::Pending));
    }

    #[test]
    fn test_step_status_indicator() {
        assert_eq!(StepStatus::Pending.indicator(), "  ");
        assert_eq!(StepStatus::InProgress.indicator(), "> ");
        assert_eq!(StepStatus::Complete.indicator(), "v ");
        assert_eq!(StepStatus::Failed.indicator(), "x ");
        assert_eq!(StepStatus::Skipped.indicator(), "- ");
    }

    #[test]
    fn test_format_success() {
        let result = InstallResult::Success {
            repo: "user/repo".to_string(),
            workflow_path: ".github/workflows/oxicode.yml".to_string(),
        };
        let msg = format_install_result(&result);
        assert!(msg.contains("successfully"));
        assert!(msg.contains("user/repo"));
    }

    #[test]
    fn test_format_cancelled() {
        let result = InstallResult::Cancelled;
        let msg = format_install_result(&result);
        assert!(msg.contains("cancelled"));
    }

    #[test]
    fn test_format_error() {
        let result = InstallResult::Error("auth failed".to_string());
        let msg = format_install_result(&result);
        assert!(msg.contains("auth failed"));
    }
}
