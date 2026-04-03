//! Workflow tool: execute named workflow scripts from `.oxicode/workflows/`.
//!
//! Workflows are shell scripts with defined steps. This tool discovers and
//! executes them, returning their output.

use std::path::PathBuf;

use async_trait::async_trait;
use oxicode_common::OxiResult;
use tokio::process::Command;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Base directory for workflow scripts.
fn workflows_dir(working_dir: &std::path::Path) -> PathBuf {
    working_dir.join(".oxicode").join("workflows")
}

pub struct WorkflowTool;

#[async_trait]
impl Tool for WorkflowTool {
    fn name(&self) -> &str {
        "Workflow"
    }
    fn description(&self) -> &str {
        "Execute a named workflow script from .oxicode/workflows/. Workflows are shell scripts with defined steps."
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
                        "enum": ["list", "run"],
                        "description": "Action: 'list' available workflows or 'run' one"
                    },
                    "name": {
                        "type": "string",
                        "description": "Workflow name (required for 'run')"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments to pass to the workflow script"
                    }
                },
                "required": ["action"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ShellExec
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(action) = input.get("action").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("action is required ('list' or 'run')"));
        };

        let wf_dir = workflows_dir(&ctx.working_dir);

        match action {
            "list" => {
                if !wf_dir.exists() {
                    return Ok(ToolResult::success(serde_json::to_string_pretty(
                        &serde_json::json!({
                            "workflows": [],
                            "message": "No .oxicode/workflows/ directory found"
                        }),
                    )
                    .unwrap_or_default()));
                }

                let mut workflows = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&wf_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                                workflows.push(name.to_string());
                            }
                        }
                    }
                }
                workflows.sort();

                let result = serde_json::json!({
                    "workflows": workflows,
                    "directory": wf_dir.display().to_string(),
                });
                Ok(ToolResult::success(
                    serde_json::to_string_pretty(&result).unwrap_or_default(),
                ))
            }
            "run" => {
                let Some(name) = input.get("name").and_then(|v| v.as_str()) else {
                    return Ok(ToolResult::error("name is required for 'run' action"));
                };

                // Prevent path traversal
                if name.contains("..") || name.contains('/') || name.contains('\\') {
                    return Ok(ToolResult::error("Invalid workflow name"));
                }

                // Try common extensions
                let script_path = find_workflow_script(&wf_dir, name);
                let Some(script) = script_path else {
                    return Ok(ToolResult::error(format!(
                        "Workflow '{name}' not found in {}",
                        wf_dir.display()
                    )));
                };

                let args: Vec<String> = input
                    .get("args")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                let output = Command::new("bash")
                    .arg(&script)
                    .args(&args)
                    .current_dir(&ctx.working_dir)
                    .output()
                    .await;

                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                        let code = out.status.code().unwrap_or(-1);

                        let result = serde_json::json!({
                            "workflow": name,
                            "exit_code": code,
                            "stdout": stdout,
                            "stderr": stderr,
                        });

                        if code == 0 {
                            Ok(ToolResult::success(
                                serde_json::to_string_pretty(&result).unwrap_or_default(),
                            ))
                        } else {
                            Ok(ToolResult::error(
                                serde_json::to_string_pretty(&result).unwrap_or_default(),
                            ))
                        }
                    }
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to execute workflow '{name}': {e}"
                    ))),
                }
            }
            _ => Ok(ToolResult::error(format!(
                "Unknown action: {action}. Use 'list' or 'run'."
            ))),
        }
    }
}

/// Find a workflow script by name, trying common extensions.
fn find_workflow_script(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    let extensions = ["sh", "bash", "zsh", "ps1", ""];
    for ext in &extensions {
        let path = if ext.is_empty() {
            dir.join(name)
        } else {
            dir.join(format!("{name}.{ext}"))
        };
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_workflow_list_no_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let tool = WorkflowTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let result = tool
            .execute(serde_json::json!({"action": "list"}), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("No .oxicode/workflows/"));
    }

    #[tokio::test]
    async fn test_workflow_run_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let tool = WorkflowTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let result = tool
            .execute(
                serde_json::json!({"action": "run", "name": "nonexistent"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_workflow_path_traversal_blocked() {
        let dir = tempfile::TempDir::new().unwrap();
        let tool = WorkflowTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let result = tool
            .execute(
                serde_json::json!({"action": "run", "name": "../etc/passwd"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Invalid"));
    }
}
