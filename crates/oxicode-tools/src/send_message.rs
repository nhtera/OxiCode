use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Send a notification message to the user via TUI.
pub struct SendMessageTool;

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a notification message to the user."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message to send"
                    }
                },
                "required": ["message"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let message = input["message"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: "message is required".into(),
            })?;

        Ok(ToolResult::success(format!("[NOTIFICATION] {message}")))
    }
}
