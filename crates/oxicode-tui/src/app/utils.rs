/// Utility free functions used across app submodules.

/// Summarize tool input for display (show the most relevant field).
pub(crate) fn summarize_input(input: &serde_json::Value) -> String {
    let raw = if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
        cmd.to_string()
    } else if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
        path.to_string()
    } else if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        format!("{pattern} in {path}")
    } else if let Some(query) = input.get("query").and_then(|v| v.as_str()) {
        query.to_string()
    } else if let Some(url) = input.get("url").and_then(|v| v.as_str()) {
        url.to_string()
    } else if let Some(skill) = input.get("skill").and_then(|v| v.as_str()) {
        skill.to_string()
    } else {
        serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())
    };
    // Truncate to 80 chars.
    if let Some((idx, _)) = raw.char_indices().nth(80) {
        format!("{}...", &raw[..idx])
    } else {
        raw
    }
}

/// Return (display_name, summary) for active-tool display.
/// For the agent tool, show subagent_type/name as the label and description as
/// the summary (matching Claude Code's rendering).
pub(crate) fn display_name_and_summary(name: &str, input: &serde_json::Value) -> (String, String) {
    if name == "Agent" || name == "agent" {
        let label = input
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .or_else(|| input.get("name").and_then(|v| v.as_str()))
            .unwrap_or("Agent");
        let summary = input
            .get("description")
            .and_then(|v| v.as_str())
            .or_else(|| input.get("prompt").and_then(|v| v.as_str()))
            .unwrap_or("");
        let truncated = if let Some((idx, _)) = summary.char_indices().nth(80) {
            format!("{}...", &summary[..idx])
        } else {
            summary.to_string()
        };
        return (label.to_string(), truncated);
    }
    (name.to_string(), summarize_input(input))
}

/// Convert a character index to a byte index in a UTF-8 string.
pub(crate) fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(i, _)| i)
}

/// Format a `DateTime<Utc>` as a human-readable relative time string.
pub(crate) fn format_relative_time(dt: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(dt);

    let secs = diff.num_seconds();
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = diff.num_minutes();
    if mins < 60 {
        return if mins == 1 {
            "1 min ago".to_string()
        } else {
            format!("{mins} mins ago")
        };
    }
    let hours = diff.num_hours();
    if hours < 24 {
        return if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{hours} hours ago")
        };
    }
    let days = diff.num_days();
    if days < 30 {
        return if days == 1 {
            "yesterday".to_string()
        } else {
            format!("{days} days ago")
        };
    }
    let months = days / 30;
    if months < 12 {
        return if months == 1 {
            "1 month ago".to_string()
        } else {
            format!("{months} months ago")
        };
    }
    let years = days / 365;
    if years == 1 {
        "1 year ago".to_string()
    } else {
        format!("{years} years ago")
    }
}

/// Detect provider name from model name for status bar display.
pub(crate) fn detect_provider_from_model_name(model: &str) -> String {
    let m = model.to_lowercase();
    if m.starts_with("anthropic.claude") {
        "bedrock".to_string()
    } else if m.starts_with("claude-") || m.starts_with("anthropic/") {
        "anthropic".to_string()
    } else if m.starts_with("gpt-")
        || m.starts_with("o1-")
        || m.starts_with("o3-")
        || m.starts_with("o4-")
    {
        "openai".to_string()
    } else if m.starts_with("deepseek-") || m.starts_with("deepseek/") {
        "deepseek".to_string()
    } else if m.contains(':') && !m.contains('/') {
        "ollama".to_string()
    } else if m.contains('/') && !m.starts_with("anthropic/") && !m.starts_with("deepseek/") {
        "openrouter".to_string()
    } else {
        String::new()
    }
}

/// Dangerous command patterns for permission dialog warning.
pub(crate) const DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf",
    "rm -r",
    "rmdir",
    "sudo ",
    "chmod 777",
    "chmod -R",
    "> /dev/",
    "mkfs",
    "dd if=",
    ":(){ :|:",
    "shutdown",
    "reboot",
    "kill -9",
    "pkill",
    "git push --force",
    "git reset --hard",
    "DROP TABLE",
    "DROP DATABASE",
    "TRUNCATE",
    "DELETE FROM",
];

/// Check if a tool operation is potentially dangerous.
pub(crate) fn is_dangerous_operation(tool_name: &str, input_summary: &str) -> bool {
    if tool_name == "bash" || tool_name == "Bash" {
        let lower = input_summary.to_lowercase();
        return DANGEROUS_PATTERNS
            .iter()
            .any(|p| lower.contains(&p.to_lowercase()));
    }
    // File write/edit to sensitive paths.
    if tool_name == "file_write"
        || tool_name == "file_edit"
        || tool_name == "Write"
        || tool_name == "Edit"
    {
        let sensitive = [
            "/etc/",
            "/usr/",
            "/bin/",
            "/sbin/",
            ".env",
            "credentials",
            ".ssh/",
        ];
        return sensitive.iter().any(|s| input_summary.contains(s));
    }
    false
}
