//! Git slash commands: /commit, /pr, /branch, /log, /stash, /push, /pull.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct CommitCommand;
impl SlashCommand for CommitCommand {
    fn name(&self) -> &str { "commit" }
    fn description(&self) -> &str { "Create a git commit" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let msg = if args.is_empty() { "auto-generated" } else { args };
        CommandOutput::Message(format!("Committing with message: {msg}"))
    }
}

pub struct PrCommand;
impl SlashCommand for PrCommand {
    fn name(&self) -> &str { "pr" }
    fn description(&self) -> &str { "Create a GitHub pull request" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message("Creating PR from current branch...".into())
        } else {
            CommandOutput::Message(format!("Creating PR: {args}"))
        }
    }
}

pub struct BranchCommand;
impl SlashCommand for BranchCommand {
    fn name(&self) -> &str { "branch" }
    fn description(&self) -> &str { "Show or switch git branch" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Message("Current branch: (git branch --show-current)".into())
        } else {
            CommandOutput::Message(format!("Switching to branch: {args}"))
        }
    }
}

pub struct LogCommand;
impl SlashCommand for LogCommand {
    fn name(&self) -> &str { "log" }
    fn description(&self) -> &str { "Show recent git log" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Recent commits: (git log --oneline -10)".into())
    }
}

pub struct StashCommand;
impl SlashCommand for StashCommand {
    fn name(&self) -> &str { "stash" }
    fn description(&self) -> &str { "Git stash operations" }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        match args {
            "pop" => CommandOutput::Message("Popping stash...".into()),
            "list" => CommandOutput::Message("Stashes: (git stash list)".into()),
            _ => CommandOutput::Message("Stashing changes...".into()),
        }
    }
}

pub struct PushCommand;
impl SlashCommand for PushCommand {
    fn name(&self) -> &str { "push" }
    fn description(&self) -> &str { "Push to remote" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Pushing to remote...".into())
    }
}

pub struct PullCommand;
impl SlashCommand for PullCommand {
    fn name(&self) -> &str { "pull" }
    fn description(&self) -> &str { "Pull from remote" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Pulling from remote...".into())
    }
}
