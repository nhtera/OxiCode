//! Task slash commands: /task create, /task stop, /task list.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct TaskCommand;
impl SlashCommand for TaskCommand {
    fn name(&self) -> &str {
        "task"
    }
    fn description(&self) -> &str {
        "Manage background tasks (create/stop/list)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub {
            "create" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /task create <description>".into())
                } else {
                    CommandOutput::Message(format!("Creating task: {rest}"))
                }
            }
            "stop" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /task stop <id>".into())
                } else {
                    CommandOutput::Message(format!("Stopping task: {rest}"))
                }
            }
            "list" | "" => CommandOutput::Message("Tasks: (none active)".into()),
            _ => CommandOutput::Error(format!("Unknown: /task {sub}")),
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["create", "stop", "list"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}
