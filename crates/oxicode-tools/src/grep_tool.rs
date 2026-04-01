use std::path::Path;

use async_trait::async_trait;
use oxicode_common::OxiResult;
use regex::Regex;
use walkdir::WalkDir;

use crate::path_utils::resolve_path;
use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Search file contents using regex patterns.
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with regex. Returns matching lines with file paths and line numbers."
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
                        "description": "Regex pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory or file to search (default: working dir)"
                    },
                    "glob": {
                        "type": "string",
                        "description": "Glob filter for files (e.g., '*.rs', '*.{ts,tsx}')"
                    },
                    "case_insensitive": {
                        "type": "boolean",
                        "description": "Case insensitive search (default: false)"
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
        let pattern_str =
            input["pattern"]
                .as_str()
                .ok_or_else(|| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: "pattern is required".into(),
                })?;

        let case_insensitive = input["case_insensitive"].as_bool().unwrap_or(false);

        let regex_pattern = if case_insensitive {
            format!("(?i){pattern_str}")
        } else {
            pattern_str.to_string()
        };

        let re = Regex::new(&regex_pattern).map_err(|e| oxicode_common::OxiError::Tool {
            name: self.name().into(),
            message: format!("Invalid regex: {e}"),
        })?;

        let search_path = input["path"].as_str().map_or_else(
            || ctx.working_dir.clone(),
            |p| resolve_path(p, &ctx.working_dir),
        );

        let glob_filter = input["glob"]
            .as_str()
            .map(|g| glob::Pattern::new(g).unwrap_or_else(|_| glob::Pattern::new("*").unwrap()));

        let mut matches = Vec::new();
        let max_matches = 250;

        for entry in WalkDir::new(&search_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || (!is_hidden(e) && !is_ignored_dir(e)))
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();

            // Apply glob filter on the file name.
            if let Some(ref filter) = glob_filter {
                let fname = path.file_name().unwrap_or_default().to_string_lossy();
                if !filter.matches(&fname) {
                    continue;
                }
            }

            // Skip binary files (check first 512 bytes).
            if is_binary(path) {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(path) {
                for (line_num, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        matches.push(format!(
                            "{}:{}:{}",
                            path.display(),
                            line_num + 1,
                            line.trim()
                        ));
                        if matches.len() >= max_matches {
                            matches.push(format!("... truncated at {max_matches} matches"));
                            return Ok(ToolResult::success(matches.join("\n")));
                        }
                    }
                }
            }
        }

        if matches.is_empty() {
            Ok(ToolResult::success("No matches found."))
        } else {
            Ok(ToolResult::success(matches.join("\n")))
        }
    }
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.') && s != ".")
}

fn is_ignored_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    matches!(
        name.as_ref(),
        "node_modules" | "target" | ".git" | "dist" | "build" | "__pycache__" | ".next"
    )
}

/// Check if a file appears to be binary by reading the first 512 bytes.
fn is_binary(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return true;
    };
    let mut buf = [0u8; 512];
    let Ok(n) = f.read(&mut buf) else {
        return true;
    };
    buf[..n].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_grep_finds_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn hello() {}\nfn world() {}").unwrap();
        std::fs::write(dir.path().join("b.txt"), "no match here").unwrap();

        let tool = GrepTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
        };

        let result = tool
            .execute(serde_json::json!({"pattern": "fn \\w+"}), &ctx)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("fn hello"));
        assert!(result.content.contains("fn world"));
    }

    #[tokio::test]
    async fn test_grep_with_glob_filter() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "match_me").unwrap();
        std::fs::write(dir.path().join("b.txt"), "match_me").unwrap();

        let tool = GrepTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
        };

        let result = tool
            .execute(
                serde_json::json!({"pattern": "match_me", "glob": "*.rs"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("a.rs"));
        assert!(!result.content.contains("b.txt"));
    }

    #[tokio::test]
    async fn test_grep_no_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "hello world").unwrap();

        let tool = GrepTool;
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
        };

        let result = tool
            .execute(
                serde_json::json!({"pattern": "nonexistent_pattern_xyz"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("No matches"));
    }
}
