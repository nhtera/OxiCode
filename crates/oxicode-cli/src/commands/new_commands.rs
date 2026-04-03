//! New commands added in Phase 5: /vim, /rename, /usage, /resume, /review,
//! /stats, /rewind, /thinking, /sandbox-toggle, /output-style.

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
        // Store the display name in active_skills as a marker.
        // Full session-level rename requires Session struct changes (future).
        ctx.state_store.update(|s| {
            s.active_skills.retain(|sk| !sk.starts_with("session_name:"));
            s.active_skills.push(format!("session_name:{name}"));
        });
        CommandOutput::Message(format!(
            "Session '{}' display name set to: {name}",
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

        // Rough cost estimates — rates vary by provider/model.
        let (in_rate, out_rate) = match ctx.provider_name.as_str() {
            "openai" => (2.5, 10.0),      // GPT-4o approx
            "google" | "gemini" => (1.25, 5.0), // Gemini 1.5 Pro approx
            _ => (3.0, 15.0),             // Anthropic Claude Sonnet default
        };
        let input_cost = f64::from(usage.input_tokens) * in_rate / 1_000_000.0;
        let output_cost = f64::from(usage.output_tokens) * out_rate / 1_000_000.0;
        let total_cost = input_cost + output_cost;

        CommandOutput::Message(format!(
            "Session: {}\n\
             Model: {}\n\
             Provider: {}\n\
             Messages: {msg_count}\n\
             Tokens: {} in / {} out\n\
             Est. cost: ${total_cost:.4} (${input_cost:.4} in + ${output_cost:.4} out)\n\
             (rates approximate for {})",
            ctx.session_id, ctx.model, ctx.provider_name,
            usage.input_tokens, usage.output_tokens, ctx.provider_name,
        ))
    }
}

/// /stats — session statistics: message counts by role, tool calls, approximate duration.
pub struct StatsCommand;
impl SlashCommand for StatsCommand {
    fn name(&self) -> &str {
        "stats"
    }
    fn description(&self) -> &str {
        "Show session statistics (messages, tools, duration)"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let messages = &state.messages;

        let user_count = messages
            .iter()
            .filter(|m| m.role == oxicode_common::Role::User)
            .count();
        let assistant_count = messages
            .iter()
            .filter(|m| m.role == oxicode_common::Role::Assistant)
            .count();

        // Count tool use blocks across all messages.
        let tool_calls: usize = messages
            .iter()
            .flat_map(|m| &m.content)
            .filter(|b| matches!(b, oxicode_common::ContentBlock::ToolUse { .. }))
            .count();

        // Approximate session duration from first to last message timestamps.
        let duration_str = if let (Some(first), Some(last)) = (messages.first(), messages.last()) {
            let dur = last.created_at.signed_duration_since(first.created_at);
            let mins = dur.num_minutes();
            let secs = dur.num_seconds() % 60;
            if mins > 0 {
                format!("{mins}m {secs}s")
            } else {
                format!("{secs}s")
            }
        } else {
            "N/A".to_string()
        };

        let usage = &state.total_usage;

        CommandOutput::Message(format!(
            "Session: {}\n\
             Duration: {duration_str}\n\
             Messages: {} total ({user_count} user, {assistant_count} assistant)\n\
             Tool calls: {tool_calls}\n\
             Tokens: {} in / {} out\n\
             Active skills: {}\n\
             Background tasks: {}",
            ctx.session_id,
            messages.len(),
            usage.input_tokens,
            usage.output_tokens,
            state.active_skills.len(),
            state.background_tasks.len(),
        ))
    }
}

/// /rewind [N] — remove the last N message pairs from conversation.
pub struct RewindCommand;
impl SlashCommand for RewindCommand {
    fn name(&self) -> &str {
        "rewind"
    }
    fn description(&self) -> &str {
        "Rewind conversation by N turns (default: 1)"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let n: usize = args.trim().parse().unwrap_or(1);
        if n == 0 {
            return CommandOutput::Message("Nothing to rewind.".into());
        }

        let state = ctx.state_store.current();
        let msg_count = state.messages.len();

        // Each "turn" = user message + assistant response = 2 messages.
        let to_remove = (n * 2).min(msg_count);
        let remaining = msg_count - to_remove;

        ctx.state_store.update(|s| {
            s.messages.truncate(remaining);
        });

        let turns_removed = to_remove / 2;
        let extra = if to_remove % 2 != 0 { " (+1 orphan)" } else { "" };
        CommandOutput::Message(format!(
            "Rewound {turns_removed} turn(s){extra}. {remaining} messages remaining."
        ))
    }
}

/// /thinking — toggle extended thinking mode.
pub struct ThinkingCommand;
impl SlashCommand for ThinkingCommand {
    fn name(&self) -> &str {
        "thinking"
    }
    fn description(&self) -> &str {
        "Toggle extended thinking mode"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let skill_key = "extended_thinking";
        let state = ctx.state_store.current();
        let was_enabled = state.active_skills.iter().any(|s| s == skill_key);

        ctx.state_store.update(|s| {
            if was_enabled {
                s.active_skills.retain(|sk| sk != skill_key);
            } else {
                s.active_skills.push(skill_key.to_string());
            }
        });

        let status = if was_enabled { "disabled" } else { "enabled" };
        CommandOutput::Message(format!("Extended thinking: {status}"))
    }
}

/// /sandbox-toggle — toggle sandbox (restricted shell) mode.
pub struct SandboxToggleCommand;
impl SlashCommand for SandboxToggleCommand {
    fn name(&self) -> &str {
        "sandbox-toggle"
    }
    fn description(&self) -> &str {
        "Toggle sandbox mode (restricted shell execution)"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let skill_key = "sandbox_mode";
        let state = ctx.state_store.current();
        let was_enabled = state.active_skills.iter().any(|s| s == skill_key);

        ctx.state_store.update(|s| {
            if was_enabled {
                s.active_skills.retain(|sk| sk != skill_key);
            } else {
                s.active_skills.push(skill_key.to_string());
            }
        });

        let status = if was_enabled { "disabled" } else { "enabled" };
        CommandOutput::Message(format!("Sandbox mode: {status}"))
    }
}

/// /output-style [style] — cycle or set output formatting style.
pub struct OutputStyleCommand;
impl SlashCommand for OutputStyleCommand {
    fn name(&self) -> &str {
        "output-style"
    }
    fn description(&self) -> &str {
        "Set output style (concise, normal, verbose)"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let styles = ["concise", "normal", "verbose"];
        let state = ctx.state_store.current();

        let current = state
            .active_skills
            .iter()
            .find_map(|s| s.strip_prefix("output:"))
            .unwrap_or("normal");

        let new_style = if args.trim().is_empty() {
            // Cycle to next style.
            let idx = styles.iter().position(|&s| s == current).unwrap_or(1);
            styles[(idx + 1) % styles.len()]
        } else {
            let requested = args.trim();
            if styles.contains(&requested) {
                requested
            } else {
                return CommandOutput::Error(format!(
                    "Unknown style: {requested}. Options: {}",
                    styles.join(", ")
                ));
            }
        };

        ctx.state_store.update(|s| {
            s.active_skills.retain(|sk| !sk.starts_with("output:"));
            s.active_skills.push(format!("output:{new_style}"));
        });

        CommandOutput::Message(format!("Output style: {new_style}"))
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["concise", "normal", "verbose"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}
