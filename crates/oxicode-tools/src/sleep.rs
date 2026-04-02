//! Sleep tool: async delay for a specified number of seconds.

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Maximum sleep duration in seconds.
const MAX_SLEEP_SECS: u64 = 300;

pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "sleep"
    }
    fn description(&self) -> &str {
        "Wait for a specified number of seconds."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "seconds": {
                        "type": "integer",
                        "description": "Number of seconds to sleep (max: 300)"
                    }
                },
                "required": ["seconds"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let secs = match input.get("seconds").and_then(|v| v.as_u64()) {
            Some(s) => s.min(MAX_SLEEP_SECS),
            None => return Ok(ToolResult::error("seconds is required (integer)")),
        };

        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;

        Ok(ToolResult::success(format!("Slept for {secs} seconds")))
    }
}
