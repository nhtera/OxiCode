//! Tool use summary — generates token-efficient summaries of tool calls per turn.
//!
//! After each turn, summarize tool calls for compact display in message history.

use std::fmt::Write as _;

use oxicode_common::ContentBlock;

/// Summary of tool calls made during a single turn.
#[derive(Debug, Clone)]
pub struct ToolUseSummary {
    /// Number of tool calls in the turn.
    pub tool_count: usize,
    /// Brief descriptions of each tool call.
    pub entries: Vec<ToolSummaryEntry>,
}

/// A single tool call summary entry.
#[derive(Debug, Clone)]
pub struct ToolSummaryEntry {
    pub tool_name: String,
    /// Compact description of what the tool did.
    pub description: String,
    /// Whether the tool call resulted in an error.
    pub is_error: bool,
}

impl ToolUseSummary {
    /// Build a summary from a list of content blocks (from an assistant message).
    pub fn from_content_blocks(blocks: &[ContentBlock]) -> Self {
        let mut entries = Vec::new();

        // Pair ToolUse with their ToolResult.
        let tool_uses: Vec<_> = blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
                _ => None,
            })
            .collect();

        for (id, name, input) in &tool_uses {
            let description = summarize_tool_input(name, input);
            let is_error = blocks.iter().any(|b| {
                matches!(b, ContentBlock::ToolResult { tool_use_id, is_error, .. }
                    if tool_use_id == *id && *is_error)
            });

            entries.push(ToolSummaryEntry {
                tool_name: (*name).clone(),
                description,
                is_error,
            });
        }

        Self {
            tool_count: entries.len(),
            entries,
        }
    }

    /// Format as a compact multi-line string for display.
    pub fn display(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }

        let mut lines = vec![format!("Tool calls ({}):", self.tool_count)];
        for entry in &self.entries {
            let icon = if entry.is_error { "✗" } else { "✓" };
            lines.push(format!("  {icon} {} — {}", entry.tool_name, entry.description));
        }
        lines.join("\n")
    }

    /// Format as a single-line compact summary for space-constrained views.
    pub fn display_compact(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }

        let names: Vec<&str> = self.entries.iter().map(|e| e.tool_name.as_str()).collect();
        let errors = self.entries.iter().filter(|e| e.is_error).count();

        let mut s = format!("[{} tools: {}]", self.tool_count, names.join(", "));
        if errors > 0 {
            write!(s, " ({errors} failed)").ok();
        }
        s
    }
}

/// Generate a brief description from tool name and input.
fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "file_read" | "Read" => {
            let path = input
                .get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("read {path}")
        }
        "file_write" | "Write" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("write {path}")
        }
        "file_edit" | "Edit" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("edit {path}")
        }
        "bash" | "Bash" => {
            let cmd = input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let preview: String = cmd.chars().take(60).collect();
            format!("$ {preview}")
        }
        "grep" | "Grep" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("grep \"{pattern}\"")
        }
        "glob" | "Glob" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("glob \"{pattern}\"")
        }
        "web_search" | "WebSearch" => {
            let query = input
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("search \"{query}\"")
        }
        _ => {
            // Generic: show first key=value pair.
            if let Some(obj) = input.as_object() {
                if let Some((k, v)) = obj.iter().next() {
                    let val_str = match v {
                        serde_json::Value::String(s) => {
                            let preview: String = s.chars().take(40).collect();
                            preview
                        }
                        other => {
                            let s = other.to_string();
                            let preview: String = s.chars().take(40).collect();
                            preview
                        }
                    };
                    return format!("{k}={val_str}");
                }
            }
            "called".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_from_blocks() {
        let blocks = vec![
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "file_read".to_string(),
                input: serde_json::json!({"file_path": "/tmp/test.rs"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: "file contents".to_string(),
                is_error: false,
            },
            ContentBlock::ToolUse {
                id: "t2".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "cargo test"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "t2".to_string(),
                content: "all tests passed".to_string(),
                is_error: false,
            },
        ];

        let summary = ToolUseSummary::from_content_blocks(&blocks);
        assert_eq!(summary.tool_count, 2);
        assert_eq!(summary.entries[0].tool_name, "file_read");
        assert!(summary.entries[0].description.contains("/tmp/test.rs"));
        assert_eq!(summary.entries[1].tool_name, "bash");
        assert!(summary.entries[1].description.contains("cargo test"));
    }

    #[test]
    fn test_summary_with_error() {
        let blocks = vec![
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "exit 1"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: "error".to_string(),
                is_error: true,
            },
        ];

        let summary = ToolUseSummary::from_content_blocks(&blocks);
        assert!(summary.entries[0].is_error);
        assert!(summary.display().contains('✗'));
    }

    #[test]
    fn test_display_compact() {
        let summary = ToolUseSummary {
            tool_count: 2,
            entries: vec![
                ToolSummaryEntry {
                    tool_name: "file_read".to_string(),
                    description: "read file".to_string(),
                    is_error: false,
                },
                ToolSummaryEntry {
                    tool_name: "bash".to_string(),
                    description: "$ cargo test".to_string(),
                    is_error: false,
                },
            ],
        };
        let compact = summary.display_compact();
        assert!(compact.contains("[2 tools:"));
    }

    #[test]
    fn test_empty_summary() {
        let summary = ToolUseSummary::from_content_blocks(&[]);
        assert_eq!(summary.tool_count, 0);
        assert!(summary.display().is_empty());
        assert!(summary.display_compact().is_empty());
    }
}
