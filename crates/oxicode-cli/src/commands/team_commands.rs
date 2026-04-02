//! Team slash commands: /team, /team create, /team delete, /team list.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct TeamCommand;
impl SlashCommand for TeamCommand {
    fn name(&self) -> &str { "team" }
    fn description(&self) -> &str { "Manage agent teams (create/delete/list)" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub {
            "create" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /team create <name>".into())
                } else {
                    CommandOutput::Message(format!("Creating team: {rest}"))
                }
            }
            "delete" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /team delete <name>".into())
                } else {
                    CommandOutput::Message(format!("Deleting team: {rest}"))
                }
            }
            "list" | "" => CommandOutput::Message("Teams: (none active)".into()),
            _ => CommandOutput::Error(format!("Unknown: /team {sub}")),
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["create", "delete", "list"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }
}

pub struct AgentsCommand;
impl SlashCommand for AgentsCommand {
    fn name(&self) -> &str { "agents" }
    fn description(&self) -> &str { "List running agents" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Active agents: (none running)".into())
    }
}
