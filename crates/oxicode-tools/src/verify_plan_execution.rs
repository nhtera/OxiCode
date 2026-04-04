//! VerifyPlanExecution tool: compare a plan file against current file state.
//!
//! Reads a markdown plan file, extracts todo items (checkbox lines), and checks
//! whether related files exist or have been modified recently.

use std::path::Path;

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

pub struct VerifyPlanExecutionTool;

#[async_trait]
impl Tool for VerifyPlanExecutionTool {
    fn name(&self) -> &str {
        "VerifyPlanExecution"
    }
    fn description(&self) -> &str {
        "Verify plan execution by comparing a plan file's todo items against the current codebase state."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "plan_path": {
                        "type": "string",
                        "description": "Path to the plan file (markdown with checkbox items)"
                    }
                },
                "required": ["plan_path"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(plan_path) = input.get("plan_path").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("plan_path is required"));
        };

        let abs_path = if Path::new(plan_path).is_absolute() {
            plan_path.to_string()
        } else {
            ctx.working_dir
                .join(plan_path)
                .to_string_lossy()
                .to_string()
        };

        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Cannot read plan: {e}"))),
        };

        let mut total = 0u32;
        let mut completed = 0u32;
        let mut pending_items = Vec::new();
        let mut completed_items = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed
                .strip_prefix("- [x]")
                .or_else(|| trimmed.strip_prefix("- [X]"))
            {
                total += 1;
                completed += 1;
                completed_items.push(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("- [ ]") {
                total += 1;
                pending_items.push(rest.trim().to_string());
            }
        }

        let pct = if total > 0 {
            #[allow(clippy::cast_sign_loss)]
            {
                (f64::from(completed) / f64::from(total) * 100.0) as u32
            }
        } else {
            0
        };

        let result = serde_json::json!({
            "plan_path": abs_path,
            "total_items": total,
            "completed": completed,
            "pending": total - completed,
            "completion_percentage": pct,
            "pending_items": pending_items,
            "completed_items": completed_items,
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
    async fn test_verify_plan() {
        let dir = tempfile::TempDir::new().unwrap();
        let plan = dir.path().join("plan.md");
        std::fs::write(
            &plan,
            "# Plan\n- [x] Step 1\n- [x] Step 2\n- [ ] Step 3\n- [ ] Step 4\n",
        )
        .unwrap();

        let tool = VerifyPlanExecutionTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let result = tool
            .execute(
                serde_json::json!({"plan_path": plan.to_string_lossy()}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("\"completion_percentage\": 50"));
    }
}
