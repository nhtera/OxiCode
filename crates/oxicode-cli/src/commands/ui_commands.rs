//! UI-related commands: /color, /keybindings, /statusline, /terminal-setup.

use super::{CommandContext, CommandOutput, SlashCommand};

/// /color [scheme] — switch color scheme.
pub struct ColorCommand;
impl SlashCommand for ColorCommand {
    fn name(&self) -> &str {
        "color"
    }
    fn description(&self) -> &str {
        "Switch color scheme (dark, light, solarized, monokai)"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let schemes = ["dark", "light", "solarized", "monokai", "dracula", "nord"];
        let current = ctx
            .state_store
            .current()
            .active_skills
            .iter()
            .find_map(|s| s.strip_prefix("theme:"))
            .unwrap_or("dark")
            .to_string();

        if args.trim().is_empty() {
            return CommandOutput::Message(format!(
                "Current theme: {current}\nAvailable: {}",
                schemes.join(", ")
            ));
        }

        let requested = args.trim();
        if !schemes.contains(&requested) {
            return CommandOutput::Error(format!(
                "Unknown scheme: {requested}. Available: {}",
                schemes.join(", ")
            ));
        }

        ctx.state_store.update(|s| {
            s.active_skills.retain(|sk| !sk.starts_with("theme:"));
            s.active_skills.push(format!("theme:{requested}"));
        });
        CommandOutput::Message(format!("Color scheme: {requested}"))
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["dark", "light", "solarized", "monokai", "dracula", "nord"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

/// /keybindings — show current keybinding table.
pub struct KeybindingsCommand;
impl SlashCommand for KeybindingsCommand {
    fn name(&self) -> &str {
        "keybindings"
    }
    fn description(&self) -> &str {
        "Show/edit keybinding table"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let bindings = [
            ("Ctrl+C", "Quit"),
            ("Ctrl+L", "Clear line"),
            ("Ctrl+W", "Delete word backward"),
            ("Ctrl+U", "Delete to line start"),
            ("Ctrl+F", "Search"),
            ("Ctrl+?", "Show shortcuts"),
            ("Tab", "Toggle side panel"),
            ("Ctrl+Left/Right", "Adjust split ratio"),
            ("Up/Down", "History navigation"),
        ];

        let mut out = String::from("Keybindings:\n");
        for (key, action) in &bindings {
            use std::fmt::Write;
            let _ = writeln!(out, "  {key:<20} {action}");
        }
        out.push_str("\nEdit: ~/.oxicode/keybindings.toml");
        CommandOutput::Message(out)
    }
}

/// /statusline [format] — configure status bar content.
pub struct StatuslineCommand;
impl SlashCommand for StatuslineCommand {
    fn name(&self) -> &str {
        "statusline"
    }
    fn description(&self) -> &str {
        "Configure status line content"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        if args.trim().is_empty() {
            let current = ctx
                .state_store
                .current()
                .active_skills
                .iter()
                .find_map(|s| s.strip_prefix("statusline:"))
                .unwrap_or("default")
                .to_string();
            return CommandOutput::Message(format!(
                "Status line format: {current}\n\
                 Options: default, minimal, verbose\n\
                 Usage: /statusline <format>"
            ));
        }

        let format = args.trim();
        let valid = ["default", "minimal", "verbose"];
        if !valid.contains(&format) {
            return CommandOutput::Error(format!(
                "Unknown format: {format}. Options: {}",
                valid.join(", ")
            ));
        }

        ctx.state_store.update(|s| {
            s.active_skills.retain(|sk| !sk.starts_with("statusline:"));
            s.active_skills.push(format!("statusline:{format}"));
        });
        CommandOutput::Message(format!("Status line: {format}"))
    }
}

/// /terminal-setup — detect terminal capabilities and suggest config.
pub struct TerminalSetupCommand;
impl SlashCommand for TerminalSetupCommand {
    fn name(&self) -> &str {
        "terminal-setup"
    }
    fn description(&self) -> &str {
        "Detect terminal capabilities and suggest configuration"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let term = std::env::var("TERM").unwrap_or_else(|_| "unknown".to_string());
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_else(|_| "unknown".to_string());
        let colorterm = std::env::var("COLORTERM").unwrap_or_else(|_| "unknown".to_string());
        let cols = std::env::var("COLUMNS").unwrap_or_else(|_| "?".to_string());
        let lines = std::env::var("LINES").unwrap_or_else(|_| "?".to_string());

        let truecolor = colorterm == "truecolor" || colorterm == "24bit";

        let mut out = String::from("Terminal Setup:\n");
        {
            use std::fmt::Write;
            let _ = writeln!(out, "  TERM: {term}");
            let _ = writeln!(out, "  Program: {term_program}");
            let _ = writeln!(
                out,
                "  True color: {}",
                if truecolor { "yes" } else { "no" }
            );
            let _ = writeln!(out, "  Size: {cols}x{lines}");
        }
        out.push('\n');

        if !truecolor {
            out.push_str("Suggestion: Set COLORTERM=truecolor for best experience.\n");
        }
        if term == "unknown" {
            out.push_str("Suggestion: Set TERM=xterm-256color or similar.\n");
        }

        CommandOutput::Message(out)
    }
}
