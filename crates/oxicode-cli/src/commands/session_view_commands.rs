//! Session-oriented view commands: /login, /logout, /skill, /cron, /schedule,
//! /worktree, /provider, /context-window, /edit-config, /reset, /retry, /fork,
//! /system-prompt, /approve, /reject, /feedback, /docs.

use super::git_helpers::run_command;
use super::{CommandContext, CommandOutput, SlashCommand};

pub struct LoginCommand;
impl SlashCommand for LoginCommand {
    fn name(&self) -> &str {
        "login"
    }
    fn description(&self) -> &str {
        "Authenticate with Anthropic via OAuth (browser-based)"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let provider = if args.trim().is_empty() {
            &ctx.provider_name
        } else {
            args.trim()
        };

        // Only Anthropic supports OAuth PKCE for now.
        if provider != "anthropic" {
            let env_key = match provider {
                "openai" => "OPENAI_API_KEY",
                "google" | "gemini" => "GOOGLE_API_KEY",
                _ => {
                    return CommandOutput::Error(format!(
                        "Unknown provider: {provider}\n\
                         Supported: anthropic (OAuth), openai, google (API key)"
                    ));
                }
            };
            let config_path = dirs::home_dir()
                .map(|h| h.join(".oxicode").join("credentials.toml"))
                .unwrap_or_default();
            return CommandOutput::Message(format!(
                "OAuth not available for {provider}.\n\
                 Set {env_key} env var, or add to:\n  {}",
                config_path.display()
            ));
        }

        // Check if already logged in via OAuth.
        if crate::oauth::is_logged_in() {
            return CommandOutput::Message(
                "Already logged in via OAuth.\n\
                 Use /logout first to re-authenticate."
                    .into(),
            );
        }

        // Check if already authenticated via API key.
        let mgr = crate::auth::AuthManager::new();
        if mgr.is_authenticated("anthropic") {
            return CommandOutput::Message(
                "Already authenticated with API key.\n\
                 Use /login to switch to OAuth (browser-based) login."
                    .into(),
            );
        }

        // Trigger OAuth PKCE flow. Since execute() is sync but login() is async,
        // spawn the async task on the tokio runtime.
        let handle = tokio::runtime::Handle::try_current();
        match handle {
            Ok(_rt) => {
                // We're inside a tokio context — spawn the login and report result.
                // Use spawn_blocking to avoid blocking the event loop.
                let result = std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().ok()?;
                    rt.block_on(crate::oauth::login()).ok()
                })
                .join()
                .ok()
                .flatten();

                match result {
                    Some(login_result) => {
                        let who = login_result.email.as_deref().unwrap_or("Anthropic account");
                        CommandOutput::Message(format!(
                            "Login successful! Authenticated as {who}.\n\
                             OAuth token stored. Restart to use."
                        ))
                    }
                    None => CommandOutput::Error(
                        "OAuth login failed or was cancelled.\n\
                         Check logs for details, or try again."
                            .into(),
                    ),
                }
            }
            Err(_) => CommandOutput::Error(
                "Cannot start OAuth flow — no async runtime available.\n\
                 Try running oxicode interactively."
                    .into(),
            ),
        }
    }
}

pub struct LogoutCommand;
impl SlashCommand for LogoutCommand {
    fn name(&self) -> &str {
        "logout"
    }
    fn description(&self) -> &str {
        "Clear stored OAuth tokens and credentials"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let provider = if args.trim().is_empty() {
            &ctx.provider_name
        } else {
            args.trim()
        };

        let mut results = Vec::new();

        // Clear OAuth tokens (Anthropic only).
        if provider == "anthropic" || args.trim().is_empty() {
            match crate::oauth::logout() {
                Ok(()) => results.push("OAuth tokens cleared.".to_string()),
                Err(e) => results.push(format!("OAuth logout error: {e}")),
            }
        }

        // Clear credential store entry.
        let mut mgr = crate::auth::AuthManager::new();
        match mgr.clear_credential(provider) {
            Ok(()) => results.push(format!("Credentials cleared for {provider}.")),
            Err(e) => results.push(format!("Credential clear error: {e}")),
        }

        results.push("Environment variables (if set) are still active.".into());
        CommandOutput::Message(results.join("\n"))
    }
}

pub struct SkillCommand;
impl SlashCommand for SkillCommand {
    fn name(&self) -> &str {
        "skill"
    }
    fn description(&self) -> &str {
        "Install or manage skills"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub.trim() {
            "install" => CommandOutput::Message(format!("Installing skill: {rest}")),
            "list" | "" => {
                let state = ctx.state_store.current();
                if state.active_skills.is_empty() {
                    CommandOutput::Message("No active skills. Skills in ~/.oxicode/skills/".into())
                } else {
                    let list = state.active_skills.join("\n  ");
                    CommandOutput::Message(format!("Active skills:\n  {list}"))
                }
            }
            other => CommandOutput::Error(format!("Unknown: /skill {other}. Use: install, list")),
        }
    }
}

