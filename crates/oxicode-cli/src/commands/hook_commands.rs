//! Hook slash commands: /hook enable, /hook disable, /hook list.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct HookCommand;
impl SlashCommand for HookCommand {
    fn name(&self) -> &str {
        "hook"
    }
    fn description(&self) -> &str {
        "Manage hooks (enable/disable/list)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub {
            "enable" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /hook enable <event>".into())
                } else {
                    CommandOutput::Message(format!("Hook enabled: {rest}"))
                }
            }
            "disable" => {
                if rest.is_empty() {
                    CommandOutput::Error("Usage: /hook disable <event>".into())
                } else {
                    CommandOutput::Message(format!("Hook disabled: {rest}"))
                }
            }
            "list" | "" => {
                use oxicode_hooks::events::HookEvent;
                let events: Vec<&str> = HookEvent::ALL.iter().map(HookEvent::as_str).collect();
                CommandOutput::Message(format!(
                    "Hook events ({}):\n  {}",
                    events.len(),
                    events.join("\n  ")
                ))
            }
            _ => CommandOutput::Error(format!("Unknown: /hook {sub}")),
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["enable", "disable", "list"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}
