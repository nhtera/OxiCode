//! Workflow slash commands: /run, /test, /lint, /format, /build, /deploy, /search, /file.

use super::git_helpers::run_command;
use super::{CommandContext, CommandOutput, SlashCommand};

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
        // Split first word as command, rest as args.
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
        "Run project tests"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let extra: Vec<&str> = args.split_whitespace().collect();
        let mut cmd_args = vec!["test"];
        cmd_args.extend(&extra);
        match run_command("cargo", &cmd_args) {
            Ok(out) => CommandOutput::Message(format!("Tests:\n{out}")),
            Err(e) => CommandOutput::Error(format!("Tests failed:\n{e}")),
        }
    }
}

pub struct LintCommand;
impl SlashCommand for LintCommand {
    fn name(&self) -> &str {
        "lint"
    }
    fn description(&self) -> &str {
        "Run linter on project"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        match run_command("cargo", &["clippy", "--all-targets"]) {
            Ok(out) if out.is_empty() => CommandOutput::Message("Lint: all clean.".into()),
            Ok(out) => CommandOutput::Message(format!("Lint:\n{out}")),
            Err(e) => CommandOutput::Error(format!("Lint failed:\n{e}")),
        }
    }
}

pub struct FormatCommand;
impl SlashCommand for FormatCommand {
    fn name(&self) -> &str {
        "format"
    }
    fn description(&self) -> &str {
        "Format code"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        match run_command("cargo", &["fmt"]) {
            Ok(_) => CommandOutput::Message("Code formatted.".into()),
            Err(e) => CommandOutput::Error(format!("Format failed: {e}")),
        }
    }
}

pub struct BuildCommand;
impl SlashCommand for BuildCommand {
    fn name(&self) -> &str {
        "build"
    }
    fn description(&self) -> &str {
        "Build the project"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        match run_command("cargo", &["build"]) {
            Ok(out) if out.is_empty() => CommandOutput::Message("Build successful.".into()),
            Ok(out) => CommandOutput::Message(format!("Build:\n{out}")),
            Err(e) => CommandOutput::Error(format!("Build failed:\n{e}")),
        }
    }
}

pub struct DeployCommand;
impl SlashCommand for DeployCommand {
    fn name(&self) -> &str {
        "deploy"
    }
    fn description(&self) -> &str {
        "Deploy the project"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Deploy: configure deployment target first.\n\
             Use /run to execute custom deploy scripts."
                .into(),
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
        match run_command("rg", &["--count", "--color=never", args.trim()]) {
            Ok(out) if out.is_empty() => {
                CommandOutput::Message(format!("No matches for: {args}"))
            }
            Ok(out) => {
                // Limit output to first 30 lines.
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
        "Quick file operations (read/write/edit)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message("Usage: /file <read|write|edit> <path>".into())
        } else {
            CommandOutput::Message(format!("File operation: {args}"))
        }
    }
}

pub struct ChatCommand;
impl SlashCommand for ChatCommand {
    fn name(&self) -> &str {
        "chat"
    }
    fn description(&self) -> &str {
        "Switch to chat mode (no tools)"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Chat mode: tools disabled.".into())
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
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Code mode: tools enabled.".into())
    }
}

pub struct ShareCommand;
impl SlashCommand for ShareCommand {
    fn name(&self) -> &str {
        "share"
    }
    fn description(&self) -> &str {
        "Share conversation as link or file"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Sharing: export conversation to file first with /export.".into(),
        )
    }
}
