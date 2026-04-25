//! Slash command framework: registry, parsing, and command trait.
//!
//! Commands are invoked with `/name [args]` in the TUI input.

pub mod agent_commands;
pub mod clipboard_commands;
pub mod debug_commands;
pub mod discovery_commands;
pub mod enterprise_commands;
pub mod extras_commands;
pub mod general;
pub mod git_commands;
pub mod git_helpers;
pub mod hook_commands;
pub mod mcp_commands;
pub mod new_commands;
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
pub mod ui_commands;
pub mod utility_commands;
pub mod view_commands;
pub mod workflow_commands;

use std::collections::HashMap;
use std::sync::Arc;

use oxicode_state::StateStore;

/// Result of executing a slash command.
#[derive(Debug)]
pub enum CommandOutput {
    /// Display text to the user.
    Message(String),
    /// Command executed silently (no output).
    Silent,
    /// Command wants to quit the app.
    Quit,
    /// Error message.
    Error(String),
    /// Command requests a model switch.
    SwitchModel(String),
}

/// Shared context available to all commands.
pub struct CommandContext {
    pub state_store: Arc<StateStore>,
    pub model: String,
    pub provider_name: String,
    pub session_id: String,
    /// Reference to the command registry — used by `/status` and future introspection.
    pub command_registry: Arc<CommandRegistry>,
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

    /// Get tab-completion candidates for partial input (public API for external callers).
    #[allow(dead_code)] // Public API — available for IDE server and future TUI integration
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

    /// List all commands with pre-computed argument completions for TUI ghost text.
    pub fn all_commands_with_completions(
        &self,
        ctx: &CommandContext,
    ) -> Vec<(&str, &str, Vec<String>)> {
        let mut cmds: Vec<_> = self
            .commands
            .values()
            .map(|cmd| (cmd.name(), cmd.description(), cmd.completions("", ctx)))
            .collect();
        cmds.sort_by_key(|(name, _, _)| *name);
        cmds
    }

    /// Number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    #[allow(dead_code)] // Standard companion to len(); not called directly yet.
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
    // /share — TS social feature without backend wiring (Phase 6 prune).

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
    reg.register(Box::new(session_view_commands::BranchesCommand));
    reg.register(Box::new(session_view_commands::SystemPromptCommand));
    reg.register(Box::new(session_view_commands::ApproveCommand));
    reg.register(Box::new(session_view_commands::RejectCommand));
    reg.register(Box::new(session_view_commands::FeedbackCommand));
    reg.register(Box::new(session_view_commands::DocsCommand));

    // Phase 5: New commands (resume/review/vim/rename/usage + stats/rewind/thinking).
    // /resume's interactive flow is handled inline by the TUI + engine task;
    // this registry entry surfaces it in /help and autocomplete.
    reg.register(Box::new(new_commands::ResumeCommand));
    reg.register(Box::new(new_commands::ReviewCommand));
    reg.register(Box::new(new_commands::VimCommand));
    reg.register(Box::new(new_commands::RenameCommand));
    reg.register(Box::new(new_commands::RenameSessionCommand));
    reg.register(Box::new(new_commands::UsageCommand));
    reg.register(Box::new(new_commands::StatsCommand));
    reg.register(Box::new(new_commands::RewindCommand));
    reg.register(Box::new(new_commands::ThinkingCommand));
    reg.register(Box::new(new_commands::SandboxToggleCommand));
    reg.register(Box::new(new_commands::OutputStyleCommand));

    // Phase 9: Enterprise commands (auth, telemetry, settings)
    // /managed dropped (TS enterprise-only; Phase 6 prune).
    reg.register(Box::new(enterprise_commands::TelemetryCommand));
    reg.register(Box::new(enterprise_commands::SettingsCommand));
    reg.register(Box::new(enterprise_commands::AuthCommand));

    // Phase 10: Extras
    // /voice, /desktop, /mobile, /bridge, /install-github-app, /remote-setup,
    // /remote-env all pruned in Phase 6 (oxicode bridge-mode + enterprise-only).

    // Phase 3: Gap closure — clipboard, reasoning, and utility commands
    reg.register(Box::new(clipboard_commands::CopyCommand));
    reg.register(Box::new(reasoning_commands::EffortCommand));
    reg.register(Box::new(utility_commands::UpgradeCommand));
    // /privacy-settings dropped (Anthropic.ai web app proxy; Phase 6 prune).
    reg.register(Box::new(utility_commands::AddDirCommand));
    reg.register(Box::new(utility_commands::ListDirsCommand));
    reg.register(Box::new(utility_commands::RemoveDirCommand));
    // /extra-usage dropped (Anthropic.ai web app proxy; Phase 6 prune).
    // /mcp-doctor dropped (use --debug flag; Phase 6 prune).

