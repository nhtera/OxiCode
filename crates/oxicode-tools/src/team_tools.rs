//! Team tools: create and delete agent teams via TeamManager.

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

// ---------------------------------------------------------------------------
// TeamCreate
// ---------------------------------------------------------------------------

pub struct TeamCreateTool;

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "TeamCreate"
    }
    fn description(&self) -> &str {
        "Create a new multi-agent team. Returns team name and confirmation."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "team_name": {
                        "type": "string",
                        "description": "Name for the new team"
                    },
                    "description": {
                        "type": "string",
                        "description": "Team description/purpose"
                    }
                },
                "required": ["team_name"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(name) = input.get("team_name").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("team_name is required"));
        };

        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut mgr = ctx.team_manager.lock().map_err(|e| {
            oxicode_common::OxiError::Other(format!("team manager lock poisoned: {e}"))
        })?;

        if let Err(e) = mgr.create_team(name) {
            return Ok(ToolResult::error(format!("Failed to create team: {e}")));
        }

        let result = serde_json::json!({
            "team_name": name,
            "description": description,
            "status": "created",
        });
        Ok(ToolResult::success(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }
}

// ---------------------------------------------------------------------------
// TeamDelete
// ---------------------------------------------------------------------------

pub struct TeamDeleteTool;

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &str {
        "TeamDelete"
    }
    fn description(&self) -> &str {
        "Delete an existing agent team and all its agents."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "team_name": {
                        "type": "string",
                        "description": "Name of the team to delete"
                    }
                },
                "required": ["team_name"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(name) = input.get("team_name").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("team_name is required"));
        };

        let mut mgr = ctx.team_manager.lock().map_err(|e| {
            oxicode_common::OxiError::Other(format!("team manager lock poisoned: {e}"))
        })?;

        if let Err(e) = mgr.delete_team(name) {
            return Ok(ToolResult::error(format!("Failed to delete team: {e}")));
        }

        let result = serde_json::json!({
            "team_name": name,
            "status": "deleted",
        });
        Ok(ToolResult::success(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_team_create_and_delete() {
        let ctx = ToolContext::default();

        let create = TeamCreateTool;
        let result = create
            .execute(
                serde_json::json!({"team_name": "test-team", "description": "A test team"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("test-team"));

        let delete = TeamDeleteTool;
        let result = delete
            .execute(serde_json::json!({"team_name": "test-team"}), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("deleted"));
    }

    #[tokio::test]
    async fn test_team_create_duplicate() {
        let ctx = ToolContext::default();
        let tool = TeamCreateTool;

        tool.execute(serde_json::json!({"team_name": "dup"}), &ctx)
            .await
            .unwrap();
        let result = tool
            .execute(serde_json::json!({"team_name": "dup"}), &ctx)
            .await
            .unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_team_delete_nonexistent() {
        let ctx = ToolContext::default();
        let tool = TeamDeleteTool;
        let result = tool
            .execute(serde_json::json!({"team_name": "ghost"}), &ctx)
            .await
            .unwrap();
        assert!(result.is_error);
    }
}
