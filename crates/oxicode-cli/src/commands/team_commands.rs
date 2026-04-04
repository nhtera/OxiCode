//! Team slash commands: /team, /agents — read active agents from state.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /team [create|delete|list] — manage agent teams.
pub struct TeamCommand;
impl SlashCommand for TeamCommand {
    fn name(&self) -> &str {
        "team"
    }
    fn description(&self) -> &str {
        "Manage agent teams (create/delete/list)"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub.trim() {
            "create" => {
                if rest.trim().is_empty() {
                    CommandOutput::Error("Usage: /team create <name>".into())
                } else {
                    CommandOutput::Message(format!(
                        "Teams are managed by the agent framework.\n\
                         Use the TeamCreate tool or mention @{} in your prompt.",
                        rest.trim()
                    ))
                }
            }
            "delete" => {
                if rest.trim().is_empty() {
                    CommandOutput::Error("Usage: /team delete <name>".into())
                } else {
                    CommandOutput::Message(format!(
                        "Teams are managed by the agent framework.\n\
                         Use the TeamDelete tool to remove team '{}'.",
                        rest.trim()
                    ))
                }
            }
            "list" | "" => {
                let state = ctx.state_store.current();
                let agents = &state.active_agents;
                if agents.is_empty() {
                    CommandOutput::Message("No active teams or agents.".into())
                } else {
                    let mut output = String::from("Active agents:\n");
                    for a in agents {
                        let _ = writeln!(
                            output,
                            "  {:<16} {:<12} started: {}",
                            a.name, a.status, a.started_at
                        );
                    }
                    CommandOutput::Message(output)
                }
            }
            other => {
                CommandOutput::Error(format!("Unknown: /team {other}. Use: create, delete, list"))
            }
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["create", "delete", "list"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

/// /agents — list running agents from state.
pub struct AgentsCommand;
impl SlashCommand for AgentsCommand {
    fn name(&self) -> &str {
        "agents"
    }
    fn description(&self) -> &str {
        "List running agents"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let agents = &state.active_agents;
        if agents.is_empty() {
            return CommandOutput::Message("No active agents.".into());
        }
        let mut output = String::from("Active agents:\n");
        for a in agents {
            let _ = writeln!(output, "  {:<16} {:<12} {}", a.name, a.status, a.started_at);
        }
        CommandOutput::Message(output)
    }
}
