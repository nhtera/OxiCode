//! Extended MCP slash commands: /mcp-connect, /mcp-disconnect, /mcp-servers, /mcp-tools.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /mcp-connect <name> — show how to add an MCP server.
pub struct McpConnectCommand;
impl SlashCommand for McpConnectCommand {
    fn name(&self) -> &str {
        "mcp-connect"
    }
    fn description(&self) -> &str {
        "Connect to an MCP server"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.trim().is_empty() {
            return CommandOutput::Error("Usage: /mcp-connect <server-name>".into());
        }
        CommandOutput::Message(format!(
            "To connect '{}':\n\
             1. Add to ~/.oxicode/mcp.toml:\n\n\
             [servers.{}]\n\
             transport = \"stdio\"\n\
             command = \"npx\"\n\
             args = [\"-y\", \"@modelcontextprotocol/{}-server\"]\n\
             enabled = true\n\n\
             2. Restart OxiCode to activate.",
            args.trim(),
            args.trim(),
            args.trim()
        ))
    }
}

/// /mcp-disconnect <name> — show how to remove an MCP server.
pub struct McpDisconnectCommand;
impl SlashCommand for McpDisconnectCommand {
    fn name(&self) -> &str {
        "mcp-disconnect"
    }
    fn description(&self) -> &str {
        "Disconnect from an MCP server"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.trim().is_empty() {
            return CommandOutput::Error("Usage: /mcp-disconnect <server-name>".into());
        }
        CommandOutput::Message(format!(
            "To disconnect '{}':\n\
             Set enabled = false in ~/.oxicode/mcp.toml:\n\n\
             [servers.{}]\n\
             enabled = false\n\n\
             Then restart OxiCode.",
            args.trim(),
            args.trim()
        ))
    }
}

/// /mcp-tools — list tools from configured MCP servers.
pub struct McpToolsCommand;
impl SlashCommand for McpToolsCommand {
    fn name(&self) -> &str {
        "mcp-tools"
    }
    fn description(&self) -> &str {
        "List tools from MCP servers"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let config = oxicode_mcp::McpConfig::load();
        let servers: Vec<_> = config.enabled_servers().collect();

        if servers.is_empty() {
            return CommandOutput::Message(
                "No MCP servers configured.\nAdd servers to ~/.oxicode/mcp.toml".into(),
            );
        }

        let mut output = String::from("MCP server tools:\n");
        for (name, _cfg) in &servers {
            let _ = writeln!(
                output,
                "  [{name}] (tools discovered at runtime via MCP protocol)"
            );
        }
        let _ = writeln!(
            output,
            "\nTools are discovered dynamically when servers connect."
        );
        CommandOutput::Message(output)
    }
}

/// /mcp-servers — list configured MCP servers with transport details.
pub struct McpServersCommand;
impl SlashCommand for McpServersCommand {
    fn name(&self) -> &str {
        "mcp-servers"
    }
    fn description(&self) -> &str {
        "List configured MCP servers"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let config = oxicode_mcp::McpConfig::load();
        let servers: Vec<_> = config.enabled_servers().collect();

        if servers.is_empty() {
            return CommandOutput::Message(
                "No MCP servers configured.\nAdd servers to ~/.oxicode/mcp.toml".into(),
            );
        }

        let mut output = String::from("MCP servers:\n");
        for (name, cfg) in &servers {
            let transport = match &cfg.transport {
                oxicode_mcp::config::McpTransportType::Stdio => {
                    cfg.command.as_deref().unwrap_or("stdio")
                }
                oxicode_mcp::config::McpTransportType::Sse => cfg.url.as_deref().unwrap_or("sse"),
                oxicode_mcp::config::McpTransportType::Http => cfg.url.as_deref().unwrap_or("http"),
            };
            let _ = writeln!(output, "  {name:<20} [{transport}]");
        }
        CommandOutput::Message(output)
    }
}

/// /mcp-doctor — run connectivity diagnostics on all configured MCP servers.
pub struct McpDoctorCommand;

impl SlashCommand for McpDoctorCommand {
    fn name(&self) -> &str {
        "mcp-doctor"
    }
    fn description(&self) -> &str {
        "Diagnose MCP server connectivity"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let config = oxicode_mcp::McpConfig::load();
        let servers: Vec<_> = config.enabled_servers().collect();

        if servers.is_empty() {
            return CommandOutput::Message(
                "No MCP servers configured.\nAdd servers to ~/.oxicode/mcp.toml".into(),
            );
        }

        let mut output = String::from("MCP server diagnostics:\n\n");
        for (name, cfg) in &servers {
            let transport = match &cfg.transport {
                oxicode_mcp::config::McpTransportType::Stdio => "stdio",
                oxicode_mcp::config::McpTransportType::Sse => "sse",
                oxicode_mcp::config::McpTransportType::Http => "http",
            };
            let _ = writeln!(output, "  {name:<20} [{transport}]");

            // Check basic config validity.
            match &cfg.transport {
                oxicode_mcp::config::McpTransportType::Stdio => {
                    if cfg.command.is_none() {
                        let _ = writeln!(output, "    ⚠ No command configured");
                    }
                }
                oxicode_mcp::config::McpTransportType::Sse
                | oxicode_mcp::config::McpTransportType::Http => {
                    if cfg.url.is_none() {
                        let _ = writeln!(output, "    ⚠ No URL configured");
                    }
                }
            }

            if !cfg.allowed_tools.is_empty() {
                let _ = writeln!(
                    output,
                    "    Allowed tools: {}",
                    cfg.allowed_tools.join(", ")
                );
            }
            if !cfg.blocked_tools.is_empty() {
                let _ = writeln!(
                    output,
                    "    Blocked tools: {}",
                    cfg.blocked_tools.join(", ")
                );
            }
        }

        let _ = writeln!(
            output,
            "\n{} server(s) configured. Full connectivity test runs at startup.",
            servers.len()
        );

        CommandOutput::Message(output)
    }
}
