//! Agent summary service — generates concise summaries after agent completion.
//!
//! Summaries are stored alongside agent results for display in the agent panel.

use crate::spawner::AgentResult;

/// Summary of a completed agent run.
#[derive(Debug, Clone)]
pub struct AgentSummary {
    pub agent_id: String,
    /// Concise summary of what the agent accomplished.
    pub summary: String,
    /// Duration in human-readable format (e.g., "2.3s").
    pub duration_display: String,
    /// Whether the agent completed successfully.
    pub success: bool,
}

impl AgentSummary {
    /// Generate a summary from an `AgentResult`.
    ///
    /// Extracts the first meaningful paragraph from the agent output
    /// and formats it as a concise summary.
    pub fn from_result(result: &AgentResult) -> Self {
        let summary = summarize_output(&result.output, result.is_error);
        let duration_display = format_duration(result.duration);

        Self {
            agent_id: result.agent_id.clone(),
            summary,
            duration_display,
            success: !result.is_error,
        }
    }

    /// One-line display for the agent panel.
    pub fn display_line(&self) -> String {
        let icon = if self.success { "✓" } else { "✗" };
        format!(
            "{icon} [{dur}] {summary}",
            dur = self.duration_display,
            summary = self.summary,
        )
    }
}

/// Extract a concise summary from agent output text.
fn summarize_output(output: &str, is_error: bool) -> String {
    if output.is_empty() {
        return if is_error {
            "Agent failed with no output.".to_string()
        } else {
            "Agent completed with no output.".to_string()
        };
    }

    // Try to parse as JSON (agent mode returns JSON).
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(output) {
        if let Some(text) = json.get("output").and_then(|v| v.as_str()) {
            return truncate_summary(text);
        }
    }

    // Fallback: take first non-empty line(s) up to 200 chars.
    truncate_summary(output)
}

/// Truncate text to a summary-friendly length.
fn truncate_summary(text: &str) -> String {
    let first_line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or(text);

    let chars: String = first_line.chars().take(200).collect();
    if first_line.chars().count() > 200 {
        format!("{chars}…")
    } else {
        chars
    }
}

/// Format a Duration into human-readable form.
fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        let mins = secs / 60.0;
        format!("{mins:.1}m")
    }
}

/// Compress long agent output using LLM (stub — returns truncated text for now).
///
/// When `output` already fits within `max_tokens` (estimated as chars / 4),
/// it is returned unchanged. Otherwise a smart truncation strategy is applied:
/// the first and last paragraphs are preserved, with an ellipsis in between.
///
/// **Future:** wire to an actual LLM provider for semantic compression.
pub fn compress_output(output: &str, max_tokens: usize) -> String {
    // Rough token estimate: 1 token ≈ 4 characters.
    if output.len() / 4 <= max_tokens {
        return output.to_string();
    }

    let paragraphs: Vec<&str> = output.split("\n\n").collect();
    if paragraphs.len() <= 2 {
        // Not enough paragraph structure — fall back to hard truncation.
        return truncate_summary(output);
    }

    format!(
        "{}\n\n...\n\n{}",
        paragraphs[0],
        paragraphs[paragraphs.len() - 1]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawner::AgentResult;
    use std::time::Duration;

    #[test]
    fn test_summary_from_success() {
        let result = AgentResult {
            agent_id: "a1".to_string(),
            output: "Task completed successfully. Created 3 files.".to_string(),
            is_error: false,
            duration: Duration::from_millis(1500),
        };
        let summary = AgentSummary::from_result(&result);
        assert!(summary.success);
        assert!(summary.summary.contains("Task completed"));
        assert_eq!(summary.duration_display, "1.5s");
    }

    #[test]
    fn test_summary_from_error() {
        let result = AgentResult {
            agent_id: "a2".to_string(),
            output: String::new(),
            is_error: true,
            duration: Duration::from_millis(200),
        };
        let summary = AgentSummary::from_result(&result);
        assert!(!summary.success);
        assert!(summary.summary.contains("failed"));
    }

    #[test]
    fn test_summary_from_json_output() {
        let json = serde_json::json!({
            "agent_id": "a3",
            "output": "Found 5 TODOs in the codebase",
            "is_error": false
        });
        let result = AgentResult {
            agent_id: "a3".to_string(),
            output: json.to_string(),
            is_error: false,
            duration: Duration::from_secs(3),
        };
        let summary = AgentSummary::from_result(&result);
        assert!(summary.summary.contains("Found 5 TODOs"));
    }

    #[test]
    fn test_display_line() {
        let summary = AgentSummary {
            agent_id: "a4".to_string(),
            summary: "Completed analysis.".to_string(),
            duration_display: "2.0s".to_string(),
            success: true,
        };
        let line = summary.display_line();
        assert!(line.starts_with('✓'));
        assert!(line.contains("2.0s"));
    }

    #[test]
    fn test_format_duration_ms() {
        assert_eq!(format_duration(Duration::from_millis(50)), "50ms");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(Duration::from_millis(2500)), "2.5s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(90)), "1.5m");
    }

    #[test]
    fn test_truncate_long_output() {
        let long = "A".repeat(500);
        let summary = truncate_summary(&long);
        assert_eq!(summary.chars().count(), 201); // 200 + '…'
    }

    #[test]
    fn compress_output_short_passthrough() {
        let short = "Hello, world!";
        // 13 chars / 4 ≈ 3 tokens — well under any reasonable max_tokens.
        assert_eq!(compress_output(short, 100), short);
    }

    #[test]
    fn compress_output_keeps_first_and_last_paragraphs() {
        let para_a = "First paragraph with some content.";
        let para_b = "Middle paragraph that should be dropped.";
        let para_c = "Last paragraph that is also preserved.";
        let input = format!("{para_a}\n\n{para_b}\n\n{para_c}");

        // Force compression: max_tokens so small that even short text exceeds it.
        let result = compress_output(&input, 1);

        assert!(result.contains(para_a), "first paragraph missing");
        assert!(result.contains(para_c), "last paragraph missing");
        assert!(result.contains("..."), "ellipsis missing");
        assert!(
            !result.contains(para_b),
            "middle paragraph should be dropped"
        );
    }

    #[test]
    fn compress_output_single_paragraph_truncates() {
        // Single paragraph, no "\n\n" — should fall back to truncation.
        let long_single = "word ".repeat(500); // ~2500 chars
        let result = compress_output(&long_single, 1); // max 1 token → triggers compression
                                                       // truncate_summary caps at 200 chars + '…' (3-byte UTF-8).
                                                       // Use char count, not byte length, since '…' is multi-byte.
        assert!(result.chars().count() <= 201); // 200 chars + 1 ellipsis char
    }
}
