//! Slash command framework: registry, parsing, and command trait.
//!
//! Commands are invoked with `/name [args]` in the TUI input.

pub mod agent_commands;
pub mod general;
pub mod provider;
pub mod session_commands;

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

    reg
}
