use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Read or write configuration values.
pub struct ConfigTool;

#[async_trait]
impl Tool for ConfigTool {
    fn name(&self) -> &str {
        "config"
    }

    fn description(&self) -> &str {
        "Read or write OxiCode configuration values."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["get", "set", "list"],
                        "description": "Config action"
                    },
                    "key": {
                        "type": "string",
                        "description": "Config key (for get/set)"
                    },
                    "value": {
                        "type": "string",
                        "description": "Config value (for set)"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let action = input["action"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: "action is required".into(),
            })?;

        match action {
            "list" => Ok(ToolResult::success(
                "Available config keys: model, max_tokens, theme, permission_mode",
            )),
            "get" => {
                let key = input["key"].as_str().unwrap_or("unknown");
                Ok(ToolResult::success(format!(
                    "[CONFIG_GET] key={key} — config integration pending"
                )))
            }
            "set" => {
                let key = input["key"].as_str().unwrap_or("unknown");
                let value = input["value"].as_str().unwrap_or("");
                Ok(ToolResult::success(format!(
                    "[CONFIG_SET] {key}={value} — config integration pending"
                )))
            }
            other => Ok(ToolResult::error(format!("Unknown action: {other}"))),
        }
    }
}
