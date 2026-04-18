//! Fork agent mode: spawn an agent in an isolated git worktree.
//!
//! Creates a temporary git worktree on a new branch, spawns a subagent in that
//! directory, and merges (or discards) changes when the agent finishes.

use std::path::{Path, PathBuf};

use oxicode_common::{OxiError, OxiResult};
use tracing::{info, warn};

use crate::spawner::{spawn_agent, AgentConfig, AgentResult};

/// Outcome of a fork agent run.
#[derive(Debug)]
pub struct ForkResult {
    pub agent_result: AgentResult,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    /// Whether the worktree branch was merged back.
    pub merged: bool,
}

/// Configuration for a forked agent.
#[derive(Debug, Clone)]
pub struct ForkConfig {
    /// Branch name for the worktree.
    pub branch: String,
    /// Agent prompt (task description).
    pub prompt: String,
    /// Model to use.
    pub model: String,
    /// Timeout in seconds.
    pub timeout_secs: u64,
    /// Whether to auto-merge on success.
    pub auto_merge: bool,
}

/// Run a forked agent: create worktree → spawn agent → merge/discard.
pub async fn run_fork_agent(repo_path: &Path, config: &ForkConfig) -> OxiResult<ForkResult> {
    let repo = git2::Repository::discover(repo_path)
        .map_err(|e| OxiError::Other(format!("Not a git repo: {e}")))?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| OxiError::Other("Bare repository not supported".into()))?;

    // Create worktree path as sibling dir.
    let worktree_path = workdir
        .parent()
        .unwrap_or(workdir)
        .join(format!(".oxicode-worktrees/{}", config.branch));

    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create branch at HEAD.
    let head = repo
        .head()
        .map_err(|e| OxiError::Other(format!("Cannot read HEAD: {e}")))?;
    let commit = head
        .peel_to_commit()
        .map_err(|e| OxiError::Other(format!("Cannot peel HEAD: {e}")))?;

    repo.branch(&config.branch, &commit, false)
        .map_err(|e| OxiError::Other(format!("Cannot create branch '{}': {e}", config.branch)))?;

    // Create worktree.
    let reference = format!("refs/heads/{}", config.branch);
    let branch_ref = repo
        .find_reference(&reference)
        .map_err(|e| OxiError::Other(format!("Cannot find ref: {e}")))?;

    repo.worktree(
        &config.branch,
        &worktree_path,
        Some(git2::WorktreeAddOptions::new().reference(Some(&branch_ref))),
    )
    .map_err(|e| OxiError::Other(format!("Cannot create worktree: {e}")))?;

    info!(branch = %config.branch, path = %worktree_path.display(), "Fork worktree created");

    // Spawn agent in the worktree directory.
    let agent_config = AgentConfig {
        name: format!("fork-{}", config.branch),
        prompt: config.prompt.clone(),
        model: config.model.clone(),
        working_dir: worktree_path.clone(),
        permission_mode: "default".to_string(),
        timeout: std::time::Duration::from_secs(config.timeout_secs),
        inherit_env: true,
        agent_type: None,
        allowed_tools: None,
        model_override: false,
    };

    let agent_result = spawn_agent(&agent_config, None).await?;

    // Decide merge vs discard.
    let merged = if agent_result.is_error {
        warn!(branch = %config.branch, "Fork agent failed — discarding worktree");
        cleanup_worktree(&repo, &config.branch, &worktree_path, true)?;
        false
    } else if config.auto_merge {
        info!(branch = %config.branch, "Fork agent succeeded — merging");
        // Merge is left to the caller (may require conflict resolution).
        // We just clean up the worktree, keeping the branch for merge.
        cleanup_worktree(&repo, &config.branch, &worktree_path, false)?;
        true
    } else {
        info!(branch = %config.branch, "Fork agent succeeded — preserving branch");
        cleanup_worktree(&repo, &config.branch, &worktree_path, false)?;
        false
    };

    Ok(ForkResult {
        agent_result,
        branch_name: config.branch.clone(),
        worktree_path,
        merged,
    })
}

/// Remove worktree directory and optionally delete the branch.
fn cleanup_worktree(
    repo: &git2::Repository,
    branch_name: &str,
    worktree_path: &Path,
    delete_branch: bool,
) -> OxiResult<()> {
    // Remove worktree directory.
    if worktree_path.exists() {
        std::fs::remove_dir_all(worktree_path)?;
    }

    // Prune worktree reference.
    if let Ok(wt) = repo.find_worktree(branch_name) {
        let _ = wt.prune(Some(
            git2::WorktreePruneOptions::new()
                .working_tree(true)
                .valid(false)
                .locked(false),
        ));
    }

    // Delete the branch if requested.
    if delete_branch {
        if let Ok(mut branch) = repo.find_branch(branch_name, git2::BranchType::Local) {
            let _ = branch.delete();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fork_config() {
        let cfg = ForkConfig {
            branch: "test-fork".to_string(),
            prompt: "do stuff".to_string(),
            model: "claude-sonnet-4".to_string(),
            timeout_secs: 60,
            auto_merge: false,
        };
        assert_eq!(cfg.branch, "test-fork");
        assert!(!cfg.auto_merge);
    }
}
