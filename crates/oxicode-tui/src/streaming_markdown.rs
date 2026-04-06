//! Newline-gated markdown stream collector.
//!
//! Accumulates streaming text deltas and only parses through the markdown
//! renderer when a complete line (ending with `\n`) is available. This avoids
//! rendering artifacts from half-parsed markdown (e.g. unclosed `**bold**` or
//! incomplete code fences) that would cause visual flicker during streaming.
//!
//! Inspired by the Codex-rs `MarkdownStreamCollector` pattern.

use ratatui::text::Line;

use crate::widgets::markdown_view;

/// Collects streaming text deltas and incrementally renders complete markdown
/// lines. Only lines terminated by `\n` are passed through the markdown parser;
/// the trailing incomplete fragment is held back until the next delta arrives
/// or [`finalize`] is called.
pub struct MarkdownStreamCollector {
    /// Raw text buffer accumulating all deltas received so far.
    buffer: String,
    /// Rendered lines from all committed (newline-terminated) content.
    committed_lines: Vec<Line<'static>>,
    /// Number of rendered lines already committed (index into `committed_lines`
    /// that has been returned to the caller). Used to emit only *new* lines on
    /// each `commit_complete_lines` call.
    committed_count: usize,
}

impl MarkdownStreamCollector {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            committed_lines: Vec::new(),
            committed_count: 0,
        }
    }

    /// Reset all state for a new streaming session.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.committed_lines.clear();
        self.committed_count = 0;
    }

    /// Append a text delta from the streaming API.
    pub fn push_delta(&mut self, delta: &str) {
        self.buffer.push_str(delta);
    }

    /// Re-render the buffer up to the last newline and return only the *new*
    /// lines since the previous commit. If no newline has been received yet,
    /// returns an empty slice.
    pub fn commit_complete_lines(&mut self) -> Vec<Line<'static>> {
        let Some(last_nl) = self.buffer.rfind('\n') else {
            return Vec::new();
        };

        // Parse everything up to (and including) the last newline.
        let complete_source = &self.buffer[..=last_nl];
        let rendered = markdown_view::parse_to_owned_lines(complete_source);

        if rendered.len() <= self.committed_count {
            // No new lines produced (can happen with blank lines).
            self.committed_lines = rendered;
            return Vec::new();
        }

        let new_lines = rendered[self.committed_count..].to_vec();
        self.committed_count = rendered.len();
        self.committed_lines = rendered;
        new_lines
    }

    /// Finalize the stream: parse any remaining buffer content (even without
    /// a trailing newline) and return new lines beyond the last commit.
    pub fn finalize(&mut self) -> Vec<Line<'static>> {
        if self.buffer.is_empty() {
            return Vec::new();
        }

        let rendered = markdown_view::parse_to_owned_lines(&self.buffer);

        if rendered.len() <= self.committed_count {
            self.committed_lines = rendered;
            return Vec::new();
        }

        let new_lines = rendered[self.committed_count..].to_vec();
        self.committed_count = rendered.len();
        self.committed_lines = rendered;
        new_lines
    }

    /// All committed lines rendered so far (for passing to MessageView).
    pub fn lines(&self) -> &[Line<'static>] {
        &self.committed_lines
    }

    /// The trailing text fragment after the last newline (incomplete line).
    /// Returns `None` if the buffer ends with `\n` or is empty.
    pub fn trailing_fragment(&self) -> Option<&str> {
        if self.buffer.is_empty() {
            return None;
        }
        match self.buffer.rfind('\n') {
            Some(idx) => {
                let tail = &self.buffer[idx + 1..];
                if tail.is_empty() {
                    None
                } else {
                    Some(tail)
                }
            }
            None => Some(&self.buffer),
        }
    }
}

impl Default for MarkdownStreamCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_newline_returns_empty() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("hello world");
        let lines = c.commit_complete_lines();
        assert!(lines.is_empty(), "No newline → no committed lines");
        assert_eq!(c.trailing_fragment(), Some("hello world"));
    }

    #[test]
    fn test_single_line_commits() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("hello\n");
        let lines = c.commit_complete_lines();
        assert!(!lines.is_empty(), "Should have committed lines after newline");
        assert!(c.trailing_fragment().is_none());
    }

    #[test]
    fn test_incremental_commits() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("line one\n");
        let first = c.commit_complete_lines();
        let first_count = first.len();
        assert!(first_count > 0);

        c.push_delta("line two\n");
        let second = c.commit_complete_lines();
        assert!(!second.is_empty(), "Second commit should produce new lines");

        // Total committed should be sum.
        assert!(c.lines().len() >= first_count + second.len());
    }

    #[test]
    fn test_finalize_emits_remaining() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("line one\nincomplete");
        let _ = c.commit_complete_lines();

        let final_lines = c.finalize();
        assert!(!final_lines.is_empty(), "Finalize should emit remaining text");
    }

    #[test]
    fn test_clear_resets_state() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("some text\n");
        let _ = c.commit_complete_lines();
        assert!(!c.lines().is_empty());

        c.clear();
        assert!(c.lines().is_empty());
        assert!(c.trailing_fragment().is_none());
    }

    #[test]
    fn test_markdown_bold_rendered() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("**bold text**\n");
        let lines = c.commit_complete_lines();
        // The markdown parser should produce styled output, not literal asterisks.
        let raw: String = lines.iter().flat_map(|l| l.spans.iter()).map(|s| s.content.as_ref()).collect();
        assert!(!raw.contains("**"), "Bold markers should be parsed, got: {raw}");
        assert!(raw.contains("bold text"));
    }
}
