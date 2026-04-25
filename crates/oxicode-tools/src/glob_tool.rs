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

        // When an explicit path is given, search only that path (original behaviour).
        // When no path is given, search working_dir AND all extra_working_dirs.
        let explicit_path = input["path"].as_str();

        let search_roots: Vec<std::path::PathBuf> = if let Some(p) = explicit_path {
            vec![resolve_path(p, &ctx.working_dir)]
        } else {
            let mut roots = vec![ctx.working_dir.clone()];
            for extra in &ctx.extra_working_dirs {
                if !roots.contains(extra) {
                    roots.push(extra.clone());
                    tracing::info!("glob: searching extra working dir {}", extra.display());
                }
            }
            roots
        };

        let mut files: Vec<(String, SystemTime)> = Vec::new();
        for base_dir in &search_roots {
            let full_pattern = base_dir.join(pattern).to_string_lossy().to_string();
            let entries =
                glob::glob(&full_pattern).map_err(|e| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: format!("Invalid glob pattern: {e}"),
                })?;
            for entry in entries.flatten() {
                let mtime = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                let path_str = entry.to_string_lossy().to_string();
                // Deduplicate (same file reachable via two roots is unlikely but possible).
                if !files.iter().any(|(p, _)| p == &path_str) {
                    files.push((path_str, mtime));
                }
            }
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
            ..Default::default()
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
            ..Default::default()
        };

        let result = tool
            .execute(serde_json::json!({"pattern": "*.xyz"}), &ctx)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("No files matched"));
    }

    #[tokio::test]
    async fn test_glob_across_extra_working_dirs() {
        // Two separate temp dirs, each with a unique .rs file.
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        std::fs::write(dir1.path().join("root1_file.rs"), "fn a() {}").unwrap();
        std::fs::write(dir2.path().join("root2_file.rs"), "fn b() {}").unwrap();

        let ctx = ToolContext {
            working_dir: dir1.path().to_path_buf(),
            extra_working_dirs: vec![dir2.path().to_path_buf()],
            ..Default::default()
        };

        let result = GlobTool
            .execute(serde_json::json!({"pattern": "*.rs"}), &ctx)
            .await
            .unwrap();

        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert!(
            result.content.contains("root1_file.rs"),
            "should find root1_file.rs; got: {}",
            result.content
        );
        assert!(
            result.content.contains("root2_file.rs"),
            "should find root2_file.rs; got: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_glob_explicit_path_ignores_extras() {
        // When an explicit `path` param is given, extra_working_dirs must NOT be consulted.
        let primary = TempDir::new().unwrap();
        let extra = TempDir::new().unwrap();
        std::fs::write(primary.path().join("primary.rs"), "").unwrap();
        std::fs::write(extra.path().join("extra.rs"), "").unwrap();

        let ctx = ToolContext {
            working_dir: primary.path().to_path_buf(),
            extra_working_dirs: vec![extra.path().to_path_buf()],
            ..Default::default()
        };

        let result = GlobTool
            .execute(
                serde_json::json!({"pattern": "*.rs", "path": primary.path().to_str().unwrap()}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("primary.rs"));
        assert!(!result.content.contains("extra.rs"));
    }
}
