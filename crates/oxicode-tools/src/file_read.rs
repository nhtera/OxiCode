use async_trait::async_trait;
use oxicode_common::OxiResult;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::path_utils::{check_path_safety, check_workspace_boundary, resolve_path};
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

        // Resolve against primary working_dir first.
        let primary_path = resolve_path(file_path, &ctx.working_dir);

        // Determine the effective path: primary if it passes all checks and exists,
        // otherwise try each extra_working_dir for relative paths.
        let path = if primary_path.exists()
            && check_path_safety(&primary_path).is_none()
            && check_workspace_boundary(&primary_path, &ctx.working_dir).is_none()
        {
            primary_path
        } else if !std::path::Path::new(file_path).is_absolute()
            && !ctx.extra_working_dirs.is_empty()
        {
            // Relative path that doesn't exist/resolve under the primary working_dir —
            // try each extra working directory in insertion order.
            let mut found: Option<std::path::PathBuf> = None;
            for extra in &ctx.extra_working_dirs {
                let candidate = resolve_path(file_path, extra);
                if candidate.exists()
                    && check_path_safety(&candidate).is_none()
                    && check_workspace_boundary(&candidate, extra).is_none()
                {
                    tracing::info!(
                        "file_read: resolved '{}' via extra working dir {}",
                        file_path,
                        extra.display()
                    );
                    found = Some(candidate);
                    break;
                }
            }
            found.unwrap_or(primary_path)
        } else {
            primary_path
        };

        if let Some(err) = check_path_safety(&path) {
            return Ok(err);
        }
        // Accept the path if it falls under the primary working_dir OR any extra.
        let in_primary = check_workspace_boundary(&path, &ctx.working_dir).is_none();
        let in_extras = ctx
            .extra_working_dirs
            .iter()
            .any(|extra| check_workspace_boundary(&path, extra).is_none());
        if !in_primary && !in_extras {
            return Ok(ToolResult::error(format!(
                "Access denied: path {} is outside the working directory",
                path.display()
            )));
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

        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let limit = input["limit"].as_u64().unwrap_or(2000) as usize;

        // Stream line-by-line to avoid loading entire file into memory (OOM risk
        // for large files like logs). Only collect lines in [offset, offset+limit).
        let file =
            tokio::fs::File::open(&path)
                .await
                .map_err(|e| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: format!("Failed to open {}: {e}", path.display()),
                })?;

        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut line_num = 0usize;
        let mut output_lines = Vec::with_capacity(limit.min(2000));

        while let Some(line) =
            lines
                .next_line()
                .await
                .map_err(|e| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: format!("Failed to read {}: {e}", path.display()),
                })?
        {
            if line_num >= offset && output_lines.len() < limit {
                output_lines.push(format!("{}\t{}", line_num + 1, line));
            }
            line_num += 1;
            // Stop early once we have enough lines past the offset range
            if output_lines.len() >= limit && line_num > offset + limit {
                break;
            }
        }

        let numbered = output_lines.join("\n");

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
        let dir = tempfile::TempDir::new().unwrap();
        let tool = FileReadTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let missing = dir.path().join("nonexistent.txt");
        let result = tool
            .execute(
                serde_json::json!({"file_path": missing.to_str().unwrap()}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("does not exist"));
    }

    #[tokio::test]
    async fn test_read_outside_workspace_blocked() {
        let tool = FileReadTool;
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let result = tool
            .execute(
                serde_json::json!({"file_path": "/nonexistent/file.txt"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("Access denied"));
    }
}
