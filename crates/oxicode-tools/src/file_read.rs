use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::path_utils::{check_path_safety, resolve_path};
use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Read a file from the filesystem with optional line range.
pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read a file from the local filesystem. Returns content with line numbers."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Absolute path to the file to read"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Line number to start reading from (0-based)",
                        "minimum": 0
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read",
                        "minimum": 1
                    }
                },
                "required": ["file_path"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let file_path =
            input["file_path"]
                .as_str()
                .ok_or_else(|| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: "file_path is required".into(),
                })?;

        let path = resolve_path(file_path, &ctx.working_dir);

        if let Some(err) = check_path_safety(&path) {
            return Ok(err);
        }

        if !path.exists() {
            return Ok(ToolResult::error(format!(
                "File does not exist: {}",
                path.display()
            )));
        }

        if !path.is_file() {
            return Ok(ToolResult::error(format!("Not a file: {}", path.display())));
        }

        let content =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: format!("Failed to read {}: {e}", path.display()),
                })?;

        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let limit = input["limit"].as_u64().unwrap_or(2000) as usize;

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        let start = offset.min(total);
        let end = (start + limit).min(total);

        let numbered: String = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{}\t{}", start + i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        // Record mtime so FileEditTool/FileWriteTool can detect external changes
        ctx.file_state.record(&path);

        Ok(ToolResult::success(numbered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_read_file() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "line1\nline2\nline3").unwrap();

        let tool = FileReadTool;
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({"file_path": f.path().to_str().unwrap()}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("1\tline1"));
    }

    #[tokio::test]
    async fn test_read_with_offset_limit() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "a\nb\nc\nd\ne").unwrap();

        let tool = FileReadTool;
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({"file_path": f.path().to_str().unwrap(), "offset": 1, "limit": 2}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("2\tb"));
        assert!(result.content.contains("3\tc"));
        assert!(!result.content.contains("4\td"));
    }

    #[tokio::test]
    async fn test_read_nonexistent() {
        let tool = FileReadTool;
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({"file_path": "/nonexistent/file.txt"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("does not exist"));
    }
}
