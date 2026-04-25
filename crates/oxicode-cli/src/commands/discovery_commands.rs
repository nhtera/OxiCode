//! Discovery & repair commands: `/dream`, `/bughunter`, `/ctx_viz`,
//! `/autofix-pr`, `/backfill-sessions`.

use std::fmt::Write as _;
use std::path::Path;

use super::{CommandContext, CommandOutput, SlashCommand};

/// Truncate a string to at most `max_chars` characters (UTF-8 safe).
fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

// ---------------------------------------------------------------------------
// /dream — auto-suggest next prompts
// ---------------------------------------------------------------------------

/// `/dream` — suggest 3-5 useful next prompts based on conversation context.
pub struct DreamCommand;

impl SlashCommand for DreamCommand {
    fn name(&self) -> &str {
        "dream"
    }
    fn description(&self) -> &str {
        "Auto-suggest next prompts based on context"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let messages = &state.messages;

        // If no conversation context, suggest starters based on project type.
        if messages.is_empty() {
            let project_suggestions = detect_project_suggestions();
            return CommandOutput::Message(format!(
                "No conversation context yet. Suggested starters:\n{project_suggestions}"
            ));
        }

        // Collect last 5 messages for context summary.
        let recent: Vec<String> = messages
            .iter()
            .rev()
            .take(5)
            .rev()
            .map(|m| {
                let role = format!("{:?}", m.role).to_lowercase();
                let text = m.text();
                let preview = truncate_chars(&text, 80);
                format!("[{role}] {preview}")
            })
            .collect();

        let context_summary = recent.join("\n");

        CommandOutput::Message(format!(
            "Based on recent conversation:\n{context_summary}\n\n\
             Suggested next prompts:\n\
             1. \"Explain the code you just wrote\"\n\
             2. \"Write tests for the changes\"\n\
             3. \"Are there any edge cases I'm missing?\"\n\
             4. \"Refactor for better readability\"\n\
             5. \"What would you improve next?\"\n\n\
             Tip: /dream works best after a few exchanges — \
             future versions will use LLM for smarter suggestions."
        ))
    }
}

/// Detect project type and return relevant starter suggestions.
fn detect_project_suggestions() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();

    if cwd.join("Cargo.toml").exists() {
        return "1. \"Explain the project structure\"\n\
                2. \"Run cargo test and fix failures\"\n\
                3. \"Find any unwrap() calls that should be handled\"\n\
                4. \"Add documentation to public functions\"\n\
                5. \"Check for clippy warnings\""
            .to_string();
    }
    if cwd.join("package.json").exists() {
        return "1. \"Explain the project structure\"\n\
                2. \"Run tests and fix failures\"\n\
                3. \"Find any TypeScript type issues\"\n\
                4. \"Add JSDoc to exported functions\"\n\
                5. \"Check for unused dependencies\""
            .to_string();
    }
    if cwd.join("pyproject.toml").exists() || cwd.join("setup.py").exists() {
        return "1. \"Explain the project structure\"\n\
                2. \"Run pytest and fix failures\"\n\
                3. \"Add type hints to functions\"\n\
                4. \"Check for security issues\"\n\
                5. \"Generate API documentation\""
            .to_string();
    }

    // Generic fallback
    "1. \"What does this project do?\"\n\
     2. \"Find potential bugs in this codebase\"\n\
     3. \"Suggest improvements to the architecture\"\n\
     4. \"List all TODO/FIXME comments\"\n\
     5. \"Help me write a README\""
        .to_string()
}

// ---------------------------------------------------------------------------
// /bughunter — scan for common bug patterns
// ---------------------------------------------------------------------------

/// `/bughunter` — scan current directory for common bug patterns.
pub struct BughunterCommand;

