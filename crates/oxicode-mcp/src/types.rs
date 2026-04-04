//! Type bridge: re-export rmcp types with backward-compatible naming.
//!
//! Keeps downstream callers (oxicode-core, oxicode-cli) stable while
//! the underlying types come from the rmcp crate.

use rmcp::model;

/// Re-export rmcp Tool as McpToolDef for backward compatibility.
pub type McpToolDef = model::Tool;

/// MCP tool call result.
pub type McpToolResult = model::CallToolResult;

/// MCP resource definition.
pub type McpResource = model::Resource;

/// MCP prompt definition.
pub type McpPrompt = model::Prompt;

/// Prompt message in a get_prompt result.
pub type McpPromptMessage = model::PromptMessage;

/// Result from get_prompt — description + messages.
pub type McpGetPromptResult = model::GetPromptResult;

/// MCP root (project directory declaration for servers).
pub type McpRoot = model::Root;

/// MCP server capabilities (from initialize response).
pub type ServerCapabilities = model::ServerCapabilities;

/// Content block from tool results (text, image, resource, etc.).
pub type McpContent = model::Content;

/// Resource contents (text or blob) from read_resource results.
pub type McpResourceContents = model::ResourceContents;

/// Convert an rmcp Tool to a Claude-format tool schema (JSON).
///
/// Produces the same format as built-in tools for a unified LLM tool list.
/// Replaces the deleted tool_adapter.rs.
pub fn mcp_tool_to_schema(server_name: &str, tool: &model::Tool) -> serde_json::Value {
    let prefixed_name = format!("{server_name}__{}", tool.name);
    let default_desc = format!("MCP tool: {}", tool.name);
    let description = tool.description.as_deref().unwrap_or(&default_desc);
    let input_schema = serde_json::to_value(tool.input_schema.as_ref())
        .unwrap_or_else(|_| serde_json::json!({"type": "object", "properties": {}}));

    serde_json::json!({
        "name": prefixed_name,
        "description": format!("[MCP:{server_name}] {description}"),
        "input_schema": input_schema,
    })
}

/// Convert all tools from a server to schema JSON values.
pub fn mcp_tools_to_schemas(server_name: &str, tools: &[model::Tool]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| mcp_tool_to_schema(server_name, t))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_tool(name: &str, desc: Option<&str>, schema: serde_json::Value) -> model::Tool {
        let schema_map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(schema).unwrap_or_default();
        model::Tool::new(name.to_string(), desc.unwrap_or("").to_string(), Arc::new(schema_map))
    }

    #[test]
    fn test_mcp_tool_to_schema_basic() {
        let tool = make_tool(
            "read_file",
            Some("Read a file from disk"),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        );

        let schema = mcp_tool_to_schema("filesystem", &tool);
        assert_eq!(schema["name"], "filesystem__read_file");
        assert!(schema["description"]
            .as_str()
            .unwrap()
            .contains("[MCP:filesystem]"));
        assert!(schema["description"]
            .as_str()
            .unwrap()
            .contains("Read a file from disk"));
        assert_eq!(schema["input_schema"]["type"], "object");
        assert!(schema["input_schema"]["properties"]["path"].is_object());
    }

    #[test]
    fn test_mcp_tool_to_schema_no_description() {
        let tool = make_tool("my_tool", None, serde_json::json!({}));
        // Tool::new always sets description, but mcp_tool_to_schema handles empty string
        let schema = mcp_tool_to_schema("srv", &tool);
        assert_eq!(schema["name"], "srv__my_tool");
        // Should have MCP prefix
        assert!(schema["description"]
            .as_str()
            .unwrap()
            .starts_with("[MCP:srv]"));
    }

    #[test]
    fn test_batch_conversion() {
        let tools = vec![
            make_tool("alpha", Some("Tool A"), serde_json::json!({})),
            make_tool("beta", Some("Tool B"), serde_json::json!({})),
            make_tool("gamma", Some("Tool C"), serde_json::json!({})),
        ];

        let schemas = mcp_tools_to_schemas("srv", &tools);
        assert_eq!(schemas.len(), 3);
        assert_eq!(schemas[0]["name"], "srv__alpha");
        assert_eq!(schemas[1]["name"], "srv__beta");
        assert_eq!(schemas[2]["name"], "srv__gamma");
    }

    #[test]
    fn test_batch_conversion_empty() {
        let schemas = mcp_tools_to_schemas("srv", &[]);
        assert!(schemas.is_empty());
    }

    #[test]
    fn test_schema_prefixes_server_name() {
        let tool = make_tool("execute", Some("Run code"), serde_json::json!({}));
        let schema = mcp_tool_to_schema("code-runner", &tool);
        assert_eq!(schema["name"], "code-runner__execute");
        assert!(schema["description"]
            .as_str()
            .unwrap()
            .contains("[MCP:code-runner]"));
    }
}
