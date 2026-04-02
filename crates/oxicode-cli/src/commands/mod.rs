//! Slash command framework: registry, parsing, and command trait.
//!
//! Commands are invoked with `/name [args]` in the TUI input.
#![allow(dead_code)] // Command framework defined but not yet wired to main app

pub mod agent_commands;
pub mod debug_commands;
pub mod general;
pub mod git_commands;
pub mod hook_commands;
pub mod mcp_commands;
pub mod plan_commands;
pub mod plugin_commands;
pub mod provider;
pub mod session_commands;
pub mod task_commands;
pub mod team_commands;
pub mod view_commands;

use std::collections::HashMap;
use std::sync::Arc;

use oxicode_state::StateStore;

/// Result of executing a slash command.
pub enum CommandOutput {
    /// Display text to the user.
    Message(String),
    /// Command executed silently (no output).
    Silent,
    /// Command wants to quit the app.
    Quit,
    /// Error message.
    Error(String),
}

/// Shared context available to all commands.
pub struct CommandContext {
    pub state_store: Arc<StateStore>,
    pub model: String,
    pub provider_name: String,
    pub session_id: String,
}

/// Trait for slash commands.
pub trait SlashCommand: Send + Sync {
    /// Command name (without leading `/`).
    fn name(&self) -> &str;
    /// Short description for /help.
    fn description(&self) -> &str;
    /// Execute the command with optional args string.
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput;
    /// Tab-completion candidates for arguments (optional).
    fn completions(&self, _partial: &str, _ctx: &CommandContext) -> Vec<String> {
        Vec::new()
    }
}

/// Registry of all available slash commands.
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn SlashCommand>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Register a command.
    pub fn register(&mut self, cmd: Box<dyn SlashCommand>) {
        self.commands.insert(cmd.name().to_string(), cmd);
    }

    /// Parse and execute a slash command string like "/help" or "/model gpt-4o".
    pub fn execute(&self, input: &str, ctx: &CommandContext) -> Option<CommandOutput> {
        let input = input.trim();
        if !input.starts_with('/') {
            return None;
        }

        let without_slash = &input[1..];
        let (name, args) = without_slash
            .split_once(' ')
            .map_or((without_slash, ""), |(n, a)| (n, a.trim()));

        if let Some(cmd) = self.commands.get(name) {
            Some(cmd.execute(args, ctx))
        } else {
            Some(CommandOutput::Error(format!(
                "Unknown command: /{name}. Type /help for available commands."
            )))
        }
    }

    /// Get tab-completion candidates for partial input.
    pub fn completions(&self, partial: &str, ctx: &CommandContext) -> Vec<String> {
        let input = partial.trim();
        if !input.starts_with('/') {
            return Vec::new();
        }

        let without_slash = &input[1..];

        // If no space yet, complete command names.
        if !without_slash.contains(' ') {
            return self
                .commands
                .keys()
                .filter(|name| name.starts_with(without_slash))
                .map(|name| format!("/{name}"))
                .collect();
        }

        // Complete arguments for the command.
        let (name, partial_arg) = without_slash.split_once(' ').unwrap_or((without_slash, ""));
        if let Some(cmd) = self.commands.get(name) {
            cmd.completions(partial_arg.trim(), ctx)
        } else {
            Vec::new()
        }
    }

    /// List all commands (sorted by name) for /help.
    pub fn all_commands(&self) -> Vec<(&str, &str)> {
        let mut cmds: Vec<_> = self
            .commands
            .values()
            .map(|cmd| (cmd.name(), cmd.description()))
            .collect();
        cmds.sort_by_key(|(name, _)| *name);
        cmds
    }

    /// Number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Create a registry with all built-in commands.
