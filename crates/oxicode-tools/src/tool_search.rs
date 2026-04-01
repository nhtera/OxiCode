use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// List all available tools and their schemas.
pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Search and list available tools with their descriptions and schemas."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query to filter tools by name or description"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let query = input["query"].as_str().unwrap_or("");

        // This tool needs access to the registry at runtime.
        // For now, return a placeholder — the query engine will
        // intercept this tool and inject the actual registry data.
        Ok(ToolResult::success(format!(
            "[TOOL_SEARCH] query={query} — registry injection pending"
        )))
    }
}