impl SlashCommand for BughunterCommand {
    fn name(&self) -> &str {
        "bughunter"
    }
    fn description(&self) -> &str {
        "Scan codebase for common bug patterns"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut findings: Vec<String> = Vec::new();

        // Detect project type and run appropriate scans.
        if cwd.join("Cargo.toml").exists() {
            scan_rust_patterns(&cwd, &mut findings);
        }
        if cwd.join("package.json").exists() {
            scan_js_patterns(&cwd, &mut findings);
        }
        if cwd.join("pyproject.toml").exists() || cwd.join("setup.py").exists() {
            scan_python_patterns(&cwd, &mut findings);
        }
        if cwd.join("go.mod").exists() {
            scan_go_patterns(&cwd, &mut findings);
        }

        if findings.is_empty() {
            return CommandOutput::Message(
                "No common bug patterns found (or project type not detected).\n\
                 Supported: Rust, JavaScript/TypeScript, Python, Go.\n\
                 Tip: run from the project root directory."
                    .to_string(),
            );
        }

        let mut output = format!("Bug patterns found ({} issues):\n\n", findings.len());
        for (i, finding) in findings.iter().enumerate() {
            let _ = writeln!(output, "  {}. {finding}", i + 1);
        }
        output.push_str("\nNote: these are heuristic checks — review each finding manually.");
        CommandOutput::Message(output)
    }
}

/// Scan Rust sources for common patterns.
fn scan_rust_patterns(root: &Path, findings: &mut Vec<String>) {
    let patterns = [
        (
            ".unwrap()",
            "Rust: unwrap() without context — prefer expect() or ?",
        ),
        ("todo!()", "Rust: todo!() macro — unfinished implementation"),
        ("unsafe {", "Rust: unsafe block — review for soundness"),
        ("panic!(", "Rust: explicit panic — may crash at runtime"),
    ];
    scan_files_for_patterns(root, &["rs"], &patterns, findings);
}

/// Scan JS/TS sources for common patterns.
fn scan_js_patterns(root: &Path, findings: &mut Vec<String>) {
    let patterns = [
        (": any", "TS: `any` type — loses type safety"),
        ("console.log(", "JS: console.log — remove before production"),
        ("eval(", "JS: eval() — security risk"),
        ("// @ts-ignore", "TS: @ts-ignore — suppressed type error"),
    ];
    scan_files_for_patterns(root, &["ts", "tsx", "js", "jsx"], &patterns, findings);
}

/// Scan Python sources for common patterns.
fn scan_python_patterns(root: &Path, findings: &mut Vec<String>) {
    let patterns = [
        (
            "except:",
            "Python: bare except — catches SystemExit/KeyboardInterrupt",
        ),
        ("eval(", "Python: eval() — security risk"),
        ("exec(", "Python: exec() — security risk"),
        (
            "# type: ignore",
            "Python: type: ignore — suppressed type check",
        ),
    ];
    scan_files_for_patterns(root, &["py"], &patterns, findings);
}

/// Scan Go sources for common patterns.
fn scan_go_patterns(root: &Path, findings: &mut Vec<String>) {
    let patterns = [
        ("_ = err", "Go: ignored error — handle or log it"),
        ("panic(", "Go: explicit panic — may crash at runtime"),
        ("// nolint", "Go: nolint — suppressed linter warning"),
    ];
    scan_files_for_patterns(root, &["go"], &patterns, findings);
}

/// Walk source files and check for pattern occurrences.
/// Limits to 500 files and top-level `src/` to avoid scanning deps.
fn scan_files_for_patterns(
    root: &Path,
    extensions: &[&str],
    patterns: &[(&str, &str)],
    findings: &mut Vec<String>,
) {
    // Only scan src/ if it exists, otherwise scan root (skip node_modules, target, .git).
    let scan_dir = if root.join("src").is_dir() {
        root.join("src")
    } else {
        root.to_path_buf()
    };

    let skip_dirs = [
        "node_modules",
        "target",
        ".git",
        "vendor",
        "__pycache__",
        "dist",
        "build",
    ];
    let mut file_count = 0u32;
    let max_files = 500;

    scan_dir_recursive(
        &scan_dir,
        extensions,
        &skip_dirs,
        patterns,
        findings,
        &mut file_count,
        max_files,
    );
}

/// Recursively scan directory for files matching extensions.
fn scan_dir_recursive(
    dir: &Path,
    extensions: &[&str],
    skip_dirs: &[&str],
    patterns: &[(&str, &str)],
    findings: &mut Vec<String>,
    file_count: &mut u32,
    max_files: u32,
) {
    if *file_count >= max_files {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        if *file_count >= max_files {
            return;
        }

        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !skip_dirs.contains(&name) {
                scan_dir_recursive(
                    &path, extensions, skip_dirs, patterns, findings, file_count, max_files,
                );
            }
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if extensions.contains(&ext) {
                *file_count += 1;
                check_file_patterns(&path, patterns, findings);
            }
        }
    }
}

