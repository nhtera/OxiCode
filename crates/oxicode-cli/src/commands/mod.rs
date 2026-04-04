//! Slash command framework: registry, parsing, and command trait.
//!
//! Commands are invoked with `/name [args]` in the TUI input.

pub mod agent_commands;
pub mod clipboard_commands;
pub mod debug_commands;
pub mod diagnostic_commands;
pub mod discovery_commands;
pub mod enterprise_commands;
pub mod extras_commands;
pub mod general;
pub mod git_commands;
pub mod git_helpers;
pub mod hook_commands;
pub mod info_commands;
pub mod mcp_commands;
pub mod new_commands;
pub mod onboarding_commands;
pub mod plan_commands;
pub mod plugin_commands;
pub mod project_detect;
pub mod provider;
pub mod reasoning_commands;
pub mod session_commands;
pub mod session_extras;
pub mod session_view_commands;
pub mod task_commands;
pub mod team_commands;
pub mod teleport_command;
pub mod ui_commands;
pub mod utility_commands;
pub mod view_commands;
pub mod workflow_commands;

use std::collections::HashMap;
use std::sync::Arc;

use oxicode_state::StateStore;

/// Result of executing a slash command.
pub enum CommandOutput {
    /// Display text to the user.
    Message(String),
    /// Command executed silently (no output).
    #[allow(dead_code)] // TODO(gap-phase-6): wire into TUI silent command execution
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
    #[allow(dead_code)] // TODO(gap-phase-6): wire into TUI tab completion
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
    #[allow(dead_code)] // TODO(gap-phase-6): wire into TUI tab completion
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
    #[allow(dead_code)] // TODO(gap-phase-6): expose in /status and TUI status bar
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    #[allow(dead_code)] // TODO(gap-phase-6): expose in /status and TUI status bar
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Create a registry with all built-in commands.
#[allow(clippy::too_many_lines)] // Command registration listing
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
    reg.register(Box::new(git_commands::IssueCommand));
    reg.register(Box::new(git_commands::PrCommentsCommand));

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

    // Phase 5: Workflow commands (extracted from view_commands)
    reg.register(Box::new(workflow_commands::RunCommand));
    reg.register(Box::new(workflow_commands::TestCommand));
    reg.register(Box::new(workflow_commands::LintCommand));
    reg.register(Box::new(workflow_commands::FormatCommand));
    reg.register(Box::new(workflow_commands::BuildCommand));
    reg.register(Box::new(workflow_commands::DeployCommand));
    reg.register(Box::new(workflow_commands::SearchCommand));
    reg.register(Box::new(workflow_commands::FileCommand));
    reg.register(Box::new(workflow_commands::ChatCommand));
    reg.register(Box::new(workflow_commands::CodeCommand));
    reg.register(Box::new(workflow_commands::ShareCommand));

    // Phase 5: Session view commands (extracted + enhanced)
    reg.register(Box::new(session_view_commands::LoginCommand));
    reg.register(Box::new(session_view_commands::LogoutCommand));
    reg.register(Box::new(session_view_commands::SkillCommand));
    reg.register(Box::new(session_view_commands::CronCommand));
    reg.register(Box::new(session_view_commands::ScheduleCommand));
    reg.register(Box::new(session_view_commands::WorktreeCommand));
    reg.register(Box::new(session_view_commands::ProviderCommand));
    reg.register(Box::new(session_view_commands::ContextWindowCommand));
    reg.register(Box::new(session_view_commands::EditConfigCommand));
    reg.register(Box::new(session_view_commands::ResetCommand));
    reg.register(Box::new(session_view_commands::RetryCommand));
    reg.register(Box::new(session_view_commands::ForkCommand));
    reg.register(Box::new(session_view_commands::SystemPromptCommand));
    reg.register(Box::new(session_view_commands::ApproveCommand));
    reg.register(Box::new(session_view_commands::RejectCommand));
    reg.register(Box::new(session_view_commands::FeedbackCommand));
    reg.register(Box::new(session_view_commands::DocsCommand));

