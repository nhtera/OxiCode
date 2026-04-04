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

        // TODO(gap-phase-3): implement real tool search with fuzzy matching.
        // For now, return a placeholder — the query engine will
        // intercept this tool and inject the actual registry data.
        Ok(ToolResult::success(format!(
            "[TOOL_SEARCH] query={query} — registry injection pending"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_and_description() {
        let tool = ToolSearchTool;
        assert_eq!(tool.name(), "tool_search");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_schema_has_query_required() {
        let tool = ToolSearchTool;
        let schema = tool.schema();
        assert_eq!(schema.name, "tool_search");
        let required = schema.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "query"));
    }

    #[test]
    fn test_permission_level() {
        let tool = ToolSearchTool;
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
    }

    #[tokio::test]
    async fn test_execute_with_query() {
        let tool = ToolSearchTool;
        let input = serde_json::json!({"query": "bash"});
        let result = tool.execute(input, &ToolContext::default()).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("bash"));
    }

    #[tokio::test]
    async fn test_execute_empty_query() {
        let tool = ToolSearchTool;
        let input = serde_json::json!({});
        let result = tool.execute(input, &ToolContext::default()).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("TOOL_SEARCH"));
    }
}
