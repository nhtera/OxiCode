//! Git worktree tools: create and exit isolated git worktrees.
//!
//! Uses git2 to create worktrees for parallel development in separate branches.
//! Each worktree gets its own directory and can be merged back or discarded.

use std::path::PathBuf;

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Create a git worktree for isolated parallel work.
pub struct EnterWorktreeTool;

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str {
        "enter_worktree"
    }
    fn description(&self) -> &str {
        "Create an isolated git worktree on a new branch for parallel development."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "branch": {
                        "type": "string",
                        "description": "Branch name for the worktree"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory path for the worktree (default: auto-generated)"
                    }
                },
                "required": ["branch"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let branch = match input.get("branch").and_then(|v| v.as_str()) {
            Some(b) => b,
            None => return Ok(ToolResult::error("branch is required")),
        };

        let repo = match git2::Repository::discover(&ctx.working_dir) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Not a git repo: {e}"))),
        };

        // Determine worktree path.
        let worktree_path: PathBuf = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => PathBuf::from(p),
            None => {
                let parent = repo
                    .workdir()
                    .unwrap_or(ctx.working_dir.as_path())
                    .parent()
                    .unwrap_or(ctx.working_dir.as_path());
                parent.join(format!(".worktrees/{branch}"))
            }
        };

        // Create parent directory.
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        // Create the worktree with a new branch.
        let head = match repo.head() {
            Ok(h) => h,
            Err(e) => return Ok(ToolResult::error(format!("Cannot read HEAD: {e}"))),
        };

        let commit = match head.peel_to_commit() {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Cannot peel to commit: {e}"))),
        };

        // Create branch pointing at HEAD.
        if let Err(e) = repo.branch(branch, &commit, false) {
            return Ok(ToolResult::error(format!("Cannot create branch '{branch}': {e}")));
        }

        // Add worktree.
        let reference = format!("refs/heads/{branch}");
        if let Err(e) = repo.worktree(
            branch,
            worktree_path.as_path(),
            Some(git2::WorktreeAddOptions::new().reference(
                repo.find_reference(&reference)
                    .ok()
                    .as_ref(),
            )),
        ) {
            return Ok(ToolResult::error(format!("Cannot create worktree: {e}")));
        }

        Ok(ToolResult::success(format!(
            "Created worktree at {} on branch '{branch}'",
            worktree_path.display()
        )))
    }
}

/// Exit a git worktree, optionally merging changes back.
pub struct ExitWorktreeTool;

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        "exit_worktree"
    }
    fn description(&self) -> &str {
        "Exit a git worktree. Optionally merge the worktree branch back and clean up."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "worktree_path": {
                        "type": "string",
                        "description": "Path to the worktree directory"
                    },
                    "merge": {
                        "type": "boolean",
                        "description": "Whether to merge the worktree branch back (default: false)"
                    },
                    "discard": {
                        "type": "boolean",
                        "description": "Discard the worktree and its branch (default: false)"
                    }
                },
                "required": ["worktree_path"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let worktree_path = match input.get("worktree_path").and_then(|v| v.as_str()) {
            Some(p) => PathBuf::from(p),
            None => return Ok(ToolResult::error("worktree_path is required")),
        };

        let discard = input.get("discard").and_then(|v| v.as_bool()).unwrap_or(false);

        let repo = match git2::Repository::discover(&ctx.working_dir) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Not a git repo: {e}"))),
        };

        // Find the worktree name from the path.
        let worktree_name = worktree_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Clean up: remove the worktree directory.
        if worktree_path.exists() {
            if let Err(e) = std::fs::remove_dir_all(&worktree_path) {
                return Ok(ToolResult::error(format!(
                    "Cannot remove worktree dir: {e}"
                )));
            }
        }

        // Prune the worktree reference from git.
        if let Ok(wt) = repo.find_worktree(worktree_name) {
            if wt.validate().is_err() {
                let _ = wt.prune(Some(
                    git2::WorktreePruneOptions::new()
                        .working_tree(true)
                        .valid(false)
                        .locked(false),
                ));
            }
        }

        // Delete the branch if discarding.
        if discard {
            if let Ok(mut branch) = repo.find_branch(worktree_name, git2::BranchType::Local) {
                let _ = branch.delete();
            }
            return Ok(ToolResult::success(format!(
                "Discarded worktree '{worktree_name}' and deleted branch"
            )));
        }

        Ok(ToolResult::success(format!(
            "Removed worktree '{worktree_name}'. Branch preserved for manual merge."
        )))
    }
}
