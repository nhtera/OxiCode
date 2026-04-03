//! Task slash commands: /task create, /task stop, /task list — uses StateStore.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /task [create|stop|list] — manage background tasks via state store.
pub struct TaskCommand;
impl SlashCommand for TaskCommand {
    fn name(&self) -> &str {
        "task"
    }
    fn description(&self) -> &str {
        "Manage background tasks (create/stop/list)"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub.trim() {
            "create" => {
                if rest.trim().is_empty() {
                    return CommandOutput::Error("Usage: /task create <description>".into());
                }
                let id = format!("t{}", ctx.state_store.current().background_tasks.len() + 1);
                let preview = super::git_helpers::truncate(rest.trim(), 30);
                ctx.state_store.update(|s| {
                    s.background_tasks.push(oxicode_state::TaskEntry {
                        id: id.clone(),
                        task_type: "manual".to_string(),
                        status: "pending".to_string(),
                        command_preview: preview.clone(),
                    });
                });
                CommandOutput::Message(format!("Created task {id}: {preview}"))
            }
            "stop" => {
                if rest.trim().is_empty() {
                    return CommandOutput::Error("Usage: /task stop <id>".into());
                }
                let target_id = rest.trim();
                let state = ctx.state_store.current();
                if state
                    .background_tasks
                    .iter()
                    .any(|t| t.id == target_id)
                {
                    ctx.state_store.update(|s| {
                        if let Some(t) = s.background_tasks.iter_mut().find(|t| t.id == target_id)
                        {
                            t.status = "completed".to_string();
                        }
                    });
                    CommandOutput::Message(format!("Task {target_id} marked completed."))
                } else {
                    CommandOutput::Error(format!("Task not found: {target_id}"))
                }
            }
            "list" | "" => {
                let state = ctx.state_store.current();
                let tasks = &state.background_tasks;
                if tasks.is_empty() {
                    return CommandOutput::Message("No background tasks.".into());
                }
                let mut output = String::from("Background tasks:\n");
                for t in tasks {
                    let _ = writeln!(
                        output,
                        "  {:<6} {:<10} {:<12} {}",
                        t.id, t.task_type, t.status, t.command_preview
                    );
                }
                CommandOutput::Message(output)
            }
            other => CommandOutput::Error(format!("Unknown: /task {other}. Use: create, stop, list")),
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
