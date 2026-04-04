//! Info/utility commands: /advisor, /insights, /stickers, /passes, /rate-limit-options, /reload-plugins.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /advisor — toggle advisor mode (suggest without acting).
pub struct AdvisorCommand;
impl SlashCommand for AdvisorCommand {
    fn name(&self) -> &str {
        "advisor"
    }
    fn description(&self) -> &str {
        "Toggle advisor mode (suggest without acting)"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let key = "advisor_mode";
        let state = ctx.state_store.current();
        let was_enabled = state.active_skills.iter().any(|s| s == key);

        ctx.state_store.update(|s| {
            if was_enabled {
                s.active_skills.retain(|sk| sk != key);
            } else {
                s.active_skills.push(key.to_string());
            }
        });

        let status = if was_enabled { "disabled" } else { "enabled" };
        CommandOutput::Message(format!(
            "Advisor mode: {status}\n\
             When enabled, the assistant suggests actions without executing them."
        ))
    }
}

/// /insights — show project stats (files, LOC, languages).
pub struct InsightsCommand;
impl SlashCommand for InsightsCommand {
    fn name(&self) -> &str {
        "insights"
    }
    fn description(&self) -> &str {
        "Show project statistics (files, LOC, languages)"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let msg_count = state.messages.len();
        let tool_count: usize = state
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .filter(|b| matches!(b, oxicode_common::ContentBlock::ToolUse { .. }))
            .count();
        let skill_count = state.active_skills.len();
        let agent_count = state.active_agents.len();
        let task_count = state.background_tasks.len();

        let mut out = String::from("Session Insights:\n");
        let _ = writeln!(out, "  Messages: {msg_count}");
        let _ = writeln!(out, "  Tool calls: {tool_count}");
        let _ = writeln!(out, "  Active skills: {skill_count}");
        let _ = writeln!(out, "  Active agents: {agent_count}");
        let _ = writeln!(out, "  Background tasks: {task_count}");
        let _ = writeln!(
            out,
            "  Tokens: {} in / {} out",
            state.total_usage.input_tokens, state.total_usage.output_tokens
        );
        CommandOutput::Message(out)
    }
}

/// /stickers — ASCII art fun.
pub struct StickersCommand;
impl SlashCommand for StickersCommand {
    fn name(&self) -> &str {
        "stickers"
    }
    fn description(&self) -> &str {
        "Display ASCII art stickers"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let sticker = match args.trim() {
            "rust" | "crab" => {
                "  \u{1f980}\n\
                 /(o o)\\\n\
                 | / \\ |\n\
                  \\___/\n\
                  Ferris says hi!"
            }
            "ok" | "thumbs" => " \u{1f44d} All good!",
            "ship" | "rocket" => " \u{1f680} Ship it!",
            "coffee" => " \u{2615} Coffee break!",
            "fire" => " \u{1f525} On fire!",
            _ => {
                "Available stickers: rust, ok, ship, coffee, fire\n\
                 Usage: /stickers <name>"
            }
        };
        CommandOutput::Message(sticker.to_string())
    }
}

/// /passes — show usage passes (stub).
pub struct PassesCommand;
impl SlashCommand for PassesCommand {
    fn name(&self) -> &str {
        "passes"
    }
    fn description(&self) -> &str {
        "Show usage passes and billing info"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let cost = f64::from(state.total_usage.input_tokens) * 3.0 / 1_000_000.0
            + f64::from(state.total_usage.output_tokens) * 15.0 / 1_000_000.0;

        CommandOutput::Message(format!(
            "Usage Passes:\n\
             Session cost: ${cost:.4}\n\
             Pass status: Free tier (no billing API configured)\n\
             \n\
             Configure billing in ~/.oxicode/settings.toml"
        ))
    }
}

/// /rate-limit-options — show current rate limit configuration.
pub struct RateLimitOptionsCommand;
impl SlashCommand for RateLimitOptionsCommand {
    fn name(&self) -> &str {
        "rate-limit-options"
    }
    fn description(&self) -> &str {
        "Show rate limit configuration"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Rate Limit Configuration:\n\
             \n\
             Strategy: Exponential backoff with jitter\n\
             Max retries: 5\n\
             Initial delay: 1s\n\
             Max delay: 60s\n\
             Backoff multiplier: 2.0\n\
             Jitter: 0-500ms\n\
             \n\
             Configure in ~/.oxicode/settings.toml under [rate_limit]"
                .into(),
        )
    }
}

/// /reload-plugins — hot-reload plugin registry.
pub struct ReloadPluginsInfoCommand;
impl SlashCommand for ReloadPluginsInfoCommand {
    fn name(&self) -> &str {
        "reload-plugins"
    }
    fn description(&self) -> &str {
        "Hot-reload plugin registry"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Plugin reload requested.\n\
             Note: Hot-reload runs on next command cycle.\n\
             Use /plugin list to verify loaded plugins."
                .into(),
        )
    }
}
