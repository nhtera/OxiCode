//! Info/utility commands: /advisor, /insights, /stickers, /passes, /rate-limit-options, /reload-plugins.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /advisor — toggle advisor mode (suggest without acting).
///
/// When enabled, a system prompt modifier is injected telling the assistant
/// to suggest approaches and ask before executing tools.
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

        if was_enabled {
            CommandOutput::Message(
                "Advisor mode: disabled\n\
                 The assistant will execute tools and actions directly."
                    .into(),
            )
        } else {
            CommandOutput::Message(
                "Advisor mode: enabled\n\
                 The assistant will suggest approaches and ask before executing tools.\n\
                 System prompt modifier active for this session."
                    .into(),
            )
        }
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

/// /rate-limit-options — show current rate limit state and configuration.
pub struct RateLimitOptionsCommand;
impl SlashCommand for RateLimitOptionsCommand {
    fn name(&self) -> &str {
        "rate-limit-options"
    }
    fn description(&self) -> &str {
        "Show rate limit state and configuration"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();

        let mut out = String::from("Rate Limit Status:\n\n");

        // Show last rate limit event if available.
        if let Some(snapshot) = &state.last_rate_limit {
            let _ = writeln!(out, "  Last event:");
            let _ = writeln!(out, "    Provider:    {}", snapshot.provider);
            let _ = writeln!(out, "    Type:        {:?}", snapshot.info.limit_type);
            if let Some(retry) = snapshot.info.retry_after_secs {
                let _ = writeln!(out, "    Retry after: {retry:.1}s");
            }
            if let Some(remaining) = snapshot.info.remaining {
                let _ = writeln!(out, "    Remaining:   {remaining}");
            }
            if !snapshot.info.message.is_empty() {
                let _ = writeln!(out, "    Message:     {}", snapshot.info.message);
            }
            let _ = writeln!(out, "    Occurred:    {}", snapshot.occurred_at);
        } else {
            let _ = writeln!(out, "  No rate limit events in this session.");
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "Configuration:");
        let _ = writeln!(out, "  Strategy:   Exponential backoff with jitter");
        let _ = writeln!(out, "  Max retries: 5");
        let _ = writeln!(out, "  Backoff:    1s initial, 60s max, 2.0x multiplier");
        let _ = writeln!(out, "  Jitter:     0-500ms");
        let _ = writeln!(out);
        out.push_str("  Configure in ~/.oxicode/settings.toml under [rate_limit]");

        CommandOutput::Message(out)
    }
}

/// /reload-plugins — trigger plugin hot-reload.
///
/// Note: the actual async reload is handled by the engine event loop.
/// This command sets a flag in active_skills that the engine reads on the
/// next turn to trigger `PluginManager::reload_plugins()`.
pub struct ReloadPluginsInfoCommand;
impl SlashCommand for ReloadPluginsInfoCommand {
    fn name(&self) -> &str {
        "reload-plugins"
    }
    fn description(&self) -> &str {
        "Hot-reload plugin registry"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        // Set a flag that the engine event loop checks to trigger async reload.
        ctx.state_store.update(|s| {
            if !s
                .active_skills
                .iter()
                .any(|sk| sk == "reload_plugins_requested")
            {
                s.active_skills.push("reload_plugins_requested".to_string());
            }
        });

        CommandOutput::Message(
            "Plugin hot-reload requested.\n\
             Plugins will be reloaded on the next engine cycle.\n\
             Use /plugin list to verify loaded plugins."
                .into(),
        )
    }
}
