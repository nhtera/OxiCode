//! Grep tool: search file contents using ripgrep (rg) subprocess.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use oxicode_common::OxiResult;
use tokio::process::Command;

use crate::path_utils::resolve_path;
use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

const RG_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_OUTPUT_CHARS: usize = 20_000;

/// Search file contents using ripgrep.
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with regex via ripgrep. Supports output modes, context lines, \
         multiline, type filter, and pagination."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "File or directory to search (default: working dir)" },
                    "glob": { "type": "string", "description": "Glob filter (e.g. \"*.rs\", \"*.{ts,tsx}\") — maps to rg --glob" },
                    "output_mode": { "type": "string", "enum": ["content", "files_with_matches", "count"], "description": "Output mode (default: files_with_matches)" },
                    "-B": { "type": "number", "description": "Lines before match (content mode only)" },
                    "-A": { "type": "number", "description": "Lines after match (content mode only)" },
                    "-C": { "type": "number", "description": "Alias for context" },
                    "context": { "type": "number", "description": "Lines before+after match (content mode only)" },
                    "-n": { "type": "boolean", "description": "Show line numbers (default: true in content mode)" },
                    "-i": { "type": "boolean", "description": "Case insensitive search" },
                    "type": { "type": "string", "description": "File type filter (e.g. \"rust\", \"js\", \"py\") — maps to rg --type" },
                    "head_limit": { "type": "number", "description": "Limit output entries (default: 250, 0=unlimited)" },
                    "offset": { "type": "number", "description": "Skip first N entries before head_limit (default: 0)" },
                    "multiline": { "type": "boolean", "description": "Enable multiline matching (rg -U --multiline-dotall)" }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let pattern = input["pattern"].as_str().ok_or_else(|| oxicode_common::OxiError::Tool {
            name: self.name().into(),
            message: "pattern is required".into(),
        })?;

        let search_path = input["path"]
            .as_str()
            .map_or_else(|| ctx.working_dir.clone(), |p| resolve_path(p, &ctx.working_dir));
        let output_mode = input["output_mode"].as_str().unwrap_or("files_with_matches");

        let args = build_rg_args(pattern, &input, output_mode, &search_path);
        let (stdout, stderr, code) = match run_rg(&args, &ctx.working_dir).await {
            Ok(result) => result,
            Err(msg) => return Ok(ToolResult::error(msg)),
        };

        match code {
            0 => Ok(format_output(&stdout, output_mode, &input, &ctx.working_dir)),
            1 => Ok(ToolResult::success("No matches found.")),
            _ => Ok(ToolResult::error(if stderr.is_empty() { stdout } else { stderr })),
        }
    }
}

/// Build the rg argument list from tool input parameters.
fn build_rg_args(pattern: &str, input: &serde_json::Value, mode: &str, search_path: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--hidden".into(), "--no-heading".into(), "--max-columns".into(), "500".into(),
        "--glob".into(), "!.git".into(), "--glob".into(), "!.svn".into(),
        "--glob".into(), "!.hg".into(), "--glob".into(), "!.bzr".into(),
        "--glob".into(), "!.jj".into(), "--glob".into(), "!.sl".into(),
    ];

    match mode {
        "files_with_matches" => args.push("-l".into()),
        "count" => args.push("-c".into()),
        _ => {} // content mode: no extra flag
    }

    if mode == "content" {
        if input["-n"].as_bool().unwrap_or(true) {
            args.push("-n".into());
        }
        // Context lines: context > -C > -B/-A (clamped to 500 max)
        let clamp = |v: u64| v.min(500);
        let ctx = input["context"].as_u64().or_else(|| input["-C"].as_u64());
        if let Some(c) = ctx {
            args.extend(["-C".into(), clamp(c).to_string()]);
        } else {
            if let Some(b) = input["-B"].as_u64() { args.extend(["-B".into(), clamp(b).to_string()]); }
            if let Some(a) = input["-A"].as_u64() { args.extend(["-A".into(), clamp(a).to_string()]); }
        }
    }

    if input["-i"].as_bool().unwrap_or(false) { args.push("-i".into()); }
    if input["multiline"].as_bool().unwrap_or(false) {
        args.extend(["-U".into(), "--multiline-dotall".into()]);
    }
    if let Some(g) = input["glob"].as_str() { args.extend(["--glob".into(), g.into()]); }
    if let Some(t) = input["type"].as_str() { args.extend(["--type".into(), t.into()]); }

    // Pattern via -e to protect against leading dashes
    args.extend(["-e".into(), pattern.into()]);
    args.push(search_path.to_string_lossy().into_owned());
    args
}

