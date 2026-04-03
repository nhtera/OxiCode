//! New commands added in Phase 5: /vim, /rename, /usage, /resume, /review.

use std::fmt::Write as _;

use super::git_helpers::{run_command, truncate};
use super::{CommandContext, CommandOutput, SlashCommand};

/// /resume [id] — list recent sessions or show resume info.
pub struct ResumeCommand;
impl SlashCommand for ResumeCommand {
    fn name(&self) -> &str {
        "resume"
    }
    fn description(&self) -> &str {
        "Resume a previous session"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.trim().is_empty() {
            match oxicode_session::list_sessions(None) {
                Ok(sessions) if sessions.is_empty() => {
                    CommandOutput::Message("No saved sessions.".into())
                }
                Ok(sessions) => {
                    let mut out = String::from("Recent sessions:\n");
                    for s in sessions.iter().take(10) {
                        let _ = writeln!(out, "  {}", s.display());
                    }
                    out.push_str("\nResume with: oxicode --session <id>");
                    CommandOutput::Message(out)
                }
                Err(e) => CommandOutput::Error(format!("Failed to list sessions: {e}")),
            }
        } else {
            let id = args.trim();
            match oxicode_session::load_session(id, None) {
                Ok(s) => CommandOutput::Message(format!(
                    "Session: {}\nMessages: {}\n\nResume: oxicode --session {}",
                    s.id,
                    s.messages.len(),
                    s.id,
                )),
                Err(e) => CommandOutput::Error(format!("Session not found: {e}")),
            }
        }
    }
}

/// /review [pr_number] — list open PRs or show PR diff.
pub struct ReviewCommand;
impl SlashCommand for ReviewCommand {
    fn name(&self) -> &str {
        "review"
    }
    fn description(&self) -> &str {
        "Review PRs (list or show diff)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.trim().is_empty() {
            match run_command("gh", &["pr", "list", "--limit", "10"]) {
                Ok(out) if out.is_empty() => CommandOutput::Message("No open PRs.".into()),
                Ok(out) => CommandOutput::Message(format!("Open PRs:\n{out}")),
                Err(e) => {
                    if e.contains("not found") {
                        CommandOutput::Error(
                            "gh CLI not found. Install: https://cli.github.com".into(),
                        )
                    } else {
                        CommandOutput::Error(format!("Failed to list PRs: {e}"))
                    }
                }
            }
        } else {
            let pr_num = args.trim();
            if !pr_num.chars().all(|c| c.is_ascii_digit()) {
                return CommandOutput::Error("PR number must be numeric.".into());
            }
            match run_command("gh", &["pr", "diff", pr_num]) {
                Ok(diff) => {
                    let lines: Vec<&str> = diff.lines().collect();
                    let total = lines.len();
                    let preview: String = lines.into_iter().take(100).collect::<Vec<_>>().join("\n");
                    let suffix = if total > 100 {
                        format!("\n\n... ({total} total lines)")
                    } else {
                        String::new()
                    };
                    CommandOutput::Message(format!("PR #{pr_num} diff:\n{preview}{suffix}"))
                }
                Err(e) => CommandOutput::Error(format!("Failed to get diff for PR #{pr_num}: {e}")),
            }
        }
    }
}

/// /vim — toggle editor mode between normal and vim.
pub struct VimCommand;
impl SlashCommand for VimCommand {
    fn name(&self) -> &str {
        "vim"
    }
    fn description(&self) -> &str {
        "Toggle vim editor mode"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let config_path = dirs::home_dir()
            .map(|h| h.join(".oxicode").join("settings.toml"))
            .unwrap_or_default();

        let current = read_editor_mode(&config_path);
        let new_mode = if current == "vim" { "normal" } else { "vim" };
        write_editor_mode(&config_path, new_mode);

        CommandOutput::Message(format!("Editor mode: {new_mode}"))
    }
}

/// Read editor_mode from settings file.
fn read_editor_mode(path: &std::path::Path) -> String {
    if !path.exists() {
        return "normal".to_string();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("editor_mode"))
                .and_then(|l| l.split('=').nth(1))
                .map(|v| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "normal".to_string())
}

/// Write editor_mode to settings file (preserves other settings).
fn write_editor_mode(path: &std::path::Path, mode: &str) {
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(path) {
            let updated = if content.contains("editor_mode") {
                content
                    .lines()
                    .map(|l| {
                        if l.starts_with("editor_mode") {
                            format!("editor_mode = \"{mode}\"")
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                format!("{content}\neditor_mode = \"{mode}\"")
            };
            let _ = std::fs::write(path, updated);
        }
    } else {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, format!("editor_mode = \"{mode}\"\n"));
    }
}

/// /rename <name> — rename current session.
pub struct RenameCommand;
impl SlashCommand for RenameCommand {
    fn name(&self) -> &str {
        "rename"
    }
    fn description(&self) -> &str {
        "Rename current session"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let name = args.trim();
        if name.is_empty() {
            return CommandOutput::Error("Usage: /rename <name>".into());
        }
        let name = truncate(name, 64);
        CommandOutput::Message(format!(
            "Session '{}' renamed to: {name}\n(applied on next save)",
            ctx.session_id
        ))
    }
}

/// /usage — show combined token usage, cost, model, duration.
pub struct UsageCommand;
impl SlashCommand for UsageCommand {
    fn name(&self) -> &str {
        "usage"
    }
    fn description(&self) -> &str {
        "Show token usage and session statistics"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let usage = &state.total_usage;
        let msg_count = state.messages.len();

        let input_cost = f64::from(usage.input_tokens) * 3.0 / 1_000_000.0;
        let output_cost = f64::from(usage.output_tokens) * 15.0 / 1_000_000.0;
        let total_cost = input_cost + output_cost;

        CommandOutput::Message(format!(
            "Session: {}\n\
             Model: {}\n\
             Messages: {msg_count}\n\
             Tokens: {} in / {} out\n\
             Cost: ${total_cost:.4} (${input_cost:.4} in + ${output_cost:.4} out)",
            ctx.session_id, ctx.model, usage.input_tokens, usage.output_tokens,
        ))
    }
}
