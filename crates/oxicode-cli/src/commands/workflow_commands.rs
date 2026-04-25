//! Workflow slash commands: /run, /test, /lint, /format, /build, /deploy, /search, /file,
//! /chat, /code, /share.
//!
//! /test, /lint, /format, /build, /deploy use project auto-detection to pick the right tool.

use std::fmt::Write as _;

use super::git_helpers::run_command;
use super::project_detect::{detect_project, format_cmd};
use super::{CommandContext, CommandOutput, SlashCommand};

/// Helper: detect project and run the appropriate command for a workflow step.
/// `step_name` is "test", "lint", etc. for error messages.
/// `extra_args` are appended to the detected command args.
fn run_detected(step: &str, extra_args: &[&str], fallback_msg: &str) -> CommandOutput {
    let cwd = std::env::current_dir().unwrap_or_default();
    let Some(project) = detect_project(&cwd) else {
        return CommandOutput::Error(format!("No project detected. {fallback_msg}"));
    };

    let (cmd, base_args) = match step {
        "test" => project.test,
        "lint" => project.lint,
        "format" => project.format,
        "build" => project.build,
        "deploy" => match project.deploy {
            Some(d) => d,
            None => {
                return CommandOutput::Message(format!(
                    "No deploy config found for {} project.\n\
                         Use /run to execute custom deploy scripts.",
                    project.project_type
                ));
            }
        },
        _ => return CommandOutput::Error(format!("Unknown workflow step: {step}")),
    };

    let mut args: Vec<&str> = base_args;
    args.extend_from_slice(extra_args);
    let display = format_cmd(cmd, &args);

    match run_command(cmd, &args) {
        Ok(out) if out.is_empty() => {
            CommandOutput::Message(format!("{step}: completed ({display})"))
        }
        Ok(out) => CommandOutput::Message(format!("{step} ({display}):\n{out}")),
        Err(e) => CommandOutput::Error(format!("{step} failed ({display}):\n{e}")),
    }
}

pub struct RunCommand;
impl SlashCommand for RunCommand {
    fn name(&self) -> &str {
        "run"
    }
    fn description(&self) -> &str {
        "Run a shell command"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            return CommandOutput::Error("Usage: /run <command>".into());
        }
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        let cmd = parts[0];
        let cmd_args: Vec<&str> = if parts.len() > 1 {
            parts[1].split_whitespace().collect()
        } else {
            vec![]
        };
        match run_command(cmd, &cmd_args) {
            Ok(out) if out.is_empty() => CommandOutput::Message("(no output)".into()),
            Ok(out) => CommandOutput::Message(out),
            Err(e) => CommandOutput::Error(format!("Command failed: {e}")),
        }
    }
}

pub struct TestCommand;
impl SlashCommand for TestCommand {
    fn name(&self) -> &str {
        "test"
    }
    fn description(&self) -> &str {
        "Run project tests (auto-detects project type)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let extra: Vec<&str> = args.split_whitespace().collect();
        run_detected("test", &extra, "Create a Cargo.toml, package.json, etc.")
    }
}

pub struct LintCommand;
impl SlashCommand for LintCommand {
    fn name(&self) -> &str {
        "lint"
    }
    fn description(&self) -> &str {
        "Run linter (auto-detects project type)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let extra: Vec<&str> = args.split_whitespace().collect();
        run_detected("lint", &extra, "Create a Cargo.toml, package.json, etc.")
    }
}

pub struct FormatCommand;
impl SlashCommand for FormatCommand {
    fn name(&self) -> &str {
        "format"
    }
    fn description(&self) -> &str {
        "Format code (auto-detects project type)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let extra: Vec<&str> = args.split_whitespace().collect();
        run_detected("format", &extra, "Create a Cargo.toml, package.json, etc.")
    }
}

pub struct BuildCommand;
impl SlashCommand for BuildCommand {
    fn name(&self) -> &str {
        "build"
    }
    fn description(&self) -> &str {
        "Build the project (auto-detects project type)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let extra: Vec<&str> = args.split_whitespace().collect();
        run_detected("build", &extra, "Create a Cargo.toml, package.json, etc.")
    }
}

pub struct DeployCommand;
impl SlashCommand for DeployCommand {
    fn name(&self) -> &str {
        "deploy"
    }
    fn description(&self) -> &str {
        "Deploy the project (auto-detects deploy config)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let extra: Vec<&str> = args.split_whitespace().collect();
        run_detected(
            "deploy",
            &extra,
            "Add fly.toml, vercel.json, Dockerfile, etc.",
        )
    }
}