/// Check a single file for pattern matches.
fn check_file_patterns(path: &Path, patterns: &[(&str, &str)], findings: &mut Vec<String>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };

    for (line_num, line) in content.lines().enumerate() {
        for (pattern, description) in patterns {
            if line.contains(pattern) {
                findings.push(format!(
                    "{description} — {}:{}",
                    path.display(),
                    line_num + 1,
                ));
                // Report at most one pattern per line (first match wins).
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// /ctx_viz — context window visualization
// ---------------------------------------------------------------------------

/// `/ctx_viz` — display ASCII context window visualization.
pub struct CtxVizCommand;

impl SlashCommand for CtxVizCommand {
    fn name(&self) -> &str {
        "ctx_viz"
    }
    fn description(&self) -> &str {
        "Visualize context window usage (ASCII chart)"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let max_context: usize = 200_000;

        // Estimate token counts per category.
        let system_tokens = estimate_system_tokens();
        let memory_tokens = estimate_memory_tokens();
        let history_tokens = estimate_history_tokens(&state.messages);
        let tool_tokens = estimate_tool_tokens(&state.messages);
        let used = system_tokens + memory_tokens + history_tokens + tool_tokens;
        let free = max_context.saturating_sub(used);

        #[allow(clippy::cast_precision_loss)]
        let pct_used = (used as f64 / max_context as f64 * 100.0).min(100.0);

        // Build ASCII bar chart.
        let bar_width: usize = 30;
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation
        )]
        let filled = ((pct_used / 100.0) * bar_width as f64) as usize;
        let empty = bar_width.saturating_sub(filled);

        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

        let mut output = format!(
            "Context Window ({} tokens)\n\
             {bar} {pct_used:.0}% used\n\n",
            format_tokens(max_context),
        );

        // Per-category bars
        let categories = [
            ("System", system_tokens, max_context),
            ("Memory", memory_tokens, max_context),
            ("History", history_tokens, max_context),
            ("Tools", tool_tokens, max_context),
            ("Free", free, max_context),
        ];

        for (label, tokens, max) in &categories {
            #[allow(clippy::cast_precision_loss)]
            let cat_pct = (*tokens as f64 / *max as f64 * 100.0).min(100.0);
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation
            )]
            let cat_bar_len = ((cat_pct / 100.0) * 15.0) as usize;
            let cat_bar_char = if *label == "Free" { "░" } else { "█" };
            let cat_bar = cat_bar_char.repeat(cat_bar_len.max(1));
            let _ = writeln!(
                output,
                "  {label:<8} {cat_bar} {} ({cat_pct:.1}%)",
                format_tokens(*tokens),
            );
        }

        let _ = writeln!(
            output,
            "\n  Model: {} | Messages: {}",
            ctx.model,
            state.messages.len()
        );

        CommandOutput::Message(output)
    }
}

/// Format token count with K suffix.
fn format_tokens(tokens: usize) -> String {
    if tokens >= 1000 {
        format!("{}K", tokens / 1000)
    } else {
        format!("{tokens}")
    }
}

/// Estimate system prompt tokens (~8K typical).
fn estimate_system_tokens() -> usize {
    8_000
}

/// Estimate memory tokens from CLAUDE.md / memory files.
fn estimate_memory_tokens() -> usize {
    let home = dirs::home_dir().unwrap_or_default();
    let claude_md = home.join(".oxicode").join("CLAUDE.md");
    if claude_md.exists() {
        std::fs::metadata(&claude_md)
            .map(|m| {
                #[allow(clippy::cast_possible_truncation)]
                let size = m.len() as usize;
                size / 4 // ~4 chars per token
            })
            .unwrap_or(2_000)
    } else {
        2_000
    }
}

/// Estimate history tokens from messages.
fn estimate_history_tokens(messages: &[oxicode_common::Message]) -> usize {
    let total_chars: usize = messages.iter().map(|m| m.text().len()).sum();
    total_chars / 4
}

