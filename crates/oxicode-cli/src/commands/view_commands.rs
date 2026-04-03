//! View/UI slash commands: /theme, /shortcuts, /about, /tools, /fast, /verbose, /history.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct ThemeCommand;
impl SlashCommand for ThemeCommand {
    fn name(&self) -> &str {
        "theme"
    }
    fn description(&self) -> &str {
        "Switch TUI color theme"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message(
                "Available themes: dark, light, catppuccin, dracula, solarized\n\
                 Usage: /theme <name>"
                    .into(),
            )
        } else {
            CommandOutput::Message(format!("Theme switched to: {args}"))
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["dark", "light", "catppuccin", "dracula", "solarized"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

pub struct ShortcutsCommand;
impl SlashCommand for ShortcutsCommand {
    fn name(&self) -> &str {
        "shortcuts"
    }
    fn description(&self) -> &str {
        "Show keyboard shortcuts"
    }
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
    fn name(&self) -> &str {
        "about"
    }
    fn description(&self) -> &str {
        "Show about information"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(format!(
            "{} v{}\nA production Rust CLI agent\n\
             Repository: https://github.com/nicktien007/oxicode",
            oxicode_common::constants::APP_DISPLAY_NAME,
            oxicode_common::constants::VERSION,
        ))
    }
}

pub struct ToolsCommand;
impl SlashCommand for ToolsCommand {
    fn name(&self) -> &str {
        "tools"
    }
    fn description(&self) -> &str {
        "List all available tools"
    }
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
    fn name(&self) -> &str {
        "fast"
    }
    fn description(&self) -> &str {
        "Toggle fast output mode"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Fast mode toggled.".into())
    }
}

pub struct VerboseCommand;
impl SlashCommand for VerboseCommand {
    fn name(&self) -> &str {
        "verbose"
    }
    fn description(&self) -> &str {
        "Toggle verbose output"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Verbose mode toggled.".into())
    }
}

pub struct HistoryCommand;
impl SlashCommand for HistoryCommand {
    fn name(&self) -> &str {
        "history"
    }
    fn description(&self) -> &str {
        "Show conversation history summary"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let count = state.messages.len();
        CommandOutput::Message(format!("Conversation has {count} messages."))
    }
}
