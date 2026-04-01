//! McpToolAdapter: wraps MCP tools to appear as native oxicode tools.
//!
//! Converts MCP tool definitions to the Tool trait interface so they
//! integrate seamlessly into the ToolRegistry.

use crate::protocol::McpToolDef;

/// Convert an MCP tool definition to a Claude-format tool schema (JSON).
///
/// This produces the same format as built-in tools so the LLM sees
/// a unified tool list.
pub fn mcp_tool_to_schema(server_name: &str, tool: &McpToolDef) -> serde_json::Value {
    let prefixed_name = format!("{server_name}__{}", tool.name);
    let description = tool
        .description
        .clone()
        .unwrap_or_else(|| format!("MCP tool: {}", tool.name));
    let input_schema = tool
        .input_schema
        .clone()
        .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));

    serde_json::json!({
        "name": prefixed_name,
        "description": format!("[MCP:{server_name}] {description}"),
        "input_schema": input_schema,
    })
}

/// Convert all tools from a server to schema JSON values.
pub fn mcp_tools_to_schemas(server_name: &str, tools: &[McpToolDef]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| mcp_tool_to_schema(server_name, t))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_to_schema() {
        let tool = McpToolDef {
            name: "read_file".to_string(),
            description: Some("Read a file from disk".to_string()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            })),
        };

        let schema = mcp_tool_to_schema("filesystem", &tool);
        assert_eq!(schema["name"], "filesystem__read_file");
        assert!(schema["description"]
            .as_str()
            .unwrap()
            .contains("[MCP:filesystem]"));
        assert_eq!(schema["input_schema"]["type"], "object");
    }

    #[test]
    fn test_batch_conversion() {
        let tools = vec![
            McpToolDef {
                name: "a".to_string(),
                description: None,
                input_schema: None,
            },
            McpToolDef {
                name: "b".to_string(),
                description: Some("Tool B".to_string()),
                input_schema: None,
            },
        ];

        let schemas = mcp_tools_to_schemas("srv", &tools);
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0]["name"], "srv__a");
        assert_eq!(schemas[1]["name"], "srv__b");
    }
}