pub struct CronCommand;
impl SlashCommand for CronCommand {
    fn name(&self) -> &str {
        "cron"
    }
    fn description(&self) -> &str {
        "Manage cron schedules"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        match args.trim() {
            "list" | "" => CommandOutput::Message("No active cron jobs.".into()),
            _ => CommandOutput::Message(format!("Cron operation: {args}")),
        }
    }
}

pub struct ScheduleCommand;
impl SlashCommand for ScheduleCommand {
    fn name(&self) -> &str {
        "schedule"
    }
    fn description(&self) -> &str {
        "Schedule a recurring remote agent"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message("Usage: /schedule <cron-expr> <prompt>".into())
        } else {
            CommandOutput::Message(format!("Schedule created: {args}"))
        }
    }
}

pub struct WorktreeCommand;
impl SlashCommand for WorktreeCommand {
    fn name(&self) -> &str {
        "worktree"
    }
    fn description(&self) -> &str {
        "Manage git worktrees"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub.trim() {
            "create" => CommandOutput::Message(format!("Creating worktree: {rest}")),
            "list" => match run_command("git", &["worktree", "list"]) {
                Ok(out) => CommandOutput::Message(format!("Worktrees:\n{out}")),
                Err(e) => CommandOutput::Error(format!("Failed: {e}")),
            },
            "exit" => CommandOutput::Message("Exiting worktree...".into()),
            "" => CommandOutput::Message("Usage: /worktree <create|list|exit>".into()),
            other => CommandOutput::Error(format!("Unknown: /worktree {other}")),
        }
    }
}

pub struct ProviderCommand;
impl SlashCommand for ProviderCommand {
    fn name(&self) -> &str {
        "provider"
    }
    fn description(&self) -> &str {
        "Show or switch LLM provider"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message(format!("Current provider: {}", ctx.provider_name))
        } else {
            CommandOutput::Message(format!("Switching provider to: {args}"))
        }
    }
}

pub struct ContextWindowCommand;
impl SlashCommand for ContextWindowCommand {
    fn name(&self) -> &str {
        "context-window"
    }
    fn description(&self) -> &str {
        "Show context window details"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(format!(
            "Model: {}\nContext: check /context for usage details",
            ctx.model
        ))
    }
}

pub struct EditConfigCommand;
impl SlashCommand for EditConfigCommand {
    fn name(&self) -> &str {
        "edit-config"
    }
    fn description(&self) -> &str {
        "Open config file in editor"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let path = dirs::home_dir()
            .map(|h| h.join(".oxicode").join("settings.toml"))
            .unwrap_or_default();
        CommandOutput::Message(format!("Edit config at: {}", path.display()))
    }
}

pub struct ResetCommand;
impl SlashCommand for ResetCommand {
    fn name(&self) -> &str {
        "reset"
    }
    fn description(&self) -> &str {
        "Reset session state"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        ctx.state_store.clear_messages();
        CommandOutput::Message("Session state reset.".into())
    }
}

pub struct RetryCommand;
impl SlashCommand for RetryCommand {
    fn name(&self) -> &str {
        "retry"
    }
    fn description(&self) -> &str {
        "Retry the last query"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Retrying last query...".into())
    }
}