/// Run rg subprocess with timeout. Returns (stdout, stderr, exit_code).
async fn run_rg(args: &[String], working_dir: &Path) -> Result<(String, String, i32), String> {
    let child = Command::new("rg")
        .args(args)
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "ripgrep (rg) not found. Install: https://github.com/BurntSushi/ripgrep".to_string())?;

    let output = tokio::time::timeout(RG_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "Search timed out after 20s".to_string())?
        .map_err(|e| format!("rg process error: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let code = output.status.code().unwrap_or(2);
    Ok((stdout, stderr, code))
}

/// Format rg output based on mode, applying pagination and char cap.
fn format_output(
    stdout: &str, mode: &str, input: &serde_json::Value, working_dir: &Path,
) -> ToolResult {
    let head_limit = input["head_limit"].as_u64().unwrap_or(250) as usize;
    let offset = input["offset"].as_u64().unwrap_or(0) as usize;

    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

    let result = if mode == "files_with_matches" {
        let mut entries: Vec<_> = lines.iter().map(|l| {
            let mtime = std::fs::metadata(l).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (*l, mtime)
        }).collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1)); // newest first
        let paths: Vec<String> = entries.iter()
            .map(|(p, _)| relativize(p, working_dir))
            .collect();
        let path_refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let paginated = paginate(&path_refs, offset, head_limit);
        if paginated.is_empty() { return ToolResult::success("No matches found."); }
        format!("Found {} files\n{}", paginated.len(), paginated.join("\n"))
    } else if mode == "count" {
        let paginated = paginate(&lines, offset, head_limit);
        let total: u64 = paginated.iter()
            .filter_map(|l| l.rsplit_once(':').and_then(|(_, c)| c.parse::<u64>().ok()))
            .sum();
        let rel: Vec<String> = paginated.iter().map(|l| relativize_line(l, working_dir)).collect();
        format!("{}\n\nTotal: {} matches in {} files", rel.join("\n"), total, rel.len())
    } else {
        // content mode
        let paginated = paginate(&lines, offset, head_limit);
        let rel: Vec<String> = paginated.iter().map(|l| relativize_line(l, working_dir)).collect();
        rel.join("\n")
    };

    ToolResult::success(cap_output(result))
}

/// Skip offset, then take limit (0=unlimited).
fn paginate<'a>(lines: &[&'a str], offset: usize, limit: usize) -> Vec<&'a str> {
    let skipped = if offset < lines.len() { &lines[offset..] } else { &[] };
    if limit == 0 { skipped.to_vec() } else { skipped.iter().take(limit).copied().collect() }
}

/// Truncate to MAX_OUTPUT_CHARS (safe for multi-byte UTF-8).
fn cap_output(s: String) -> String {
    if s.len() <= MAX_OUTPUT_CHARS { return s; }
    // Walk back to a valid char boundary to avoid panic on multi-byte chars
    let mut end = MAX_OUTPUT_CHARS;
    while !s.is_char_boundary(end) { end -= 1; }
    format!("{}... truncated at {} chars", &s[..end], MAX_OUTPUT_CHARS)
}

/// Make a path relative to working_dir.
fn relativize(path: &str, working_dir: &Path) -> String {
    Path::new(path).strip_prefix(working_dir)
        .map_or_else(|_| path.to_string(), |r| r.display().to_string())
}