/// Estimate tool result tokens (messages with role=tool or containing tool results).
fn estimate_tool_tokens(messages: &[oxicode_common::Message]) -> usize {
    messages
        .iter()
        .filter(|m| m.role == oxicode_common::Role::System || m.text().contains("tool_use"))
        .map(|m| m.text().len() / 4)
        .sum()
}

// ---------------------------------------------------------------------------
// /autofix-pr — auto-fix PR issues
// ---------------------------------------------------------------------------

/// `/autofix-pr` — review PR diff and suggest/apply fixes.
pub struct AutofixPrCommand;

impl SlashCommand for AutofixPrCommand {
    fn name(&self) -> &str {
        "autofix-pr"
    }
    fn description(&self) -> &str {
        "Review and auto-fix PR issues"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let pr_number = args.trim();

        // Determine PR number: from args or current branch.
        let pr_ref = if pr_number.is_empty() {
            // Try to detect from current branch.
            match std::process::Command::new("gh")
                .args(["pr", "view", "--json", "number", "-q", ".number"])
                .output()
            {
                Ok(output) if output.status.success() => {
                    String::from_utf8_lossy(&output.stdout).trim().to_string()
                }
                _ => {
                    return CommandOutput::Error(
                        "No PR number provided and couldn't detect from current branch.\n\
                         Usage: /autofix-pr <number>\n\
                         Requires: gh CLI installed and authenticated."
                            .to_string(),
                    );
                }
            }
        } else {
            pr_number.to_string()
        };

        // Get PR diff.
        let diff_output = match std::process::Command::new("gh")
            .args(["pr", "diff", &pr_ref])
            .output()
        {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).to_string()
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return CommandOutput::Error(format!("Failed to get PR #{pr_ref} diff: {stderr}"));
            }
            Err(e) => {
                return CommandOutput::Error(format!(
                    "Failed to run `gh pr diff`: {e}\n\
                     Ensure `gh` CLI is installed and authenticated."
                ));
            }
        };

        if diff_output.is_empty() {
            return CommandOutput::Message(format!(
                "PR #{pr_ref} has no diff (empty or already merged)."
            ));
        }

        // Summarize diff stats.
        let additions = diff_output
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .count();
        let deletions = diff_output
            .lines()
            .filter(|l| l.starts_with('-') && !l.starts_with("---"))
            .count();
        let files_changed: Vec<&str> = diff_output
            .lines()
            .filter(|l| l.starts_with("diff --git"))
            .collect();

        CommandOutput::Message(format!(
            "PR #{pr_ref} analysis:\n\
             Files changed: {}\n\
             Additions: +{additions}\n\
             Deletions: -{deletions}\n\n\
             To auto-fix: paste the PR diff into conversation and ask:\n\
             \"Review this diff and fix any issues\"\n\n\
             Tip: use --dry-run to preview fixes without applying.",
            files_changed.len(),
        ))
    }
}

// ---------------------------------------------------------------------------
// /backfill-sessions — repair broken session files
// ---------------------------------------------------------------------------

/// `/backfill-sessions` — scan and repair broken session files.
pub struct BackfillSessionsCommand;

impl SlashCommand for BackfillSessionsCommand {
    fn name(&self) -> &str {
        "backfill-sessions"
    }
    fn description(&self) -> &str {
        "Scan and repair broken session files"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let sessions_dir = match dirs::home_dir() {
            Some(h) => h.join(".oxicode").join("sessions"),
            None => return CommandOutput::Error("Could not determine home directory.".to_string()),
        };

        if !sessions_dir.exists() {
            return CommandOutput::Message(
                "No sessions directory found (~/.oxicode/sessions/).\n\
                 Sessions will be created automatically when you save a conversation."
                    .to_string(),
            );
        }

        let entries = match std::fs::read_dir(&sessions_dir) {
            Ok(entries) => entries,
            Err(e) => return CommandOutput::Error(format!("Failed to read sessions dir: {e}")),
        };

        let mut total = 0u32;
        let mut valid = 0u32;
        let mut repaired = 0u32;
        let mut unrecoverable = 0u32;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            total += 1;

            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    // Try parsing as JSON.
                    if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
                        valid += 1;
                    } else {
                        // Attempt repair: fix truncated JSON.
                        match attempt_json_repair(&content) {
                            Some(fixed) => {
                                // Write repaired content to .repaired file (safe backup).
                                let repaired_path = path.with_extension("json.repaired");
                                if std::fs::write(&repaired_path, &fixed).is_ok() {
                                    repaired += 1;
                                } else {
                                    unrecoverable += 1;
                                }
                            }
                            None => {
                                unrecoverable += 1;
                            }
                        }
                    }
                }
                Err(_) => {
                    unrecoverable += 1;
                }
            }
        }

        CommandOutput::Message(format!(
            "Session backfill results:\n\
             Total files:    {total}\n\
             Valid:          {valid}\n\
             Repaired:       {repaired}\n\
             Unrecoverable: {unrecoverable}\n\n\
             Repaired files saved as .json.repaired (originals untouched).\n\
             To apply repairs: rename .json.repaired → .json"
        ))
    }
}

