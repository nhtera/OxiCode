use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Prompt the user with a question via the TUI.
pub struct AskUserTool;

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user a question and wait for their response."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question to ask the user"
                    }
                },
                "required": ["question"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let question =
            input["question"]
                .as_str()
                .ok_or_else(|| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: "question is required".into(),
                })?;

        // In the real implementation, this sends the question to the TUI
        // and waits for user input via a channel. For now, return the question
        // as a marker that the TUI layer should handle.
        Ok(ToolResult::success(format!("[ASK_USER] {question}")))
    }
}