    // Phase 6: UI commands (/color, /keybindings, /statusline, /terminal-setup)
    reg.register(Box::new(ui_commands::ColorCommand));
    reg.register(Box::new(ui_commands::KeybindingsCommand));
    reg.register(Box::new(ui_commands::StatuslineCommand));
    reg.register(Box::new(ui_commands::TerminalSetupCommand));

    // Phase 6: Session extras (/tag, /btw, /thinkback)
    // /release-notes dropped (oxicode original; Phase 6 prune).
    reg.register(Box::new(session_extras::TagCommand));
    reg.register(Box::new(session_extras::BtwCommand));
    reg.register(Box::new(session_extras::ThinkbackCommand));

    // /onboarding dropped (first-run hook covers it; Phase 6 prune).

    // Phase 4 (gap-closure): Discovery commands — only ctx-viz survives.
    // /dream, /bughunter, /autofix-pr, /backfill-sessions pruned in Phase 6.
    reg.register(Box::new(discovery_commands::CtxVizCommand));

    // Phase 4 diagnostic commands (/heapdump, /perf-issue, /ant-trace) all
    // pruned in Phase 6 — TS internal-only / oxicode-original.

    // Phase 6 info commands (/advisor, /insights, /stickers, /passes,
    // /rate-limit-options, /reload-plugins-info) all pruned — joke / read-only
    // info dumps with no real backing.

    // /teleport, /ultraplan, /buddy, /good-claude, /settings-sync, /vcr
    // all pruned in Phase 6 (oxicode originals / TS enterprise-only).

    reg
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxicode_common::Message;
    use oxicode_state::{AppState, StateStore};

    fn make_ctx(state_store: Arc<StateStore>) -> CommandContext {
        CommandContext {
            state_store,
            model: "claude-sonnet-4-20250514".to_string(),
            provider_name: "anthropic".to_string(),
            session_id: "test-session-123".to_string(),
            command_registry: Arc::new(default_registry()),
        }
    }

    fn store() -> Arc<StateStore> {
        Arc::new(StateStore::new(AppState::default()))
    }

    fn exec(input: &str) -> Option<CommandOutput> {
        let ctx = make_ctx(store());
        default_registry().execute(input, &ctx)
    }

    fn exec_with(input: &str, ss: Arc<StateStore>) -> Option<CommandOutput> {
        let ctx = make_ctx(ss);
        default_registry().execute(input, &ctx)
    }

    fn expect_msg(output: Option<CommandOutput>) -> String {
        match output {
            Some(CommandOutput::Message(m)) => m,
            Some(CommandOutput::Error(e)) => panic!("Expected message, got error: {e}"),
            Some(CommandOutput::Quit) => panic!("Expected message, got quit"),
            Some(CommandOutput::Silent) => panic!("Expected message, got silent"),
            Some(CommandOutput::SwitchModel(m)) => panic!("Expected message, got SwitchModel({m})"),
            None => panic!("Expected message, got None"),
        }
    }

    // ── /help ──────────────────────────────────────────────────

    #[test]
    fn help_lists_commands() {
        let msg = expect_msg(exec("/help"));
        assert!(msg.contains("Available commands:"));
        assert!(msg.contains("/help"));
        assert!(msg.contains("/model"));
        assert!(msg.contains("/cost"));
        assert!(msg.contains("/status"));
        assert!(msg.contains("/clear"));
    }

    #[test]
    fn help_lists_many_commands() {
        let msg = expect_msg(exec("/help"));
        let count = msg.lines().filter(|l| l.trim().starts_with('/')).count();
        assert!(count > 20, "Should list 20+ commands, got {count}");
    }

    // ── /version ───────────────────────────────────────────────

    #[test]
    fn version_shows_name() {
        let msg = expect_msg(exec("/version"));
        assert!(msg.contains("OxiCode") || msg.contains("oxicode"));
    }

    // ── /model ─────────────────────────────────────────────────

    #[test]
    fn model_shows_current() {
        let msg = expect_msg(exec("/model"));
        assert!(msg.contains("claude-sonnet-4-20250514"), "got: {msg}");
    }

    // ── /cost ──────────────────────────────────────────────────

    #[test]
    fn cost_zero_initially() {
        let msg = expect_msg(exec("/cost"));
        assert!(msg.contains('0'), "Cost should show zero, got: {msg}");
    }

