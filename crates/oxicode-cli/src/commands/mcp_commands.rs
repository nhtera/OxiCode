//! Extended MCP slash commands: /mcp connect, /mcp disconnect, /mcp servers, /mcp tools.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct McpConnectCommand;
impl SlashCommand for McpConnectCommand {
    fn name(&self) -> &str {
        "mcp-connect"
    }
    fn description(&self) -> &str {
        "Connect to an MCP server"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Error("Usage: /mcp-connect <server-name>".into())
        } else {
            CommandOutput::Message(format!("Connecting to MCP server: {args}"))
        }
    }
}

pub struct McpDisconnectCommand;
impl SlashCommand for McpDisconnectCommand {
    fn name(&self) -> &str {
        "mcp-disconnect"
    }
    fn description(&self) -> &str {
        "Disconnect from an MCP server"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        if args.is_empty() {
            CommandOutput::Error("Usage: /mcp-disconnect <server-name>".into())
        } else {
            CommandOutput::Message(format!("Disconnecting: {args}"))
        }
    }
}

pub struct McpToolsCommand;
impl SlashCommand for McpToolsCommand {
    fn name(&self) -> &str {
        "mcp-tools"
    }
    fn description(&self) -> &str {
        "List tools from MCP servers"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("MCP tools: (none discovered yet)".into())
    }
}

pub struct McpServersCommand;
impl SlashCommand for McpServersCommand {
    fn name(&self) -> &str {
        "mcp-servers"
    }
    fn description(&self) -> &str {
        "List configured MCP servers"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("MCP servers: see /mcp for details".into())
    }
}
