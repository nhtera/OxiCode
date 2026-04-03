//! Git slash commands: /commit, /pr, /branch, /log, /stash, /push, /pull.
//!
//! All commands shell out to `git` (or `gh`) CLI via helpers in `git_helpers`.

use super::git_helpers::{run_command, run_git};
use super::{CommandContext, CommandOutput, SlashCommand};

/// /commit [message] — stage all changes and create a git commit.
pub struct CommitCommand;
impl SlashCommand for CommitCommand {
    fn name(&self) -> &str {
        "commit"
    }
    fn description(&self) -> &str {
        "Create a git commit (stages all changes)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        // Stage all tracked + untracked files.
        if let Err(e) = run_git(&["add", "-A"]) {
            return CommandOutput::Error(format!("git add failed: {e}"));
        }

        // Check if there's anything to commit.
        match run_git(&["diff", "--cached", "--stat"]) {
            Ok(stat) if stat.is_empty() => {
                return CommandOutput::Message("Nothing to commit (working tree clean).".into());
            }
            Err(e) => return CommandOutput::Error(format!("git diff failed: {e}")),
            _ => {}
        }

        let result = if args.trim().is_empty() {
            // No message — use a default based on diff stat.
            match run_git(&["diff", "--cached", "--shortstat"]) {
                Ok(shortstat) => run_git(&["commit", "-m", &format!("wip: {shortstat}")]),
                Err(_) => run_git(&["commit", "-m", "wip"]),
            }
        } else {
            run_git(&["commit", "-m", args.trim()])
        };

        match result {
            Ok(out) => CommandOutput::Message(format!("Committed:\n{out}")),
            Err(e) => CommandOutput::Error(format!("Commit failed: {e}")),
        }
    }
}

/// /pr [title] — create a GitHub PR using `gh` CLI.
pub struct PrCommand;
impl SlashCommand for PrCommand {
    fn name(&self) -> &str {
        "pr"
    }
    fn description(&self) -> &str {
        "Create a GitHub pull request (requires gh CLI)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let result = if args.trim().is_empty() {
            run_command("gh", &["pr", "create", "--fill"])
        } else {
            run_command("gh", &["pr", "create", "--title", args.trim(), "--fill-verbose"])
        };
        match result {
            Ok(url) => CommandOutput::Message(format!("PR created: {url}")),
            Err(e) => {
                if e.contains("not found") {
                    CommandOutput::Error(
                        "gh CLI not found. Install: https://cli.github.com".into(),
                    )
                } else {
                    CommandOutput::Error(format!("PR creation failed: {e}"))
                }
            }
        }
    }
}

/// /branch [name] — show current branch or switch/create a branch.
pub struct BranchCommand;
impl SlashCommand for BranchCommand {
    fn name(&self) -> &str {
        "branch"
    }
    fn description(&self) -> &str {
        "Show or switch git branch"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.trim().is_empty() {
            let current = run_git(&["branch", "--show-current"]).unwrap_or_default();
            match run_git(&["branch", "--list"]) {
                Ok(list) => CommandOutput::Message(format!("Current: {current}\n{list}")),
                Err(e) => CommandOutput::Error(format!("Not a git repo: {e}")),
            }
        } else {
            let name = args.trim();
            // Try switching first, then create if it doesn't exist.
            match run_git(&["checkout", name]) {
                Ok(out) => CommandOutput::Message(out),
                Err(_) => match run_git(&["checkout", "-b", name]) {
                    Ok(out) => CommandOutput::Message(out),
                    Err(e) => CommandOutput::Error(format!("Branch switch failed: {e}")),
                },
            }
        }
    }
}

/// /log [count] — show recent git log entries.
pub struct LogCommand;
impl SlashCommand for LogCommand {
    fn name(&self) -> &str {
        "log"
    }
    fn description(&self) -> &str {
        "Show recent git log"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let count = args.trim().parse::<u32>().unwrap_or(10).min(100);
        let count_str = format!("-{count}");
        match run_git(&["log", "--oneline", "--decorate", &count_str]) {
            Ok(log) if log.is_empty() => {
                CommandOutput::Message("No commits yet.".into())
            }
            Ok(log) => CommandOutput::Message(log),
            Err(e) => CommandOutput::Error(format!("git log failed: {e}")),
        }
    }
}

