use std::time::SystemTime;

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::path_utils::resolve_path;
use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Find files matching a glob pattern, sorted by modification time.
pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Returns paths sorted by modification time."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern (e.g., '**/*.rs', 'src/**/*.ts')"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default: working directory)"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: "pattern is required".into(),
            })?;

        let base_dir = input["path"].as_str().map_or_else(
            || ctx.working_dir.clone(),
            |p| resolve_path(p, &ctx.working_dir),
        );

        let full_pattern = base_dir.join(pattern).to_string_lossy().to_string();

        let entries = glob::glob(&full_pattern).map_err(|e| oxicode_common::OxiError::Tool {
            name: self.name().into(),
            message: format!("Invalid glob pattern: {e}"),
        })?;

        let mut files: Vec<(String, SystemTime)> = Vec::new();
        for entry in entries.flatten() {
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((entry.to_string_lossy().to_string(), mtime));
        }

        // Sort by modification time, most recent first.
        files.sort_by(|a, b| b.1.cmp(&a.1));

        let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        let count = paths.len();

        if paths.is_empty() {
            return Ok(ToolResult::success("No files matched the pattern."));
        }

        // Limit output to 500 entries.
        let display: Vec<&str> = paths.iter().take(500).copied().collect();
        let mut output = display.join("\n");
        if count > 500 {
            use std::fmt::Write;
            let _ = write!(output, "\n... and {} more files", count - 500);
        }

        Ok(ToolResult::success(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_glob_finds_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        std::fs::write(dir.path().join("c.rs"), "c").unwrap();

        let tool = GlobTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
        };

        let result = tool
            .execute(serde_json::json!({"pattern": "*.txt"}), &ctx)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("a.txt"));
        assert!(result.content.contains("b.txt"));
        assert!(!result.content.contains("c.rs"));
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let dir = TempDir::new().unwrap();

        let tool = GlobTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
        };

        let result = tool
            .execute(serde_json::json!({"pattern": "*.xyz"}), &ctx)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("No files matched"));
    }
}
