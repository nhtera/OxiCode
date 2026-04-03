//! SyntheticOutput tool: return structured output to the LLM without side effects.
//!
//! A passthrough tool that validates input against a provided JSON schema and
//! returns it as structured data. Used in non-interactive/SDK sessions.

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

pub struct SyntheticOutputTool;

#[async_trait]
impl Tool for SyntheticOutputTool {
    fn name(&self) -> &str {
        "SyntheticOutput"
    }
    fn description(&self) -> &str {
        "Return structured output to the caller. Validates input and passes it through as the tool result."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "data": {
                        "description": "The structured data to return"
                    },
                    "schema_name": {
                        "type": "string",
                        "description": "Name/label for this output schema"
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

        let output = serde_json::json!({
            "structured_output": data,
        });

        Ok(ToolResult::success(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| data.to_string()),
        ))
    }
}
