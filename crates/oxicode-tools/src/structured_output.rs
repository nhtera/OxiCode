//! Structured output tool: return data in a specific JSON schema format.
//!
//! Allows the LLM to output structured JSON data matching a user-defined schema.

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

pub struct StructuredOutputTool;

#[async_trait]
impl Tool for StructuredOutputTool {
    fn name(&self) -> &str {
        "structured_output"
    }
    fn description(&self) -> &str {
        "Return structured JSON data matching a schema. Use when you need to output typed, parseable data."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "data": {
                        "description": "The structured JSON data to return"
                    },
                    "schema_name": {
                        "type": "string",
                        "description": "Name/label for this structured output"
                    }
                },
                "required": ["data"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(data) = input.get("data") else {
            return Ok(ToolResult::error("data field is required"));
        };

        let output = serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string());

        Ok(ToolResult::success(output))
    }
}