    // Phase 5: New commands (enhanced resume/review + vim/rename/usage + stats/rewind/thinking)
    reg.register(Box::new(new_commands::ResumeCommand));
    reg.register(Box::new(new_commands::ReviewCommand));
    reg.register(Box::new(new_commands::VimCommand));
    reg.register(Box::new(new_commands::RenameCommand));
    reg.register(Box::new(new_commands::UsageCommand));
    reg.register(Box::new(new_commands::StatsCommand));
    reg.register(Box::new(new_commands::RewindCommand));
    reg.register(Box::new(new_commands::ThinkingCommand));
    reg.register(Box::new(new_commands::SandboxToggleCommand));
    reg.register(Box::new(new_commands::OutputStyleCommand));

    // Phase 9: Enterprise commands (auth, telemetry, settings, managed)
    reg.register(Box::new(enterprise_commands::TelemetryCommand));
    reg.register(Box::new(enterprise_commands::SettingsCommand));
    reg.register(Box::new(enterprise_commands::AuthCommand));
    reg.register(Box::new(enterprise_commands::ManagedCommand));

    // Phase 10: Extra commands (voice, desktop, mobile, bridge)
    reg.register(Box::new(extras_commands::VoiceCommand));
    reg.register(Box::new(extras_commands::DesktopCommand));
    reg.register(Box::new(extras_commands::MobileCommand));
    reg.register(Box::new(extras_commands::BridgeCommand));

    // Phase 7: GitHub integration + remote commands
    reg.register(Box::new(extras_commands::InstallGithubAppCommand));
    reg.register(Box::new(extras_commands::RemoteSetupCommand));
    reg.register(Box::new(extras_commands::RemoteEnvCommand));

    // Phase 3: Gap closure — clipboard, reasoning, and utility commands
    reg.register(Box::new(clipboard_commands::CopyCommand));
    reg.register(Box::new(reasoning_commands::EffortCommand));
    reg.register(Box::new(utility_commands::UpgradeCommand));
    reg.register(Box::new(utility_commands::PrivacySettingsCommand));
    reg.register(Box::new(utility_commands::AddDirCommand));
    reg.register(Box::new(utility_commands::ExtraUsageCommand));
    reg.register(Box::new(mcp_commands::McpDoctorCommand));

    // Phase 6: UI commands (/color, /keybindings, /statusline, /terminal-setup)
    reg.register(Box::new(ui_commands::ColorCommand));
    reg.register(Box::new(ui_commands::KeybindingsCommand));
    reg.register(Box::new(ui_commands::StatuslineCommand));
    reg.register(Box::new(ui_commands::TerminalSetupCommand));

    // Phase 6: Session extras (/tag, /btw, /thinkback, /release-notes)
    reg.register(Box::new(session_extras::TagCommand));
    reg.register(Box::new(session_extras::BtwCommand));
    reg.register(Box::new(session_extras::ThinkbackCommand));
    reg.register(Box::new(session_extras::ReleaseNotesCommand));

    // Phase 4 (gap-closure): Onboarding command
    reg.register(Box::new(onboarding_commands::OnboardingCommand));

    // Phase 4 (gap-closure): Discovery commands (dream, bughunter, ctx_viz, autofix-pr, backfill-sessions)
    reg.register(Box::new(discovery_commands::DreamCommand));
    reg.register(Box::new(discovery_commands::BughunterCommand));
    reg.register(Box::new(discovery_commands::CtxVizCommand));
    reg.register(Box::new(discovery_commands::AutofixPrCommand));
    reg.register(Box::new(discovery_commands::BackfillSessionsCommand));

    // Phase 4 (gap-closure): Diagnostic commands (heapdump, perf-issue, ant-trace)
    reg.register(Box::new(diagnostic_commands::HeapdumpCommand));
    reg.register(Box::new(diagnostic_commands::PerfIssueCommand));
    reg.register(Box::new(diagnostic_commands::AntTraceCommand));

    // Phase 6: Info commands (/advisor, /insights, /stickers, /passes, /rate-limit-options, /reload-plugins)
    reg.register(Box::new(info_commands::AdvisorCommand));
    reg.register(Box::new(info_commands::InsightsCommand));
    reg.register(Box::new(info_commands::StickersCommand));
    reg.register(Box::new(info_commands::PassesCommand));
    reg.register(Box::new(info_commands::RateLimitOptionsCommand));
    reg.register(Box::new(info_commands::ReloadPluginsInfoCommand));

    // Phase 7 (gap-closure): Teleport command
    reg.register(Box::new(teleport_command::TeleportCommand));

    reg
}
