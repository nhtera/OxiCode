//! MCP Auth tool: initiate OAuth authentication flows for MCP servers.
//!
//! When an MCP server requires authentication, this tool launches the OAuth
//! flow and stores tokens in `~/.oxicode/mcp-auth/`.

use std::path::PathBuf;

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

fn auth_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".oxicode")
        .join("mcp-auth")
}

pub struct McpAuthTool;

#[async_trait]
impl Tool for McpAuthTool {
    fn name(&self) -> &str {
        "McpAuth"
    }
    fn description(&self) -> &str {
        "Initiate OAuth authentication for an MCP server that requires credentials."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "server_name": {
                        "type": "string",
                        "description": "Name of the MCP server to authenticate with"
                    },
                    "auth_url": {
                        "type": "string",
                        "description": "OAuth authorization URL (if known)"
                    }
                },
                "required": ["server_name"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(server_name) = input.get("server_name").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("server_name is required"));
        };

        let auth_url = input.get("auth_url").and_then(|v| v.as_str());

        // Check if we already have stored auth for this server
        let token_path = auth_dir().join(format!("{server_name}.json"));
        if token_path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&token_path) {
                if let Ok(token_data) = serde_json::from_str::<serde_json::Value>(&contents) {
                    // Check if token hasn't expired
                    if let Some(expires_at) = token_data
                        .get("expires_at")
                        .and_then(serde_json::Value::as_i64)
                    {
                        let now = chrono::Utc::now().timestamp();
                        if now < expires_at {
                            let result = serde_json::json!({
                                "status": "already_authenticated",
                                "server_name": server_name,
                                "message": "Valid authentication token exists for this server.",
                            });
                            return Ok(ToolResult::success(
                                serde_json::to_string_pretty(&result).unwrap_or_default(),
                            ));
                        }
                    }
                }
            }
        }

        // Check if the MCP manager knows about this server
        let servers = ctx.mcp_manager.server_names();
        let server_exists = servers.contains(&server_name);

        if !server_exists {
            return Ok(ToolResult::error(format!(
                "MCP server '{server_name}' is not configured. Add it to settings first."
            )));
        }

        // Build the auth response
        let result = if let Some(url) = auth_url {
            // If auth URL provided, direct user to it
            serde_json::json!({
                "status": "auth_url",
                "server_name": server_name,
                "authUrl": url,
                "message": format!("Open this URL to authenticate with '{server_name}': {url}"),
            })
        } else {
            // No auth URL — server needs to provide one via its protocol
            serde_json::json!({
                "status": "unsupported",
                "server_name": server_name,
                "message": format!(
                    "MCP server '{server_name}' requires authentication but no auth URL is available. \
                     The server may need to be configured with OAuth credentials."
                ),
            })
        };

        Ok(ToolResult::success(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }
}