pub struct SearchCommand;
impl SlashCommand for SearchCommand {
    fn name(&self) -> &str {
        "search"
    }
    fn description(&self) -> &str {
        "Search codebase for a pattern"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            return CommandOutput::Error("Usage: /search <pattern>".into());
        }
        match run_command("rg", &["--count", "--color=never", "--", args.trim()]) {
            Ok(out) if out.is_empty() => CommandOutput::Message(format!("No matches for: {args}")),
            Ok(out) => {
                let preview: String = out.lines().take(30).collect::<Vec<_>>().join("\n");
                let total = out.lines().count();
                let suffix = if total > 30 {
                    format!("\n... ({total} files total)")
                } else {
                    String::new()
                };
                CommandOutput::Message(format!("Search results:\n{preview}{suffix}"))
            }
            Err(_) => {
                // Fallback to grep if rg not available.
                match run_command("grep", &["-r", "--count", args.trim(), "."]) {
                    Ok(out) => CommandOutput::Message(format!("Search results:\n{out}")),
                    Err(e) => CommandOutput::Error(format!("Search failed: {e}")),
                }
            }
        }
    }
}

pub struct FileCommand;
impl SlashCommand for FileCommand {
    fn name(&self) -> &str {
        "file"
    }
    fn description(&self) -> &str {
        "Read a file or open in $EDITOR"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let args = args.trim();
        if args.is_empty() {
            return CommandOutput::Message("Usage: /file <path> [--edit]".into());
        }

        let (path_str, action) = if args.ends_with("--edit") {
            (args.trim_end_matches("--edit").trim(), "edit")
        } else {
            (args, "read")
        };

        let path = std::path::Path::new(path_str);
        if !path.exists() {
            return CommandOutput::Error(format!("File not found: {path_str}"));
        }

        if action == "edit" {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
            match run_command(&editor, &[path_str]) {
                Ok(_) => CommandOutput::Message(format!("Opened {path_str} in {editor}")),
                Err(e) => CommandOutput::Error(format!("Failed to open editor: {e}")),
            }
        } else {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    let lines: Vec<&str> = content.lines().collect();
                    let total = lines.len();
                    let preview: String = lines.into_iter().take(50).collect::<Vec<_>>().join("\n");
                    let suffix = if total > 50 {
                        format!("\n\n... ({total} total lines, showing first 50)")
                    } else {
                        String::new()
                    };
                    CommandOutput::Message(format!("{path_str}:\n{preview}{suffix}"))
                }
                Err(e) => CommandOutput::Error(format!("Cannot read {path_str}: {e}")),
            }
        }
    }
}

pub struct ChatCommand;
impl SlashCommand for ChatCommand {
    fn name(&self) -> &str {
        "chat"
    }
    fn description(&self) -> &str {
        "Switch to chat mode (tools disabled)"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        ctx.state_store.update(|s| {
            // Store mode as a skill marker so the engine can check it.
            s.active_skills.retain(|sk| sk != "mode:code");
            if !s.active_skills.iter().any(|sk| sk == "mode:chat") {
                s.active_skills.push("mode:chat".to_string());
            }
        });
        CommandOutput::Message("Chat mode enabled: tools disabled for next turns.".into())
    }
}

pub struct CodeCommand;
impl SlashCommand for CodeCommand {
    fn name(&self) -> &str {
        "code"
    }
    fn description(&self) -> &str {
        "Switch to code mode (tools enabled)"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        ctx.state_store.update(|s| {
            s.active_skills.retain(|sk| sk != "mode:chat");
            if !s.active_skills.iter().any(|sk| sk == "mode:code") {
                s.active_skills.push("mode:code".to_string());
            }
        });
        CommandOutput::Message("Code mode enabled: tools active.".into())
    }
}

/// Unregistered in Phase 6 prune (2026-04-26); kept for re-introduction.
#[allow(dead_code)]
pub struct ShareCommand;
impl SlashCommand for ShareCommand {
    fn name(&self) -> &str {
        "share"
    }
    fn description(&self) -> &str {
        "Export conversation as markdown or JSON"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        if state.messages.is_empty() {
            return CommandOutput::Message("No conversation to share.".into());
        }

        let format = args.trim();
        let is_json = format == "json" || format == "--json";

        let filename = if is_json {
            format!("oxicode-share-{}.json", ctx.session_id)
        } else {
            format!("oxicode-share-{}.md", ctx.session_id)
        };

        let content = if is_json {
            match serde_json::to_string_pretty(&state.messages) {
                Ok(json) => json,
                Err(e) => return CommandOutput::Error(format!("JSON serialization failed: {e}")),
            }
        } else {
            let mut md = String::new();
            for msg in &state.messages {
                let role = match msg.role {
                    oxicode_common::Role::User => "User",
                    oxicode_common::Role::Assistant => "Assistant",
                    oxicode_common::Role::System => "System",
                };
                let _ = write!(md, "## {role}\n\n{}\n\n", msg.text());
            }
            md
        };

        match std::fs::write(&filename, &content) {
            Ok(()) => CommandOutput::Message(format!(
                "Conversation exported to {filename} ({} messages)",
                state.messages.len()
            )),
            Err(e) => CommandOutput::Error(format!("Failed to write: {e}")),
        }
    }
}
