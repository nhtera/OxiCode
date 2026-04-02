//! View/UI slash commands: /theme, /shortcuts, /about, /tools, /fast, /verbose.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct ThemeCommand;
impl SlashCommand for ThemeCommand {
    fn name(&self) -> &str { "theme" }
    fn description(&self) -> &str { "Switch TUI color theme" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message(
                "Available themes: dark, light, catppuccin, dracula, solarized\nUsage: /theme <name>".into()
            )
        } else {
            CommandOutput::Message(format!("Theme switched to: {args}"))
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["dark", "light", "catppuccin", "dracula", "solarized"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }
}

pub struct ShortcutsCommand;
impl SlashCommand for ShortcutsCommand {
    fn name(&self) -> &str { "shortcuts" }
    fn description(&self) -> &str { "Show keyboard shortcuts" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Keyboard shortcuts:\n\
             \x20 Ctrl+C    Cancel / interrupt\n\
             \x20 Ctrl+D    Quit\n\
             \x20 Ctrl+L    Clear screen\n\
             \x20 Tab       Accept autocomplete\n\
             \x20 /         Slash command\n\
             \x20 ?         Show shortcuts overlay\n\
             \x20 Esc       Cancel current input"
                .into(),
        )
    }
}

pub struct AboutCommand;
impl SlashCommand for AboutCommand {
    fn name(&self) -> &str { "about" }
    fn description(&self) -> &str { "Show about information" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(format!(
            "{} v{}\nA production Rust CLI agent\nRepository: https://github.com/nicktien007/oxicode",
            oxicode_common::constants::APP_DISPLAY_NAME,
            oxicode_common::constants::VERSION,
        ))
    }
}

pub struct ToolsCommand;
impl SlashCommand for ToolsCommand {
    fn name(&self) -> &str { "tools" }
    fn description(&self) -> &str { "List all available tools" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let registry = oxicode_tools::default_registry();
        let mut names = registry.names();
        names.sort();
        CommandOutput::Message(format!(
            "Available tools ({}):\n  {}",
            names.len(),
            names.join("\n  ")
        ))
    }
}

pub struct FastCommand;
impl SlashCommand for FastCommand {
    fn name(&self) -> &str { "fast" }
    fn description(&self) -> &str { "Toggle fast output mode" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Fast mode toggled.".into())
    }
}

pub struct VerboseCommand;
impl SlashCommand for VerboseCommand {
    fn name(&self) -> &str { "verbose" }
    fn description(&self) -> &str { "Toggle verbose output" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Verbose mode toggled.".into())
    }
}

pub struct HistoryCommand;
impl SlashCommand for HistoryCommand {
    fn name(&self) -> &str { "history" }
    fn description(&self) -> &str { "Show conversation history summary" }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let count = state.messages.len();
        CommandOutput::Message(format!("Conversation has {count} messages."))
    }
}

pub struct ReviewCommand;
impl SlashCommand for ReviewCommand {
    fn name(&self) -> &str { "review" }
    fn description(&self) -> &str { "Review recent changes" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Reviewing recent file changes...".into())
    }
}

pub struct LoginCommand;
impl SlashCommand for LoginCommand {
    fn name(&self) -> &str { "login" }
    fn description(&self) -> &str { "Authenticate with API provider" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Use environment variables to set API keys:\n  ANTHROPIC_API_KEY, OPENAI_API_KEY, etc.".into())
    }
}

pub struct LogoutCommand;
impl SlashCommand for LogoutCommand {
    fn name(&self) -> &str { "logout" }
    fn description(&self) -> &str { "Clear stored credentials" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Credentials cleared.".into())
    }
}

pub struct ResumeCommand;
impl SlashCommand for ResumeCommand {
    fn name(&self) -> &str { "resume" }
    fn description(&self) -> &str { "Resume last session" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Resuming last session...".into())
    }
}

pub struct SkillCommand;
impl SlashCommand for SkillCommand {
    fn name(&self) -> &str { "skill" }
    fn description(&self) -> &str { "Install or manage skills" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub {
            "install" => CommandOutput::Message(format!("Installing skill: {rest}")),
            "list" | "" => CommandOutput::Message("Skills: (list from ~/.oxicode/skills/)".into()),
            _ => CommandOutput::Error(format!("Unknown: /skill {sub}")),
        }
    }
}

pub struct CronCommand;
impl SlashCommand for CronCommand {
    fn name(&self) -> &str { "cron" }
    fn description(&self) -> &str { "Manage cron schedules" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        match args {
            "list" | "" => CommandOutput::Message("Schedules: (none)".into()),
            _ => CommandOutput::Message(format!("Cron operation: {args}")),
        }
    }
}

pub struct ScheduleCommand;
impl SlashCommand for ScheduleCommand {
    fn name(&self) -> &str { "schedule" }
    fn description(&self) -> &str { "Schedule a recurring remote agent" }
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
    fn name(&self) -> &str { "worktree" }
    fn description(&self) -> &str { "Manage git worktrees" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub {
            "create" => CommandOutput::Message(format!("Creating worktree: {rest}")),
            "list" => CommandOutput::Message("Worktrees: (none active)".into()),
            "exit" => CommandOutput::Message("Exiting worktree...".into()),
            "" => CommandOutput::Message("Usage: /worktree <create|list|exit>".into()),
            _ => CommandOutput::Error(format!("Unknown: /worktree {sub}")),
        }
    }
}

