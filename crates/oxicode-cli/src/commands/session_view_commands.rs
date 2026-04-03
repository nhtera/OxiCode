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
        "Authenticate with API provider"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let provider = if args.trim().is_empty() {
            &ctx.provider_name
        } else {
            args.trim()
        };

        // Check if key is already set in environment.
        let env_key = match provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "google" | "gemini" => "GOOGLE_API_KEY",
            _ => {
                return CommandOutput::Message(format!(
                    "Unknown provider: {provider}\n\
                     Supported: anthropic, openai, google"
                ));
            }
        };

        if std::env::var(env_key).is_ok() {
            CommandOutput::Message(format!(
                "Already authenticated with {provider} (via {env_key})."
            ))
        } else {
            let config_path = dirs::home_dir()
                .map(|h| h.join(".oxicode").join("credentials.toml"))
                .unwrap_or_default();
            CommandOutput::Message(format!(
                "Not authenticated with {provider}.\n\
                 Set {env_key} environment variable, or add to:\n  {}",
                config_path.display()
            ))
        }
    }
}

pub struct LogoutCommand;
impl SlashCommand for LogoutCommand {
    fn name(&self) -> &str {
        "logout"
    }
    fn description(&self) -> &str {
        "Clear stored credentials"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let cred_path = dirs::home_dir()
            .map(|h| h.join(".oxicode").join("credentials.toml"))
            .unwrap_or_default();

        if cred_path.exists() {
            match std::fs::remove_file(&cred_path) {
                Ok(()) => CommandOutput::Message(format!(
                    "Credentials file removed: {}\n\
                     Environment variables (if set) are still active.",
                    cred_path.display()
                )),
                Err(e) => CommandOutput::Error(format!(
                    "Failed to remove credentials: {e}"
                )),
            }
        } else {
            CommandOutput::Message(
                "No stored credentials file found.\n\
                 Unset environment variables (ANTHROPIC_API_KEY, etc.) to fully log out."
                    .into(),
            )
        }
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

pub struct ForkCommand;
impl SlashCommand for ForkCommand {
    fn name(&self) -> &str {
        "fork"
    }
    fn description(&self) -> &str {
        "Fork an agent in an isolated worktree"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Error("Usage: /fork <branch-name> <task-prompt>".into())
        } else {
            CommandOutput::Message(format!("Forking agent: {args}"))
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
        CommandOutput::Message(
            "Report issues: https://github.com/nicktien007/oxicode/issues".into(),
        )
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
        CommandOutput::Message("Docs: https://github.com/nicktien007/oxicode".into())
    }
}
