//! `/buddy` — start, stop, and inspect the background buddy agent.
//!
//! State is persisted in `active_skills` as the token `"buddy:active"`.

use super::{CommandContext, CommandOutput, SlashCommand};

const BUDDY_SKILL_KEY: &str = "buddy:active";

/// `/buddy [start|stop]` — manage the buddy background agent.
pub struct BuddyCommand;

impl SlashCommand for BuddyCommand {
    fn name(&self) -> &str {
        "buddy"
    }

    fn description(&self) -> &str {
        "Start/stop the buddy background agent"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        match args.trim() {
            "start" => start_buddy(ctx),
            "stop" => stop_buddy(ctx),
            "" => buddy_status(ctx),
            other => CommandOutput::Error(format!(
                "Unknown buddy subcommand: '{other}'\n\
                 Usage: /buddy [start|stop]\n\
                 No argument shows current status."
            )),
        }
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["start", "stop"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Sub-command handlers
// ---------------------------------------------------------------------------

/// Activate the buddy agent.
fn start_buddy(ctx: &CommandContext) -> CommandOutput {
    let already_active = ctx
        .state_store
        .current()
        .active_skills
        .iter()
        .any(|s| s == BUDDY_SKILL_KEY);

    if already_active {
        return CommandOutput::Message(
            "Buddy agent is already running.\n\
             Use /buddy stop to stop it."
                .to_string(),
        );
    }

    ctx.state_store.update(|s| {
        s.active_skills.push(BUDDY_SKILL_KEY.to_string());
    });

    tracing::debug!("Buddy agent started");
    CommandOutput::Message(
        "Buddy agent started.\n\
         The buddy will proactively suggest improvements, flag issues, and\n\
         offer helpful prompts as you work.\n\n\
         Use /buddy stop to stop it."
            .to_string(),
    )
}

/// Deactivate the buddy agent.
fn stop_buddy(ctx: &CommandContext) -> CommandOutput {
    let was_active = ctx
        .state_store
        .current()
        .active_skills
        .iter()
        .any(|s| s == BUDDY_SKILL_KEY);

    if !was_active {
        return CommandOutput::Message(
            "Buddy agent is not running.\n\
             Use /buddy start to start it."
                .to_string(),
        );
    }

    ctx.state_store.update(|s| {
        s.active_skills.retain(|sk| sk != BUDDY_SKILL_KEY);
    });

    tracing::debug!("Buddy agent stopped");
    CommandOutput::Message("Buddy agent stopped.".to_string())
}

/// Show whether the buddy is currently active.
fn buddy_status(ctx: &CommandContext) -> CommandOutput {
    let active = ctx
        .state_store
        .current()
        .active_skills
        .iter()
        .any(|s| s == BUDDY_SKILL_KEY);

    if active {
        CommandOutput::Message(
            "Buddy agent status: running\n\
             Use /buddy stop to stop it."
                .to_string(),
        )
    } else {
        CommandOutput::Message(
            "Buddy agent status: stopped\n\
             Use /buddy start to start it."
                .to_string(),
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_ctx() -> CommandContext {
        CommandContext {
            state_store: Arc::new(oxicode_state::StateStore::default()),
            model: "test".to_string(),
            provider_name: "test".to_string(),
            session_id: "test".to_string(),
            command_registry: Arc::new(crate::commands::default_registry()),
        }
    }

    #[test]
    fn test_status_initially_stopped() {
        let cmd = BuddyCommand;
        let ctx = make_ctx();
        match cmd.execute("", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("stopped")),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_start_stop_cycle() {
        let cmd = BuddyCommand;
        let ctx = make_ctx();

        // Start
        match cmd.execute("start", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("started")),
            _ => panic!("expected Message"),
        }
        // Status should show running
        match cmd.execute("", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("running")),
            _ => panic!("expected Message"),
        }
        // Stop
        match cmd.execute("stop", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("stopped")),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_unknown_subcommand_is_error() {
        let cmd = BuddyCommand;
        let ctx = make_ctx();
        match cmd.execute("fly", &ctx) {
            CommandOutput::Error(_) => {}
            _ => panic!("expected Error"),
        }
    }
}
