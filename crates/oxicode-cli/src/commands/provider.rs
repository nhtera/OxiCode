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

        // Signal the engine task to switch the model at runtime.
        CommandOutput::SwitchModel(args.trim().to_string())
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

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let config_path = dirs::home_dir()
            .map(|h| h.join(".oxicode").join("settings.toml"))
            .unwrap_or_default();

        // Read permission_mode from config file if it exists.
        let mode = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|content| {
                    content
                        .lines()
                        .find(|l| l.starts_with("permission_mode"))
                        .and_then(|l| l.split('=').nth(1))
                        .map(|v| v.trim().trim_matches('"').to_string())
                })
                .unwrap_or_else(|| "default".to_string())
        } else {
            "default".to_string()
        };

        if !args.trim().is_empty() {
            let info = match args.trim() {
                "bypass" => {
                    "bypass: Skips all permission checks except hard security denials.\n\
                             WARNING: All tools auto-approved, including shell execution."
                }
                "default" => {
                    "default: Asks approval for file writes and shell execution.\n\
                              Read-only tools (glob, grep, read) are auto-approved."
                }
                "approval_only" => {
                    "approval_only: Asks for every non-readonly tool invocation.\n\
                     Most restrictive mode for maximum control."
                }
                other => {
                    return CommandOutput::Error(format!(
                        "Unknown mode: {other}. Available: default, bypass, approval_only"
                    ));
                }
            };
            return CommandOutput::Message(info.to_string());
        }

        let mode_desc = match mode.as_str() {
            "bypass" => "bypass (all tools auto-approved)",
            "approval_only" => "approval_only (ask for every tool)",
            _ => "default (ask for writes & shell)",
        };

        CommandOutput::Message(format!(
            "Permission mode: {mode_desc}\n\
             Config: {}\n\n\
             Modes: default, bypass, approval_only\n\
             Set via: permission_mode = \"<mode>\" in settings.toml\n\
             Info: /permissions <mode>",
            config_path.display(),
        ))
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["default", "bypass", "approval_only"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
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
                oxicode_mcp::config::McpTransportType::Http => cfg.url.as_deref().unwrap_or("http"),
            };
            let _ = writeln!(output, "  {name:<20} {transport}");
        }
        CommandOutput::Message(output)
    }
}

/// /cost — show token usage and estimated cost with per-model breakdown.
pub struct CostCommand;

impl SlashCommand for CostCommand {
    fn name(&self) -> &str {
        "cost"
    }
    fn description(&self) -> &str {
        "Show token usage and estimated cost"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        use oxicode_state::cost_tracker::CostTracker;
        use std::fmt::Write;

        let state = ctx.state_store.current();
        let tracker = &state.cost_tracker;
        let total = tracker.total_cost();
        let (total_in, total_out) = tracker.total_tokens();

        let mut output = String::new();
        let _ = writeln!(output, "Total cost: {}", CostTracker::format_cost(total));
        if tracker.has_unknown_model {
            let _ = writeln!(
                output,
                "(costs may be inaccurate — unknown model pricing used)"
            );
        }
        let _ = writeln!(output, "Tokens: {total_in} in / {total_out} out");

        let summary = tracker.summary();
        if summary.len() > 1 {
            let _ = writeln!(output, "\nBy model:");
            for (model, usage) in &summary {
                let _ = writeln!(
                    output,
                    "  {model}: {} in / {} out ({}) {}",
                    usage.input_tokens,
                    usage.output_tokens,
                    CostTracker::format_cost(usage.cost_usd),
                    if usage.cache_read_tokens > 0 || usage.cache_write_tokens > 0 {
                        format!(
                            "[cache: {} read, {} write]",
                            usage.cache_read_tokens, usage.cache_write_tokens
                        )
                    } else {
                        String::new()
                    },
                );
            }
        } else if let Some((model, usage)) = summary.first() {
            let _ = writeln!(output, "Model: {model}");
            if usage.cache_read_tokens > 0 || usage.cache_write_tokens > 0 {
                let _ = writeln!(
                    output,
                    "Cache: {} read / {} write",
                    usage.cache_read_tokens, usage.cache_write_tokens
                );
            }
        }

        CommandOutput::Message(output.trim_end().to_string())
    }
}
