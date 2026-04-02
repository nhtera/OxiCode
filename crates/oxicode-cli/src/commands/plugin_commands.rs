//! Plugin slash commands: /plugin install, /plugin list, /plugin remove.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct PluginCommand;

impl SlashCommand for PluginCommand {
    fn name(&self) -> &str { "plugin" }
    fn description(&self) -> &str { "Manage plugins (install/list/remove)" }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub {
            "install" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /plugin install <path>".into())
                } else {
                    CommandOutput::Message(format!("Installing plugin from: {rest}"))
                }
            }
            "remove" | "uninstall" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /plugin remove <name>".into())
                } else {
                    CommandOutput::Message(format!("Removing plugin: {rest}"))
                }
            }
            "list" | "" => {
                CommandOutput::Message("Installed plugins: (none loaded)".into())
            }
            _ => CommandOutput::Error(format!("Unknown subcommand: /plugin {sub}")),
        }
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["install", "list", "remove"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }
}
