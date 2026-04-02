use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::path_utils::{check_path_safety, check_workspace_boundary, resolve_path};
use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Write content to a file (create or overwrite).
pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed."
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
                        "description": "Absolute path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["file_path", "content"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::FileWrite
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
        if let Some(err) = check_workspace_boundary(&path, &ctx.working_dir) {
            return Ok(err);
        }

        let content = input["content"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: "content is required".into(),
            })?;

        // Staleness check for existing files: reject if modified since last read
        if path.exists() {
            if let Err(msg) = ctx.file_state.check_staleness(&path) {
                return Ok(ToolResult::error(msg));
            }
        }

        // Create parent directories
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: format!("Failed to create directories: {e}"),
                }
            })?;
        }

        // Atomic write via temp file + rename to prevent partial writes on crash.
        // Append suffix to full filename (not with_extension, which replaces it).
        let tmp_path = path.with_file_name(format!(
            "{}.oxicode-tmp",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        tokio::fs::write(&tmp_path, content)
            .await
            .map_err(|e| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Failed to write temp file: {e}"),
            })?;
        // Preserve original file permissions (e.g., execute bits on scripts).
        if path.exists() {
            if let Ok(meta) = std::fs::metadata(&path) {
                let _ = std::fs::set_permissions(&tmp_path, meta.permissions());
            }
        }
        tokio::fs::rename(&tmp_path, &path).await.map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Failed to write {}: {e}", path.display()),
            }
        })?;

        // Record mtime after successful write
        ctx.file_state.record(&path);

        Ok(ToolResult::success(format!(
            "Successfully wrote {} bytes to {}",
            content.len(),
            path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_new_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");

        let tool = FileWriteTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };

        let result = tool
            .execute(
                serde_json::json!({"file_path": file.to_str().unwrap(), "content": "hello world"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello world");
    }

    #[tokio::test]
    async fn test_write_creates_parents() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("sub/dir/test.txt");

        let tool = FileWriteTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };

        let result = tool
            .execute(
                serde_json::json!({"file_path": file.to_str().unwrap(), "content": "nested"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "nested");
    }
}