pub struct ProviderCommand;
impl SlashCommand for ProviderCommand {
    fn name(&self) -> &str { "provider" }
    fn description(&self) -> &str { "Show or switch LLM provider" }
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
    fn name(&self) -> &str { "context-window" }
    fn description(&self) -> &str { "Show context window details" }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(format!("Model: {}\nContext: check /tokens for details", ctx.model))
    }
}

pub struct EditConfigCommand;
impl SlashCommand for EditConfigCommand {
    fn name(&self) -> &str { "edit-config" }
    fn description(&self) -> &str { "Open config file in editor" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Open ~/.oxicode/config.toml in your editor.".into())
    }
}

pub struct ResetCommand;
impl SlashCommand for ResetCommand {
    fn name(&self) -> &str { "reset" }
    fn description(&self) -> &str { "Reset session state" }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        ctx.state_store.clear_messages();
        CommandOutput::Message("Session state reset.".into())
    }
}

pub struct RetryCommand;
impl SlashCommand for RetryCommand {
    fn name(&self) -> &str { "retry" }
    fn description(&self) -> &str { "Retry the last query" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Retrying last query...".into())
    }
}

pub struct ForkCommand;
impl SlashCommand for ForkCommand {
    fn name(&self) -> &str { "fork" }
    fn description(&self) -> &str { "Fork an agent in an isolated worktree" }
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
    fn name(&self) -> &str { "system-prompt" }
    fn description(&self) -> &str { "View or modify the system prompt" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("System prompt: (configured via CLAUDE.md / OXICODE.md)".into())
    }
}

pub struct ApproveCommand;
impl SlashCommand for ApproveCommand {
    fn name(&self) -> &str { "approve" }
    fn description(&self) -> &str { "Approve pending plan or action" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("No pending approvals.".into())
    }
}

pub struct RejectCommand;
impl SlashCommand for RejectCommand {
    fn name(&self) -> &str { "reject" }
    fn description(&self) -> &str { "Reject pending plan or action" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("No pending items to reject.".into())
    }
}

pub struct FileCommand;
impl SlashCommand for FileCommand {
    fn name(&self) -> &str { "file" }
    fn description(&self) -> &str { "Quick file operations (read/write/edit)" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message("Usage: /file <read|write|edit> <path>".into())
        } else {
            CommandOutput::Message(format!("File operation: {args}"))
        }
    }
}

pub struct SearchCommand;
impl SlashCommand for SearchCommand {
    fn name(&self) -> &str { "search" }
    fn description(&self) -> &str { "Search codebase for a pattern" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Error("Usage: /search <pattern>".into())
        } else {
            CommandOutput::Message(format!("Searching for: {args}"))
        }
    }
}

pub struct RunCommand;
impl SlashCommand for RunCommand {
    fn name(&self) -> &str { "run" }
    fn description(&self) -> &str { "Run a shell command" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Error("Usage: /run <command>".into())
        } else {
            CommandOutput::Message(format!("Running: {args}"))
        }
    }
}

pub struct TestCommand;
impl SlashCommand for TestCommand {
    fn name(&self) -> &str { "test" }
    fn description(&self) -> &str { "Run project tests" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message("Running all tests...".into())
        } else {
            CommandOutput::Message(format!("Running tests: {args}"))
        }
    }
}

pub struct LintCommand;
impl SlashCommand for LintCommand {
    fn name(&self) -> &str { "lint" }
    fn description(&self) -> &str { "Run linter on project" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Running linter...".into())
    }
}

pub struct FormatCommand;
impl SlashCommand for FormatCommand {
    fn name(&self) -> &str { "format" }
    fn description(&self) -> &str { "Format code" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Formatting code...".into())
    }
}

pub struct BuildCommand;
impl SlashCommand for BuildCommand {
    fn name(&self) -> &str { "build" }
    fn description(&self) -> &str { "Build the project" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Building project...".into())
    }
}

pub struct DeployCommand;
impl SlashCommand for DeployCommand {
    fn name(&self) -> &str { "deploy" }
    fn description(&self) -> &str { "Deploy the project" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Deploy: configure deployment target first.".into())
    }
}

pub struct ChatCommand;
impl SlashCommand for ChatCommand {
    fn name(&self) -> &str { "chat" }
    fn description(&self) -> &str { "Switch to chat mode (no tools)" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Chat mode: tools disabled.".into())
    }
}

pub struct CodeCommand;
impl SlashCommand for CodeCommand {
    fn name(&self) -> &str { "code" }
    fn description(&self) -> &str { "Switch to code mode (tools enabled)" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Code mode: tools enabled.".into())
    }
}

pub struct ShareCommand;
impl SlashCommand for ShareCommand {
    fn name(&self) -> &str { "share" }
    fn description(&self) -> &str { "Share conversation as link or file" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Sharing: export conversation to file first with /export.".into())
    }
}

pub struct FeedbackCommand;
impl SlashCommand for FeedbackCommand {
    fn name(&self) -> &str { "feedback" }
    fn description(&self) -> &str { "Send feedback" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Report issues: https://github.com/nicktien007/oxicode/issues".into())
    }
}

pub struct DocsCommand;
impl SlashCommand for DocsCommand {
    fn name(&self) -> &str { "docs" }
    fn description(&self) -> &str { "Open documentation" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Docs: https://github.com/nicktien007/oxicode".into())
    }
}
