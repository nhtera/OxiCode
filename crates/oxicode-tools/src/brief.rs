//! Brief tool: generate an AI summary of the current conversation or content.
//!
//! Returns a structured summary request that the core engine handles
//! by making a separate LLM call with summarization instructions.

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

pub struct BriefTool;

#[async_trait]
impl Tool for BriefTool {
    fn name(&self) -> &str {
        "brief"
    }
    fn description(&self) -> &str {
        "Generate a brief AI summary of content or the current conversation."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "Content to summarize (if empty, summarizes the conversation)"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Maximum summary length in words (default: 200)"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["paragraph", "bullets", "tldr"],
                        "description": "Summary format (default: bullets)"
                    }
                }
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let max_length = input
            .get("max_length")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(200);
        let format = input
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("bullets");

        // The core engine intercepts this result and makes a summarization LLM call.
        // We return the request parameters for the engine to process.
        let summary_request = serde_json::json!({
            "action": "summarize",
            "content": content,
            "max_length": max_length,
            "format": format,
        });

        Ok(ToolResult::success(
            serde_json::to_string_pretty(&summary_request)
                .unwrap_or_else(|_| "Summary request created".to_string()),
        ))
    }
}