pub fn default_registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();

    // General commands
    reg.register(Box::new(general::HelpCommand));
    reg.register(Box::new(general::VersionCommand));
    reg.register(Box::new(general::ClearCommand));
    reg.register(Box::new(general::StatusCommand));
    reg.register(Box::new(general::ConfigCommand));
    reg.register(Box::new(general::CompactCommand));
    reg.register(Box::new(general::InitCommand));
    reg.register(Box::new(general::QuitCommand));

    // Provider commands
    reg.register(Box::new(provider::ModelCommand));
    reg.register(Box::new(provider::PermissionsCommand));
    reg.register(Box::new(provider::HooksCommand));
    reg.register(Box::new(provider::McpCommand));
    reg.register(Box::new(provider::CostCommand));

    // Session commands
    reg.register(Box::new(session_commands::SessionCommand));
    reg.register(Box::new(session_commands::ExportCommand));
    reg.register(Box::new(session_commands::DiffCommand));
    reg.register(Box::new(session_commands::MemoryCommand));
    reg.register(Box::new(session_commands::UndoCommand));
    reg.register(Box::new(session_commands::BugCommand));
    reg.register(Box::new(session_commands::DoctorCommand));

    // Agent / skill / task introspection commands
    reg.register(Box::new(agent_commands::AgentCommand));
    reg.register(Box::new(agent_commands::SkillsCommand));
    reg.register(Box::new(agent_commands::TasksCommand));

    // Phase 5: Plugin commands
    reg.register(Box::new(plugin_commands::PluginCommand));

    // Phase 5: Plan commands
    reg.register(Box::new(plan_commands::PlanCommand));

    // Phase 5: Git commands
    reg.register(Box::new(git_commands::CommitCommand));
    reg.register(Box::new(git_commands::PrCommand));
    reg.register(Box::new(git_commands::BranchCommand));
    reg.register(Box::new(git_commands::LogCommand));
    reg.register(Box::new(git_commands::StashCommand));
    reg.register(Box::new(git_commands::PushCommand));
    reg.register(Box::new(git_commands::PullCommand));

    // Phase 5: Team commands
    reg.register(Box::new(team_commands::TeamCommand));
    reg.register(Box::new(team_commands::AgentsCommand));

    // Phase 5: Task commands
    reg.register(Box::new(task_commands::TaskCommand));

    // Phase 5: MCP extended commands
    reg.register(Box::new(mcp_commands::McpConnectCommand));
    reg.register(Box::new(mcp_commands::McpDisconnectCommand));
    reg.register(Box::new(mcp_commands::McpToolsCommand));
    reg.register(Box::new(mcp_commands::McpServersCommand));

    // Phase 5: Hook commands
    reg.register(Box::new(hook_commands::HookCommand));

    // Phase 5: Debug commands
    reg.register(Box::new(debug_commands::DebugCommand));
    reg.register(Box::new(debug_commands::DebugToolCallCommand));
    reg.register(Box::new(debug_commands::TokensCommand));
    reg.register(Box::new(debug_commands::ContextCommand));

    // Phase 5: View/UI commands
    reg.register(Box::new(view_commands::ThemeCommand));
    reg.register(Box::new(view_commands::ShortcutsCommand));
    reg.register(Box::new(view_commands::AboutCommand));
    reg.register(Box::new(view_commands::ToolsCommand));
    reg.register(Box::new(view_commands::FastCommand));
    reg.register(Box::new(view_commands::VerboseCommand));
    reg.register(Box::new(view_commands::HistoryCommand));
    reg.register(Box::new(view_commands::ReviewCommand));
    reg.register(Box::new(view_commands::LoginCommand));
    reg.register(Box::new(view_commands::LogoutCommand));
    reg.register(Box::new(view_commands::ResumeCommand));
    reg.register(Box::new(view_commands::SkillCommand));
    reg.register(Box::new(view_commands::CronCommand));
    reg.register(Box::new(view_commands::ScheduleCommand));
    reg.register(Box::new(view_commands::WorktreeCommand));
    reg.register(Box::new(view_commands::ProviderCommand));
    reg.register(Box::new(view_commands::ContextWindowCommand));
    reg.register(Box::new(view_commands::EditConfigCommand));
    reg.register(Box::new(view_commands::ResetCommand));
    reg.register(Box::new(view_commands::RetryCommand));
    reg.register(Box::new(view_commands::ForkCommand));
    reg.register(Box::new(view_commands::SystemPromptCommand));
    reg.register(Box::new(view_commands::ApproveCommand));
    reg.register(Box::new(view_commands::RejectCommand));
    reg.register(Box::new(view_commands::FileCommand));
    reg.register(Box::new(view_commands::SearchCommand));
    reg.register(Box::new(view_commands::RunCommand));
    reg.register(Box::new(view_commands::TestCommand));
    reg.register(Box::new(view_commands::LintCommand));
    reg.register(Box::new(view_commands::FormatCommand));
    reg.register(Box::new(view_commands::BuildCommand));
    reg.register(Box::new(view_commands::DeployCommand));
    reg.register(Box::new(view_commands::ChatCommand));
    reg.register(Box::new(view_commands::CodeCommand));
    reg.register(Box::new(view_commands::ShareCommand));
    reg.register(Box::new(view_commands::FeedbackCommand));
    reg.register(Box::new(view_commands::DocsCommand));

    reg
}
