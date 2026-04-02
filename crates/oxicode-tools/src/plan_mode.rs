//! Plan mode tools: EnterPlanMode and ExitPlanMode.
//!
//! Plan mode restricts the agent to read-only operations (research/planning).
//! The agent must submit a plan for approval before implementation begins.

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Switch the session into plan mode (read-only research + plan creation).
pub struct EnterPlanModeTool;

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "enter_plan_mode"
    }
    fn description(&self) -> &str {
        "Switch to plan mode. In plan mode, only read-only tools are allowed. Submit a plan for approval before implementation."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "Why entering plan mode"
                    }
                }
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("planning phase");

        // State transition handled by the core query engine (reads this result).
        Ok(ToolResult::success(format!(
            "Entered plan mode: {reason}. Only read-only operations allowed until plan is approved."
        )))
    }
}

/// Exit plan mode by submitting a plan for approval.
pub struct ExitPlanModeTool;

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "exit_plan_mode"
    }
    fn description(&self) -> &str {
        "Exit plan mode by submitting a plan. The plan is sent for user approval."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "plan": {
                        "type": "string",
                        "description": "The plan to submit for approval (markdown)"
                    }
                },
                "required": ["plan"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(plan) = input.get("plan").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("plan field is required"));
        };

        // The core engine reads this result to trigger the approval flow.
        Ok(ToolResult::success(format!(
            "Plan submitted for approval ({} chars). Waiting for user response.",
            plan.len()
        )))
    }
}
