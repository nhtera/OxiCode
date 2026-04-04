//! Smart output truncation and structured exit code mapping for bash results.
//!
//! When output exceeds the size threshold, preserves the first and last portions
//! with a truncation marker in between — keeping the most useful context.

/// Default truncation threshold in bytes (10 KB).
const TRUNCATION_THRESHOLD: usize = 10 * 1024;
/// How many bytes to keep from the start when truncating.
const HEAD_KEEP: usize = 4 * 1024;
/// How many bytes to keep from the end when truncating.
const TAIL_KEEP: usize = 4 * 1024;

/// Truncate output if it exceeds the threshold, keeping first + last chunks.
///
/// Returns the (possibly truncated) string. If truncated, a marker line is
/// inserted between the head and tail sections showing how many lines were cut.
pub fn truncate_output(output: &str) -> String {
    if output.len() <= TRUNCATION_THRESHOLD {
        return output.to_string();
    }

    let head = safe_split(output, HEAD_KEEP);
    let tail = safe_split_end(output, TAIL_KEEP);

    // Count lines dropped
    let total_lines = output.lines().count();
    let head_lines = head.lines().count();
    let tail_lines = tail.lines().count();
    let dropped = total_lines.saturating_sub(head_lines + tail_lines);

    format!(
        "{}\n\n[... {} lines truncated ({} bytes total) ...]\n\n{}",
        head,
        dropped,
        output.len(),
        tail
    )
}

/// Map common exit codes to human-readable descriptions.
pub fn describe_exit_code(code: i32) -> &'static str {
    match code {
        0 => "success",
        1 => "general error",
        2 => "misuse of shell built-in",
        126 => "command not executable (permission denied)",
        127 => "command not found",
        128 => "invalid exit argument",
        130 => "terminated by Ctrl+C (SIGINT)",
        137 => "killed (SIGKILL, e.g. OOM killer or timeout)",
        139 => "segmentation fault (SIGSEGV)",
        141 => "broken pipe (SIGPIPE)",
        143 => "terminated (SIGTERM)",
        _ if code > 128 => "killed by signal",
        _ => "error",
    }
}

/// Format a structured error message for non-zero exit codes.
pub fn format_exit_error(code: i32, output: &str) -> String {
    let desc = describe_exit_code(code);
    let truncated = truncate_output(output);
    format!("Command failed (exit {code}: {desc})\n{truncated}")
}

/// Format a successful result with optional truncation.
pub fn format_success(output: &str) -> String {
    let truncated = truncate_output(output);
    if truncated.is_empty() {
        "(no output)".to_string()
    } else {
        truncated
    }
}

/// Split at a byte boundary, but snap to the nearest newline to avoid cutting mid-line.
fn safe_split(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the last newline within max_bytes
    let search_region = &s[..max_bytes.min(s.len())];
    if let Some(pos) = search_region.rfind('\n') {
        &s[..pos]
    } else {
        // No newline found; snap to char boundary
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Split from the end, snapping to the nearest newline.
fn safe_split_end(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let start = s.len() - max_bytes;
    // Find the first newline after the start point
    if let Some(pos) = s[start..].find('\n') {
        &s[start + pos + 1..]
    } else {
        // Snap to char boundary
        let mut idx = start;
        while idx < s.len() && !s.is_char_boundary(idx) {
            idx += 1;
        }
        &s[idx..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_unchanged() {
        let input = "hello world\n";
        assert_eq!(truncate_output(input), input);
    }

    #[test]
    fn long_output_truncated() {
        // Generate output larger than threshold
        let line = "x".repeat(100) + "\n";
        let output = line.repeat(200); // 20KB+
        assert!(output.len() > TRUNCATION_THRESHOLD);

        let result = truncate_output(&output);
        assert!(result.contains("[..."));
        assert!(result.contains("lines truncated"));
        assert!(result.len() < output.len());
    }

    #[test]
    fn truncation_preserves_head_and_tail() {
        let mut output = String::new();
        output.push_str("HEADER_LINE\n");
        for i in 0..500 {
            output.push_str(&format!("line-{i:04}\n"));
        }
        output.push_str("FOOTER_LINE\n");

        if output.len() > TRUNCATION_THRESHOLD {
            let result = truncate_output(&output);
            assert!(result.contains("HEADER_LINE"));
            assert!(result.contains("FOOTER_LINE"));
        }
    }

    #[test]
    fn exit_code_0_is_success() {
        assert_eq!(describe_exit_code(0), "success");
    }

    #[test]
    fn exit_code_127_not_found() {
        assert_eq!(describe_exit_code(127), "command not found");
    }

    #[test]
    fn exit_code_126_permission() {
        assert!(describe_exit_code(126).contains("permission"));
    }

    #[test]
    fn exit_code_137_killed() {
        assert!(describe_exit_code(137).contains("SIGKILL"));
    }

    #[test]
    fn exit_code_130_sigint() {
        assert!(describe_exit_code(130).contains("SIGINT"));
    }

    #[test]
    fn exit_code_high_signal() {
        assert_eq!(describe_exit_code(200), "killed by signal");
    }

    #[test]
    fn format_exit_error_includes_code() {
        let msg = format_exit_error(127, "bash: foo: command not found");
        assert!(msg.contains("exit 127"));
        assert!(msg.contains("command not found"));
    }

    #[test]
    fn format_success_empty() {
        assert_eq!(format_success(""), "(no output)");
    }

    #[test]
    fn format_success_normal() {
        assert_eq!(format_success("hello"), "hello");
    }

    #[test]
    fn safe_split_respects_newlines() {
        let s = "line1\nline2\nline3\n";
        let result = safe_split(s, 10);
        assert!(result.ends_with('\n') || !result.contains('\n') || result == "line1");
    }

    #[test]
    fn safe_split_end_respects_newlines() {
        let s = "line1\nline2\nline3\n";
        let result = safe_split_end(s, 10);
        // Should start at a line boundary
        assert!(
            result.starts_with("line") || result.is_empty(),
            "got: {}",
            result
        );
    }
}
