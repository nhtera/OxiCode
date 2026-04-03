//! General slash commands: /help, /version, /clear, /status, /config, /compact, /init, /quit.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /help — list available commands.
pub struct HelpCommand;

impl SlashCommand for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }
    fn description(&self) -> &str {
        "Show available commands"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let registry = super::default_registry();
        let mut output = String::from("Available commands:\n");
        for (name, desc) in registry.all_commands() {
            let _ = writeln!(output, "  /{name:<16} {desc}");
        }
        CommandOutput::Message(output)
    }
}

/// /version — show app version.
pub struct VersionCommand;

impl SlashCommand for VersionCommand {
    fn name(&self) -> &str {
        "version"
    }
    fn description(&self) -> &str {
        "Show OxiCode version"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(format!(
            "{} v{}",
            oxicode_common::constants::APP_DISPLAY_NAME,
            oxicode_common::constants::VERSION,
        ))
    }
}

/// /clear — clear conversation history.
pub struct ClearCommand;

impl SlashCommand for ClearCommand {
    fn name(&self) -> &str {
        "clear"
    }
    fn description(&self) -> &str {
        "Clear conversation history"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        ctx.state_store.clear_messages();
        CommandOutput::Message("Conversation cleared.".to_string())
    }
}

/// /status — show current session status.
pub struct StatusCommand;

impl SlashCommand for StatusCommand {
    fn name(&self) -> &str {
        "status"
    }
    fn description(&self) -> &str {
        "Show session status"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let msg_count = state.messages.len();
        let usage = &state.total_usage;

        let mut output = format!(
            "Model: {}\nProvider: {}\nSession: {}\nMessages: {msg_count}\nTokens: {}in / {}out",
            ctx.model, ctx.provider_name, ctx.session_id, usage.input_tokens, usage.output_tokens,
        );

        // Show last rate limit event if present.
        if let Some(snapshot) = &state.last_rate_limit {
            let _ = write!(
                output,
                "\nLast rate limited: {} ({}, {})",
                snapshot.occurred_at, snapshot.provider, snapshot.info.limit_type,
            );
        }

        CommandOutput::Message(output)
    }
}

/// /config — show current configuration.
pub struct ConfigCommand;

impl SlashCommand for ConfigCommand {
    fn name(&self) -> &str {
        "config"
    }
    fn description(&self) -> &str {
        "Show current configuration"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(format!(
            "model: {}\nprovider: {}\nsession: {}",
            ctx.model, ctx.provider_name, ctx.session_id,
        ))
    }
}

/// /compact — compact conversation context (remove old messages).
pub struct CompactCommand;

impl SlashCommand for CompactCommand {
    fn name(&self) -> &str {
        "compact"
    }
    fn description(&self) -> &str {
        "Compact conversation context"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        // Actual compaction is handled async in the engine task (main.rs).
        // This fallback only triggers in non-TUI contexts.
        CommandOutput::Message("Compact requires interactive mode (TUI).".to_string())
    }
}

/// /init — initialize OxiCode config in current directory.
pub struct InitCommand;

impl SlashCommand for InitCommand {
    fn name(&self) -> &str {
        "init"
    }
    fn description(&self) -> &str {
        "Initialize .oxicode config in current directory"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let dir = std::path::Path::new(".oxicode");
        if dir.exists() {
            return CommandOutput::Message(".oxicode/ already exists.".to_string());
        }

        match std::fs::create_dir_all(dir) {
            Ok(()) => CommandOutput::Message(
                "Created .oxicode/ directory. Add OXICODE.md for project instructions.".to_string(),
            ),
            Err(e) => CommandOutput::Error(format!("Failed to create .oxicode/: {e}")),
        }
    }
}

/// /quit — exit the application.
pub struct QuitCommand;

impl SlashCommand for QuitCommand {
    fn name(&self) -> &str {
        "quit"
    }
    fn description(&self) -> &str {
        "Exit OxiCode"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Quit
    }
}
