//! MCP resource tools: list and read resources from connected MCP servers.

use std::fmt::Write as _;

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// List resources from all connected MCP servers.
pub struct ListMcpResourcesTool;

#[async_trait]
impl Tool for ListMcpResourcesTool {
    fn name(&self) -> &str {
        "list_mcp_resources"
    }

    fn description(&self) -> &str {
        "List resources available from connected MCP servers"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list_mcp_resources".into(),
            description: "List resources available from connected MCP servers. \
                Returns resource URIs, names, and descriptions grouped by server."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "server": {
                        "type": "string",
                        "description": "Optional: filter to a specific MCP server name"
                    }
                },
                "required": []
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let server_filter = input.get("server").and_then(|v| v.as_str());
        let all_resources = ctx.mcp_manager.list_resources().await;

        if all_resources.is_empty() {
            return Ok(ToolResult::success("No MCP resources available."));
        }

        let mut output = String::new();
        for (server_name, resources) in &all_resources {
            if let Some(filter) = server_filter {
                if server_name != filter {
                    continue;
                }
            }
            let _ = writeln!(output, "## Server: {server_name}");
            for res in resources {
                let name = &res.name;
                let desc = res
                    .description
                    .as_deref()
                    .map(|d| format!(" — {d}"))
                    .unwrap_or_default();
                let mime = res
                    .mime_type
                    .as_deref()
                    .map(|m| format!(" [{m}]"))
                    .unwrap_or_default();
                let _ = writeln!(output, "- `{}` {name}{desc}{mime}", res.uri);
            }
            output.push('\n');
        }

        if output.is_empty() {
            Ok(ToolResult::success(
                "No resources found for the specified server.",
            ))
        } else {
            Ok(ToolResult::success(output))
        }
    }
}

/// Read a specific resource by URI from an MCP server.
pub struct ReadMcpResourceTool;

#[async_trait]
impl Tool for ReadMcpResourceTool {
    fn name(&self) -> &str {
        "read_mcp_resource"
    }

    fn description(&self) -> &str {
        "Read a resource from a connected MCP server by URI"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read_mcp_resource".into(),
            description: "Read a resource from a connected MCP server. \
                Provide the server name and resource URI."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "server": {
                        "type": "string",
                        "description": "MCP server name that hosts the resource"
                    },
                    "uri": {
                        "type": "string",
                        "description": "Resource URI (e.g., file:///path or custom://resource)"
                    }
                },
                "required": ["server", "uri"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let server = input["server"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Other("Missing 'server' parameter".into()))?;
        let uri = input["uri"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Other("Missing 'uri' parameter".into()))?;

        let result = ctx.mcp_manager.read_resource(server, uri).await?;
        let contents = &result.contents;

        if contents.is_empty() {
            return Ok(ToolResult::success("(empty resource)"));
        }

        // Collect text content from resource contents.
        let text: String = contents
            .iter()
            .filter_map(|c| match c {
                oxicode_mcp::McpResourceContents::TextResourceContents { text, .. } => {
                    Some(text.as_str())
                }
                oxicode_mcp::McpResourceContents::BlobResourceContents { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let has_non_text = contents.iter().any(|c| {
            matches!(
                c,
                oxicode_mcp::McpResourceContents::BlobResourceContents { .. }
            )
        });

        let mut output = if text.is_empty() {
            "(no text content)".to_string()
        } else {
            text
        };

        if has_non_text {
            output.push_str("\n[Note: non-text content was returned but omitted]");
        }

        Ok(ToolResult::success(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tool_schema_valid() {
        let tool = ListMcpResourcesTool;
        let schema = tool.schema();
        assert_eq!(schema.name, "list_mcp_resources");
        assert_eq!(schema.input_schema["type"], "object");
    }

    #[test]
    fn read_tool_schema_valid() {
        let tool = ReadMcpResourceTool;
        let schema = tool.schema();
        assert_eq!(schema.name, "read_mcp_resource");
        let required = schema.input_schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
    }
}