/// Relativize the path portion of a `path:line:content` or `path:count` line.
fn relativize_line(line: &str, working_dir: &Path) -> String {
    // Find first colon after the path portion. Paths may contain `:` on Windows,
    // but rg output on Unix is always `path:line:content` or `path:count`.
    if let Some(idx) = line.find(':') {
        let (path, rest) = line.split_at(idx);
        format!("{}{rest}", relativize(path, working_dir))
    } else {
        relativize(line, working_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx(dir: &TempDir) -> ToolContext {
        ToolContext { working_dir: dir.path().to_path_buf(), ..Default::default() }
    }

    #[tokio::test]
    async fn test_grep_finds_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn hello() {}\nfn world() {}").unwrap();
        std::fs::write(dir.path().join("b.txt"), "no match here").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "fn \\w+", "output_mode": "content"}), &ctx(&dir))
            .await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("fn hello"));
        assert!(result.content.contains("fn world"));
    }

    #[tokio::test]
    async fn test_grep_with_glob_filter() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "match_me").unwrap();
        std::fs::write(dir.path().join("b.txt"), "match_me").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "match_me", "glob": "*.rs"}), &ctx(&dir))
            .await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("a.rs"));
        assert!(!result.content.contains("b.txt"));
    }

    #[tokio::test]
    async fn test_grep_no_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "hello world").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "nonexistent_pattern_xyz"}), &ctx(&dir))
            .await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("No matches"));
    }

    #[tokio::test]
    async fn test_grep_output_mode_content() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.rs"), "line1\nmatch_here\nline3").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "match_here", "output_mode": "content"}), &ctx(&dir))
            .await.unwrap();

        assert!(result.content.contains("match_here"));
        assert!(result.content.contains("2:")); // line number
    }

    #[tokio::test]
    async fn test_grep_output_mode_count() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "foo\nfoo\nbar").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "foo", "output_mode": "count"}), &ctx(&dir))
            .await.unwrap();

        assert!(result.content.contains("2")); // 2 matches
        assert!(result.content.contains("Total:"));
    }

    #[tokio::test]
    async fn test_grep_context_lines() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ctx.txt"), "before\nmatch_line\nafter").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "match_line", "output_mode": "content", "-C": 1}), &ctx(&dir))
            .await.unwrap();

        assert!(result.content.contains("before"));
        assert!(result.content.contains("match_line"));
        assert!(result.content.contains("after"));
    }

    #[tokio::test]
    async fn test_grep_case_insensitive() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ci.txt"), "Hello World").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "hello", "-i": true, "output_mode": "content"}), &ctx(&dir))
            .await.unwrap();

        assert!(result.content.contains("Hello World"));
    }

    #[tokio::test]
    async fn test_grep_multiline() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ml.txt"), "start\nend").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "start\\nend", "multiline": true, "output_mode": "content"}), &ctx(&dir))
            .await.unwrap();

        assert!(result.content.contains("start"));
    }

    #[tokio::test]
    async fn test_grep_head_limit() {
        let dir = TempDir::new().unwrap();
        let content: String = (1..=10).map(|i| format!("match_{i}")).collect::<Vec<_>>().join("\n");
        std::fs::write(dir.path().join("many.txt"), &content).unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "match_", "output_mode": "content", "head_limit": 3}), &ctx(&dir))
            .await.unwrap();

        let lines: Vec<&str> = result.content.lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[tokio::test]
    async fn test_grep_offset() {
        let dir = TempDir::new().unwrap();
        let content: String = (1..=5).map(|i| format!("item_{i}")).collect::<Vec<_>>().join("\n");
        std::fs::write(dir.path().join("off.txt"), &content).unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "item_", "output_mode": "content", "offset": 2, "head_limit": 2}), &ctx(&dir))
            .await.unwrap();

        let lines: Vec<&str> = result.content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(result.content.contains("item_3"));
    }

    #[tokio::test]
    async fn test_grep_type_filter() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("code.rs"), "use std").unwrap();
        std::fs::write(dir.path().join("readme.md"), "use std").unwrap();

        let result = GrepTool
            .execute(serde_json::json!({"pattern": "use std", "type": "rust"}), &ctx(&dir))
            .await.unwrap();

        assert!(result.content.contains("code.rs"));
        assert!(!result.content.contains("readme.md"));
    }
}