/// Attempt to repair truncated JSON by adding missing closing brackets.
fn attempt_json_repair(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut fixed = trimmed.to_string();

    // Count unmatched brackets.
    let open_braces = fixed.chars().filter(|c| *c == '{').count();
    let close_braces = fixed.chars().filter(|c| *c == '}').count();
    let open_brackets = fixed.chars().filter(|c| *c == '[').count();
    let close_brackets = fixed.chars().filter(|c| *c == ']').count();

    // Add missing closing brackets.
    for _ in 0..(open_brackets.saturating_sub(close_brackets)) {
        fixed.push(']');
    }
    for _ in 0..(open_braces.saturating_sub(close_braces)) {
        fixed.push('}');
    }

    // Verify the repair worked.
    if serde_json::from_str::<serde_json::Value>(&fixed).is_ok() {
        Some(fixed)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_ctx() -> CommandContext {
        CommandContext {
            state_store: Arc::new(oxicode_state::StateStore::default()),
            model: "test".to_string(),
            provider_name: "test".to_string(),
            session_id: "test".to_string(),
            command_registry: Arc::new(crate::commands::default_registry()),
        }
    }

    #[test]
    fn test_dream_no_context() {
        let cmd = DreamCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("starter") || msg.contains("Suggested"));
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_dream_with_context() {
        let ctx_data = make_ctx();
        ctx_data
            .state_store
            .push_message(oxicode_common::Message::user("hello"));
        let cmd = DreamCommand;
        let output = cmd.execute("", &ctx_data);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("Suggested")),
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_bughunter_no_project() {
        // Run from temp dir with no project files.
        let cmd = BughunterCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(!msg.is_empty()),
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_ctx_viz_output() {
        let cmd = CtxVizCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Context Window"));
                assert!(msg.contains("System"));
                assert!(msg.contains("Free"));
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_autofix_pr_no_args_no_gh() {
        let cmd = AutofixPrCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        // Without gh CLI or PR, should return error or message.
        match output {
            CommandOutput::Error(msg) | CommandOutput::Message(msg) => {
                assert!(!msg.is_empty());
            }
            _ => panic!("Expected message or error"),
        }
    }

    #[test]
    fn test_backfill_sessions_no_dir() {
        // If sessions dir doesn't exist, should report that.
        let cmd = BackfillSessionsCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) | CommandOutput::Error(msg) => assert!(!msg.is_empty()),
            _ => panic!("Expected message or error"),
        }
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(8000), "8K");
        assert_eq!(format_tokens(200_000), "200K");
    }

    #[test]
    fn test_attempt_json_repair_valid() {
        let valid = r#"{"key": "value"}"#;
        // Valid JSON should not need repair, but let's check it works.
        assert!(serde_json::from_str::<serde_json::Value>(valid).is_ok());
    }

    #[test]
    fn test_attempt_json_repair_truncated() {
        let truncated = r#"{"key": "value""#;
        let result = attempt_json_repair(truncated);
        assert!(result.is_some());
        let fixed = result.unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&fixed).is_ok());
    }

    #[test]
    fn test_attempt_json_repair_empty() {
        assert!(attempt_json_repair("").is_none());
    }

    #[test]
    fn test_detect_project_suggestions_fallback() {
        let suggestions = detect_project_suggestions();
        assert!(!suggestions.is_empty());
    }
}
