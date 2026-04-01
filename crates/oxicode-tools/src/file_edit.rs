use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::path_utils::{check_path_safety, resolve_path};
use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Edit a file by replacing an exact string match.
pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing an exact string match with new content."
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
                        "description": "Absolute path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact string to find and replace"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement string"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace all occurrences (default: false)",
                        "default": false
                    }
                },
                "required": ["file_path", "old_string", "new_string"]
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

        let old_string =
            input["old_string"]
                .as_str()
                .ok_or_else(|| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: "old_string is required".into(),
                })?;

        let new_string =
            input["new_string"]
                .as_str()
                .ok_or_else(|| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: "new_string is required".into(),
                })?;

        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

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

        let content =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: format!("Failed to read {}: {e}", path.display()),
                })?;

        let count = content.matches(old_string).count();

        if count == 0 {
            return Ok(ToolResult::error(format!(
                "old_string not found in {}",
                path.display()
            )));
        }

        if !replace_all && count > 1 {
            return Ok(ToolResult::error(format!(
                "old_string matches {count} times in {}. Use replace_all or provide more context.",
                path.display()
            )));
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        tokio::fs::write(&path, &new_content).await.map_err(|e| {
            oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Failed to write {}: {e}", path.display()),
            }
        })?;

        Ok(ToolResult::success(format!(
            "Replaced {count} occurrence(s) in {}",
            path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_edit_single_match() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "hello world").unwrap();

        let tool = FileEditTool;
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({
                    "file_path": f.path().to_str().unwrap(),
                    "old_string": "hello",
                    "new_string": "goodbye"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(f.path()).unwrap(), "goodbye world");
    }

    #[tokio::test]
    async fn test_edit_not_unique() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "aaa bbb aaa").unwrap();

        let tool = FileEditTool;
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({
                    "file_path": f.path().to_str().unwrap(),
                    "old_string": "aaa",
                    "new_string": "ccc"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("matches 2 times"));
    }

    #[tokio::test]
    async fn test_edit_replace_all() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "aaa bbb aaa").unwrap();

        let tool = FileEditTool;
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({
                    "file_path": f.path().to_str().unwrap(),
                    "old_string": "aaa",
                    "new_string": "ccc",
                    "replace_all": true
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(f.path()).unwrap(), "ccc bbb ccc");
    }

    #[tokio::test]
    async fn test_edit_not_found() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "hello world").unwrap();

        let tool = FileEditTool;
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({
                    "file_path": f.path().to_str().unwrap(),
                    "old_string": "missing",
                    "new_string": "x"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }
}
