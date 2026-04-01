//! Session-related commands: /session, /export, /diff, /memory, /undo, /bug, /doctor.

use super::{CommandContext, CommandOutput, SlashCommand};

/// /session — show or manage sessions.
pub struct SessionCommand;

impl SlashCommand for SessionCommand {
    fn name(&self) -> &str { "session" }
    fn description(&self) -> &str { "Show current session info or list sessions" }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        match args.trim() {
            "list" | "ls" => {
                match oxicode_session::list_sessions(None) {
                    Ok(sessions) => {
                        if sessions.is_empty() {
                            return CommandOutput::Message("No saved sessions.".to_string());
                        }
                        let mut output = String::from("Sessions:\n");
                        for s in sessions {
                            output.push_str(&format!("  {}\n", s.display()));
                        }
                        CommandOutput::Message(output)
                    }
                    Err(e) => CommandOutput::Error(format!("Failed to list sessions: {e}")),
                }
            }
            "" => {
                CommandOutput::Message(format!(
                    "Current session: {}\nMessages: {}",
                    ctx.session_id,
                    ctx.state_store.current().messages.len(),
                ))
            }
            _ => CommandOutput::Error(
                "Usage: /session or /session list".to_string(),
            ),
        }
    }
}

/// /export — export conversation to file.
pub struct ExportCommand;

impl SlashCommand for ExportCommand {
    fn name(&self) -> &str { "export" }
    fn description(&self) -> &str { "Export conversation to file (markdown or JSON)" }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let path = if args.is_empty() {
            format!("oxicode-export-{}.md", ctx.session_id)
        } else {
            args.trim().to_string()
        };

        let state = ctx.state_store.current();
        let mut content = String::new();

        for msg in &state.messages {
            let role = match msg.role {
                oxicode_common::Role::User => "User",
                oxicode_common::Role::Assistant => "Assistant",
                oxicode_common::Role::System => "System",
            };
            content.push_str(&format!("## {role}\n\n{}\n\n", msg.text()));
        }

        match std::fs::write(&path, &content) {
            Ok(()) => CommandOutput::Message(format!("Exported to {path}")),
            Err(e) => CommandOutput::Error(format!("Failed to export: {e}")),
        }
    }
}

/// /diff — show recent file changes.
pub struct DiffCommand;

impl SlashCommand for DiffCommand {
    fn name(&self) -> &str { "diff" }
    fn description(&self) -> &str { "Show recent file changes in working directory" }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        match std::process::Command::new("git")
            .args(["diff", "--stat"])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.is_empty() {
                    CommandOutput::Message("No uncommitted changes.".to_string())
                } else {
                    CommandOutput::Message(format!("Changes:\n{stdout}"))
                }
            }
            Err(_) => CommandOutput::Error("Not a git repository or git not available.".to_string()),
        }
    }
}

/// /memory — show or manage memory files.
pub struct MemoryCommand;

impl SlashCommand for MemoryCommand {
    fn name(&self) -> &str { "memory" }
    fn description(&self) -> &str { "Show conversation memory usage" }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let total_chars: usize = state.messages.iter().map(|m| m.text().len()).sum();
        let approx_tokens = total_chars / 4; // rough estimate

        CommandOutput::Message(format!(
            "Messages: {}\nApprox tokens in context: ~{approx_tokens}",
            state.messages.len(),
        ))
    }
}

/// /undo — undo the last assistant response.
pub struct UndoCommand;

impl SlashCommand for UndoCommand {
    fn name(&self) -> &str { "undo" }
    fn description(&self) -> &str { "Remove last assistant response and your prompt" }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        // TODO: Implement state mutation to pop last message pair.
        CommandOutput::Message("Undo not yet implemented.".to_string())
    }
}

/// /bug — report a bug or issue.
pub struct BugCommand;

impl SlashCommand for BugCommand {
    fn name(&self) -> &str { "bug" }
    fn description(&self) -> &str { "Report a bug" }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Report bugs at: https://github.com/nicktien007/oxicode/issues".to_string(),
        )
    }
}

/// /doctor — check system health.
pub struct DoctorCommand;

impl SlashCommand for DoctorCommand {
    fn name(&self) -> &str { "doctor" }
    fn description(&self) -> &str { "Check system health and dependencies" }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let mut checks = Vec::new();

        // Check git.
        let git_ok = std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_ok();
        checks.push(format!("  git: {}", if git_ok { "ok" } else { "not found" }));

        // Check model config.
        checks.push(format!("  model: {}", ctx.model));
        checks.push(format!("  provider: {}", ctx.provider_name));

        // Check config directory.
        let config_dir = dirs::home_dir()
            .map(|h| h.join(".oxicode"))
            .unwrap_or_default();
        let config_exists = config_dir.exists();
        checks.push(format!(
            "  config dir: {} ({})",
            config_dir.display(),
            if config_exists { "exists" } else { "missing" },
        ));

        CommandOutput::Message(format!("Health check:\n{}", checks.join("\n")))
    }
}