    #[test]
    fn cost_reflects_usage() {
        let ss = store();
        ss.add_usage(
            &oxicode_common::Usage {
                input_tokens: 1000,
                output_tokens: 500,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            "claude-sonnet-4-20250514",
        );
        let msg = expect_msg(exec_with("/cost", ss));
        assert!(msg.contains("1000") || msg.contains("1,000"), "got: {msg}");
    }

    // ── /status ────────────────────────────────────────────────

    #[test]
    fn status_shows_info() {
        let msg = expect_msg(exec("/status"));
        assert!(msg.contains("Model:"));
        assert!(msg.contains("Messages:"));
        assert!(msg.contains("Tokens:"));
    }

    #[test]
    fn status_tracks_messages() {
        let ss = store();
        ss.push_message(Message::user("hello"));
        ss.push_message(Message::assistant());
        let msg = expect_msg(exec_with("/status", ss));
        assert!(msg.contains("Messages: 2"), "got: {msg}");
    }

    #[test]
    fn status_shows_slash_command_count() {
        // /status should include "Slash commands: N registered" from ctx.command_registry.
        let msg = expect_msg(exec("/status"));
        assert!(
            msg.contains("Slash commands:") && msg.contains("registered"),
            "got: {msg}"
        );
        // The default registry has > 50 commands; verify it's a non-zero number.
        let count_str = msg
            .lines()
            .find(|l| l.contains("Slash commands:"))
            .expect("line not found");
        let n: usize = count_str
            .split_whitespace()
            .nth(2)
            .and_then(|s| s.parse().ok())
            .expect("parse count");
        assert!(n > 50, "expected >50 slash commands, got {n}");
    }

    // ── /clear ─────────────────────────────────────────────────

    #[test]
    fn clear_empties_messages() {
        let ss = store();
        ss.push_message(Message::user("hello"));
        ss.push_message(Message::assistant());
        assert_eq!(ss.current().messages.len(), 2);

        // /clear returns Silent (no echo line) and clears the transcript.
        assert!(matches!(
            exec_with("/clear", ss.clone()),
            Some(CommandOutput::Silent)
        ));
        assert_eq!(ss.current().messages.len(), 0);
    }

    #[test]
    fn clear_returns_silent() {
        let ss = store();
        ss.push_message(Message::user("first message"));
        assert_eq!(ss.current().messages.len(), 1);

        // Verify Silent variant — no "Conversation cleared" echo line emitted.
        let output = exec_with("/clear", ss.clone());
        assert!(
            matches!(output, Some(CommandOutput::Silent)),
            "Expected Silent, got: {output:?}"
        );
        // State mutation still happened.
        assert_eq!(ss.current().messages.len(), 0, "messages should be cleared");
    }

    // ── /quit ──────────────────────────────────────────────────

    #[test]
    fn quit_returns_quit_variant() {
        assert!(matches!(exec("/quit"), Some(CommandOutput::Quit)));
    }

    // ── /session ───────────────────────────────────────────────

    #[test]
    fn session_shows_id() {
        let msg = expect_msg(exec("/session"));
        assert!(
            msg.contains("test-session-123") || msg.contains("Session"),
            "got: {msg}"
        );
    }

    // ── unknown / non-slash ────────────────────────────────────

    #[test]
    fn unknown_command_errors() {
        match exec("/nonexistent") {
            Some(CommandOutput::Error(e)) => {
                assert!(e.contains("Unknown command"));
                assert!(e.contains("/help"));
            }
            other => panic!("Expected Error, got: {other:?}"),
        }
    }

    #[test]
    fn non_slash_returns_none() {
        assert!(exec("hello world").is_none());
    }

    // ── registry properties ────────────────────────────────────

    #[test]
    fn registry_has_50_plus_commands() {
        let reg = default_registry();
        let cmds = reg.all_commands();
        assert!(cmds.len() > 50, "got {}", cmds.len());
    }

    #[test]
    fn registry_size_under_phase6_target() {
        // Phase 6 prune: trim default registry toward TS parity (~71 cmds).
        // Allow some headroom for oxicode originals — 110 is the soft cap;
        // anything above means a new ghost command sneaked in.
        let count = default_registry().len();
        assert!(
            count <= 110,
            "default_registry should be ≤110 commands after Phase 6 prune; got {count}"
        );
    }

    #[test]
    fn pruned_commands_no_longer_registered() {
        let reg = default_registry();
        let names: std::collections::HashSet<&str> =
            reg.all_commands().into_iter().map(|(n, _)| n).collect();
        for ghost in [
            "ultraplan",
            "buddy",
            "good-claude",
            "vcr",
            "teleport",
            "settings-sync",
            "onboarding",
            "managed",
            "voice",
            "desktop",
            "mobile",
            "bridge",
            "share",
            "release-notes",
            "advisor",
            "insights",
            "stickers",
            "passes",
            "mcp-doctor",
            "privacy-settings",
            "extra-usage",
        ] {
            assert!(
                !names.contains(ghost),
                "/{ghost} should have been removed in Phase 6 prune"
            );
        }
    }

    #[test]
    fn all_commands_have_descriptions() {
        for (name, desc) in default_registry().all_commands() {
            assert!(!name.is_empty());
            assert!(!desc.is_empty(), "/{name} needs description");
        }
    }

    #[test]
    fn command_names_are_lowercase() {
        for (name, _) in default_registry().all_commands() {
            assert_eq!(name, name.to_lowercase(), "/{name} should be lowercase");
        }
    }

    // ── /config ────────────────────────────────────────────────

    #[test]
    fn config_shows_configuration() {
        let msg = expect_msg(exec("/config"));
        assert!(!msg.is_empty());
    }
}
