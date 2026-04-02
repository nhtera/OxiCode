//! Provider-related commands: /model, /permissions, /hooks, /mcp, /cost.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /model — show or switch the active model.
pub struct ModelCommand;

impl SlashCommand for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }
    fn description(&self) -> &str {
        "Show or switch model (e.g., /model gpt-4o)"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            return CommandOutput::Message(format!(
                "Current model: {} (provider: {})",
                ctx.model, ctx.provider_name,
            ));
        }

        // Model switching requires engine mutation — signal it to the TUI loop.
        // For now, report what would happen.
        CommandOutput::Message(format!(
            "Model switch to '{}' requested. Restart with --model {0} for now.",
            args.trim()
        ))
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        let known_models = [
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "claude-haiku-4-5-20251001",
            "gpt-4o",
            "gpt-4-turbo",
            "deepseek-chat",
            "deepseek-reasoner",
        ];
        known_models
            .iter()
            .filter(|m| m.starts_with(partial))
            .map(|m| (*m).to_string())
            .collect()
    }
}

/// /permissions — show current permission mode and rules.
pub struct PermissionsCommand;

impl SlashCommand for PermissionsCommand {
    fn name(&self) -> &str {
        "permissions"
    }
    fn description(&self) -> &str {
        "Show permission settings"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Permission system active. Use settings.toml to configure permission_mode.".to_string(),
        )
    }
}

/// /hooks — list configured hooks and their status.
pub struct HooksCommand;

impl SlashCommand for HooksCommand {
    fn name(&self) -> &str {
        "hooks"
    }
    fn description(&self) -> &str {
        "List configured hooks"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let config = oxicode_hooks::HooksConfig::load_from_settings_dir();
        if config.hooks.is_empty() {
            return CommandOutput::Message("No hooks configured.".to_string());
        }

        let mut output = String::from("Configured hooks:\n");
        for (event, def) in &config.hooks {
            let status = if def.enabled { "enabled" } else { "disabled" };
            let _ = writeln!(output, "  {event:<20} {status:<10} {}", def.command);
        }
        CommandOutput::Message(output)
    }
}

/// /mcp — list MCP server connections.
pub struct McpCommand;

impl SlashCommand for McpCommand {
    fn name(&self) -> &str {
        "mcp"
    }
    fn description(&self) -> &str {
        "List MCP server connections"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let config = oxicode_mcp::McpConfig::load();
        let servers: Vec<_> = config.enabled_servers().collect();

        if servers.is_empty() {
            return CommandOutput::Message(
                "No MCP servers configured. Add servers to ~/.oxicode/mcp.toml".to_string(),
            );
        }

        let mut output = String::from("MCP servers:\n");
        for (name, cfg) in servers {
            let transport = match &cfg.transport {
                oxicode_mcp::config::McpTransportType::Stdio => {
                    cfg.command.as_deref().unwrap_or("stdio")
                }
                oxicode_mcp::config::McpTransportType::Sse => cfg.url.as_deref().unwrap_or("sse"),
                oxicode_mcp::config::McpTransportType::WebSocket => {
                    cfg.url.as_deref().unwrap_or("websocket")
                }
            };
            let _ = writeln!(output, "  {name:<20} {transport}");
        }
        CommandOutput::Message(output)
    }
}

/// /cost — show token usage and estimated cost.
pub struct CostCommand;

impl SlashCommand for CostCommand {
    fn name(&self) -> &str {
        "cost"
    }
    fn description(&self) -> &str {
        "Show token usage and estimated cost"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let usage = &state.total_usage;

        // Rough cost estimates for Claude Sonnet.
        let input_cost = f64::from(usage.input_tokens) * 3.0 / 1_000_000.0;
        let output_cost = f64::from(usage.output_tokens) * 15.0 / 1_000_000.0;
        let total = input_cost + output_cost;

        CommandOutput::Message(format!(
            "Tokens: {}in / {}out\nEstimated cost: ${total:.4} (${input_cost:.4} in + ${output_cost:.4} out)",
            usage.input_tokens, usage.output_tokens,
        ))
    }
}
