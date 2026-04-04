//! SuggestBackgroundPR tool: create a draft PR from current changes.
//!
//! Shells out to `gh pr create --draft` to suggest a background pull request
//! for the current branch's changes.

use async_trait::async_trait;
use oxicode_common::OxiResult;
use tokio::process::Command;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

pub struct SuggestBackgroundPrTool;

#[async_trait]
impl Tool for SuggestBackgroundPrTool {
    fn name(&self) -> &str {
        "SuggestBackgroundPR"
    }
    fn description(&self) -> &str {
        "Create a draft pull request from the current branch's changes using GitHub CLI."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "PR title"
                    },
                    "body": {
                        "type": "string",
                        "description": "PR body/description"
                    },
                    "base": {
                        "type": "string",
                        "description": "Base branch (default: main)"
                    }
                },
                "required": ["title"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(title) = input.get("title").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("title is required"));
        };

        let body = input
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("Draft PR created by OxiCode");
        let base = input.get("base").and_then(|v| v.as_str()).unwrap_or("main");

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            Command::new("gh")
                .args([
                    "pr", "create", "--draft", "--title", title, "--body", body, "--base", base,
                ])
                .current_dir(&ctx.working_dir)
                .kill_on_drop(true)
                .output(),
        )
        .await;

        match output {
            Ok(Ok(out)) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();

                if out.status.success() {
                    let result = serde_json::json!({
                        "status": "created",
                        "pr_url": stdout.trim(),
                        "title": title,
                    });
                    Ok(ToolResult::success(
                        serde_json::to_string_pretty(&result).unwrap_or_default(),
                    ))
                } else {
                    Ok(ToolResult::error(format!(
                        "gh pr create failed (exit {}):\n{stderr}",
                        out.status.code().unwrap_or(-1)
                    )))
                }
            }
            Ok(Err(e)) => Ok(ToolResult::error(format!(
                "Failed to run gh CLI: {e}. Is GitHub CLI installed?"
            ))),
            Err(_) => Ok(ToolResult::error("gh pr create timed out after 60s")),
        }
    }
}