/// `/fork [title]` — fork the current session at the current turn.
///
/// Creates a new session branch starting from the last message in the active
/// conversation. Title defaults to `"branch-{short_uuid}"`.
///
/// The TUI engine loop handles the actual `SessionTree::fork` call after
/// receiving the `UiEvent::SlashCommand { name: "fork", args: title }` event.
/// This struct registers the command for autocomplete and `/help`.
pub struct ForkCommand;
impl SlashCommand for ForkCommand {
    fn name(&self) -> &str {
        "fork"
    }
    fn description(&self) -> &str {
        "Fork session at current turn into a new branch (/fork [title])"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        // Build a title: use args if provided, else "branch-{short_id}".
        let title = if args.trim().is_empty() {
            let short_id: String = uuid::Uuid::new_v4().to_string().chars().take(8).collect();
            format!("branch-{short_id}")
        } else {
            args.trim().to_string()
        };

        // Load the current session tree (or legacy session) and fork.
        let session_id = &ctx.session_id;

        // Try to load existing tree from disk.
        let tree_result = oxicode_session::branch::SessionTree::load_or_migrate(session_id, None);

        match tree_result {
            Ok(mut tree) => {
                let current_branch = tree.current;
                let msg_count = tree
                    .branches
                    .get(&current_branch)
                    .map(|b| b.messages.len())
                    .unwrap_or(0);

                // Also include in-memory messages from state that may not be persisted yet.
                let state_msg_count = ctx.state_store.current().messages.len();
                let effective_count = msg_count.max(state_msg_count);

                if effective_count == 0 {
                    return CommandOutput::Message(
                        "Cannot fork an empty session — send at least one message first.".into(),
                    );
                }

                let at_turn = (effective_count - 1) as u32;
                match tree.fork(current_branch, at_turn, title.clone()) {
                    Ok(new_branch_id) => {
                        // Persist the updated tree.
                        if let Err(e) = tree.save(None) {
                            return CommandOutput::Error(format!(
                                "Fork created in memory but save failed: {e}"
                            ));
                        }
                        let short = new_branch_id
                            .to_string()
                            .chars()
                            .take(8)
                            .collect::<String>();
                        CommandOutput::Message(format!(
                            "Forked at turn {at_turn} → branch \"{title}\" ({short})\n\
                             Use /branches to switch between branches."
                        ))
                    }
                    Err(e) => CommandOutput::Error(format!("Fork failed: {e}")),
                }
            }
            Err(_) => {
                // Session not on disk yet — provide guidance.
                CommandOutput::Message(format!(
                    "Session not yet saved. Fork will be created as \"{title}\" \
                     on next save.\nSend a message first to establish a fork point."
                ))
            }
        }
    }
}

/// `/branches` — list all branches of the current session.
pub struct BranchesCommand;
impl SlashCommand for BranchesCommand {
    fn name(&self) -> &str {
        "branches"
    }
    fn description(&self) -> &str {
        "List all branches of the current session tree"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let session_id = &ctx.session_id;
        match oxicode_session::branch::SessionTree::load_or_migrate(session_id, None) {
            Ok(tree) => {
                if tree.branches.len() <= 1 {
                    return CommandOutput::Message(
                        "Only one branch (main). Use /fork to create a branch.".into(),
                    );
                }

                let mut lines = vec!["Branches:".to_string()];
                // Collect and sort: root first, then by created_at.
                let mut sorted: Vec<_> = tree.branches.values().collect();
                sorted.sort_by(|a, b| {
                    // Root (no parent) first, then creation order.
                    match (a.parent, b.parent) {
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        _ => a.created_at.cmp(&b.created_at),
                    }
                });

                for branch in sorted {
                    let active = if branch.id == tree.current {
                        " *"
                    } else {
                        "  "
                    };
                    let short = branch.id.to_string().chars().take(8).collect::<String>();
                    let fork_info = branch
                        .parent_turn
                        .map(|t| format!(" (fork at turn {t})"))
                        .unwrap_or_default();
                    lines.push(format!(
                        "{active} [{short}] \"{}\" — {} msgs{}",
                        branch.title,
                        branch.messages.len(),
                        fork_info,
                    ));
                }
                lines.push(String::new());
                lines.push(
                    "Use /fork to create a branch. Open TUI branches overlay with Ctrl+B.".into(),
                );
                CommandOutput::Message(lines.join("\n"))
            }
            Err(_) => {
                CommandOutput::Message("No session branches found. Use /fork to create one.".into())
            }
        }
    }
}

pub struct SystemPromptCommand;
impl SlashCommand for SystemPromptCommand {
    fn name(&self) -> &str {
        "system-prompt"
    }
    fn description(&self) -> &str {
        "View or modify the system prompt"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "System prompt: configured via CLAUDE.md / OXICODE.md\n\
             Edit project root CLAUDE.md or ~/.oxicode/CLAUDE.md"
                .into(),
        )
    }
}

pub struct ApproveCommand;
impl SlashCommand for ApproveCommand {
    fn name(&self) -> &str {
        "approve"
    }
    fn description(&self) -> &str {
        "Approve pending plan or action"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("No pending approvals.".into())
    }
}

pub struct RejectCommand;
impl SlashCommand for RejectCommand {
    fn name(&self) -> &str {
        "reject"
    }
    fn description(&self) -> &str {
        "Reject pending plan or action"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("No pending items to reject.".into())
    }
}

pub struct FeedbackCommand;
impl SlashCommand for FeedbackCommand {
    fn name(&self) -> &str {
        "feedback"
    }
    fn description(&self) -> &str {
        "Send feedback"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Report issues: https://github.com/nhtera/oxicode/issues".into())
    }
}

pub struct DocsCommand;
impl SlashCommand for DocsCommand {
    fn name(&self) -> &str {
        "docs"
    }
    fn description(&self) -> &str {
        "Open documentation"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Docs: https://github.com/nhtera/oxicode".into())
    }
}