/// /stash [push|pop|list|drop] — git stash operations.
pub struct StashCommand;
impl SlashCommand for StashCommand {
    fn name(&self) -> &str {
        "stash"
    }
    fn description(&self) -> &str {
        "Git stash operations (push/pop/list/drop)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let sub = parts.first().copied().unwrap_or("push");
        let result = match sub {
            "pop" => run_git(&["stash", "pop"]),
            "list" => run_git(&["stash", "list"]),
            "drop" => {
                let index = parts.get(1).unwrap_or(&"0");
                run_git(&["stash", "drop", index])
            }
            // Default action: stash push.
            _ => run_git(&["stash", "push"]),
        };
        match result {
            Ok(out) if out.is_empty() => {
                CommandOutput::Message("Stash operation completed (no output).".into())
            }
            Ok(out) => CommandOutput::Message(out),
            Err(e) => CommandOutput::Error(format!("Stash failed: {e}")),
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["push", "pop", "list", "drop"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

/// /push [remote] [branch] — push to a remote.
pub struct PushCommand;
impl SlashCommand for PushCommand {
    fn name(&self) -> &str {
        "push"
    }
    fn description(&self) -> &str {
        "Push to remote"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let result = match parts.len() {
            0 => run_git(&["push"]),
            1 => run_git(&["push", parts[0]]),
            _ => run_git(&["push", parts[0], parts[1]]),
        };
        match result {
            Ok(out) => {
                let msg = if out.is_empty() {
                    "Pushed successfully.".to_string()
                } else {
                    out
                };
                CommandOutput::Message(msg)
            }
            Err(e) => CommandOutput::Error(format!("Push failed: {e}")),
        }
    }
}

/// /pull [remote] [branch] — pull from a remote.
pub struct PullCommand;
impl SlashCommand for PullCommand {
    fn name(&self) -> &str {
        "pull"
    }
    fn description(&self) -> &str {
        "Pull from remote"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let result = match parts.len() {
            0 => run_git(&["pull"]),
            1 => run_git(&["pull", parts[0]]),
            _ => run_git(&["pull", parts[0], parts[1]]),
        };
        match result {
            Ok(out) => {
                let msg = if out.is_empty() {
                    "Already up to date.".to_string()
                } else {
                    out
                };
                CommandOutput::Message(msg)
            }
            Err(e) => CommandOutput::Error(format!("Pull failed: {e}")),
        }
    }
}

/// /issue [title] — create a GitHub issue using `gh` CLI.
pub struct IssueCommand;
impl SlashCommand for IssueCommand {
    fn name(&self) -> &str {
        "issue"
    }
    fn description(&self) -> &str {
        "Create a GitHub issue (requires gh CLI)"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        if args.trim().is_empty() {
            // List recent issues.
            match run_command("gh", &["issue", "list", "--limit", "10"]) {
                Ok(out) if out.is_empty() => CommandOutput::Message("No open issues.".into()),
                Ok(out) => CommandOutput::Message(format!("Open issues:\n{out}")),
                Err(e) => {
                    if e.contains("not found") {
                        CommandOutput::Error(
                            "gh CLI not found. Install: https://cli.github.com".into(),
                        )
                    } else {
                        CommandOutput::Error(format!("Failed to list issues: {e}"))
                    }
                }
            }
        } else {
            let title = args.trim();
            // Build body from recent conversation context.
            let state = ctx.state_store.current();
            let context_msgs: Vec<String> = state
                .messages
                .iter()
                .rev()
                .take(4)
                .map(|m| {
                    let role = match m.role {
                        oxicode_common::Role::User => "User",
                        oxicode_common::Role::Assistant => "Assistant",
                        oxicode_common::Role::System => "System",
                    };
                    let text = super::git_helpers::truncate(&m.text(), 200);
                    format!("**{role}:** {text}")
                })
                .collect();

            let body = if context_msgs.is_empty() {
                String::from("Created via OxiCode CLI.")
            } else {
                format!(
                    "Created via OxiCode CLI.\n\n## Context\n\n{}",
                    context_msgs.into_iter().rev().collect::<Vec<_>>().join("\n\n")
                )
            };

            match run_command("gh", &["issue", "create", "--title", title, "--body", &body]) {
                Ok(url) => CommandOutput::Message(format!("Issue created: {url}")),
                Err(e) => {
                    if e.contains("not found") {
                        CommandOutput::Error(
                            "gh CLI not found. Install: https://cli.github.com".into(),
                        )
                    } else {
                        CommandOutput::Error(format!("Issue creation failed: {e}"))
                    }
                }
            }
        }
    }
}

/// /pr-comments [pr_number] — show PR review comments.
pub struct PrCommentsCommand;
impl SlashCommand for PrCommentsCommand {
    fn name(&self) -> &str {
        "pr-comments"
    }
    fn description(&self) -> &str {
        "Show PR review comments (requires gh CLI)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let pr_num = args.trim();
        if pr_num.is_empty() {
            return CommandOutput::Error("Usage: /pr-comments <pr_number>".into());
        }
        if !pr_num.chars().all(|c| c.is_ascii_digit()) {
            return CommandOutput::Error("PR number must be numeric.".into());
        }

        match run_command("gh", &["pr", "view", pr_num, "--comments"]) {
            Ok(out) if out.is_empty() => {
                CommandOutput::Message(format!("No comments on PR #{pr_num}."))
            }
            Ok(out) => {
                let lines: Vec<&str> = out.lines().collect();
                let total = lines.len();
                let preview: String = lines.into_iter().take(80).collect::<Vec<_>>().join("\n");
                let suffix = if total > 80 {
                    format!("\n\n... ({total} total lines)")
                } else {
                    String::new()
                };
                CommandOutput::Message(format!("PR #{pr_num} comments:\n{preview}{suffix}"))
            }
            Err(e) => {
                if e.contains("not found") {
                    CommandOutput::Error(
                        "gh CLI not found. Install: https://cli.github.com".into(),
                    )
                } else {
                    CommandOutput::Error(format!("Failed to get PR comments: {e}"))
                }
            }
        }
    }
}
