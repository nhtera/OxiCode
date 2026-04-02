//! Plan slash commands: /plan, /plan create, /plan list.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct PlanCommand;

impl SlashCommand for PlanCommand {
    fn name(&self) -> &str { "plan" }
    fn description(&self) -> &str { "Plan mode (create/list/show)" }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub {
            "create" => {
                let name = if rest.is_empty() { "untitled" } else { rest };
                CommandOutput::Message(format!("Entering plan mode: {name}"))
            }
            "list" => CommandOutput::Message("Plans: (scan plans/ directory)".into()),
            "show" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /plan show <name>".into())
                } else {
                    CommandOutput::Message(format!("Plan: {rest}"))
                }
            }
            "" => CommandOutput::Message("Usage: /plan <create|list|show>".into()),
            _ => CommandOutput::Error(format!("Unknown: /plan {sub}")),
        }
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["create", "list", "show"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }
}
