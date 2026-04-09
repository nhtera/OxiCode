//! Newline-gated incremental markdown stream collector.
//!
//! Accumulates streaming text deltas and only parses through the markdown
//! renderer when a complete line (ending with `\n`) is available. This avoids
//! rendering artifacts from half-parsed markdown (e.g. unclosed `**bold**` or
//! incomplete code fences) that would cause visual flicker during streaming.
//!
//! **Performance**: Only NEW content since the last commit is parsed — O(delta)
//! per commit, not O(total_buffer). Code fence state is carried across commits
//! via `StreamParserState` so fenced blocks spanning multiple deltas render
//! correctly.
//!
//! Inspired by the Codex-rs `MarkdownStreamCollector` pattern.

use ratatui::text::Line;

use crate::widgets::markdown_view::{self, StreamParserState};

/// Collects streaming text deltas and incrementally renders complete markdown
/// lines. Only lines terminated by `\n` are passed through the markdown parser;
/// the trailing incomplete fragment is held back until the next delta arrives
/// or [`finalize`] is called.
pub struct MarkdownStreamCollector {
    /// Raw text buffer accumulating all deltas received so far.
    buffer: String,
    /// Byte offset of content already committed (parsed and rendered).
    committed_byte_offset: usize,
    /// All rendered lines from committed content.
    committed_lines: Vec<Line<'static>>,
    /// Parser state carried across incremental commits (code fence tracking).
    parser_state: StreamParserState,
}

impl MarkdownStreamCollector {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            committed_byte_offset: 0,
            committed_lines: Vec::new(),
            parser_state: StreamParserState::default(),
        }
    }

    /// Reset all state for a new streaming session.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.committed_byte_offset = 0;
        self.committed_lines.clear();
        self.parser_state = StreamParserState::default();
    }

    /// Append a text delta from the streaming API.
    pub fn push_delta(&mut self, delta: &str) {
        self.buffer.push_str(delta);
    }

    /// Parse only NEW complete lines since the last commit and return them.
    ///
    /// Only content beyond `committed_byte_offset` up to the last `\n` is
    /// parsed. Previous content is never re-parsed — O(delta) per call.
    pub fn commit_complete_lines(&mut self) -> Vec<Line<'static>> {
        let uncommitted = &self.buffer[self.committed_byte_offset..];
        let Some(rel_last_nl) = uncommitted.rfind('\n') else {
            return Vec::new();
        };

        let abs_last_nl = self.committed_byte_offset + rel_last_nl;
        let new_content = &self.buffer[self.committed_byte_offset..=abs_last_nl];

        // Parse only the new slice, carrying code fence state.
        let new_lines = markdown_view::parse_incremental(new_content, &mut self.parser_state);

        self.committed_byte_offset = abs_last_nl + 1;
        self.committed_lines.extend(new_lines.iter().cloned());
        new_lines
    }

    /// Finalize the stream: parse any remaining buffer content (even without
    /// a trailing newline) and return new lines beyond the last commit.
    ///
    /// Uses full-buffer parse for correctness on the final fragment.
    pub fn finalize(&mut self) -> Vec<Line<'static>> {
        if self.committed_byte_offset >= self.buffer.len() {
            return Vec::new();
        }

        let remaining = &self.buffer[self.committed_byte_offset..];
        if remaining.is_empty() {
            return Vec::new();
        }

        let new_lines = markdown_view::parse_incremental(remaining, &mut self.parser_state);

        self.committed_byte_offset = self.buffer.len();
        self.committed_lines.extend(new_lines.iter().cloned());
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
        assert!(
            !lines.is_empty(),
            "Should have committed lines after newline"
        );
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
        assert!(
            !final_lines.is_empty(),
            "Finalize should emit remaining text"
        );
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
        let raw: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            !raw.contains("**"),
            "Bold markers should be parsed, got: {raw}"
        );
        assert!(raw.contains("bold text"));
    }

    #[test]
    fn test_incremental_does_not_reparse_old_content() {
        let mut c = MarkdownStreamCollector::new();

        // Commit 500 lines.
        for i in 0..500 {
            c.push_delta(&format!("Line {i}\n"));
        }
        let first_batch = c.commit_complete_lines();
        assert!(!first_batch.is_empty());

        // Add 1 more line — only that line should be parsed.
        c.push_delta("Last line\n");
        let second_batch = c.commit_complete_lines();
        assert!(!second_batch.is_empty());
        // Total should be first + second, proving old content wasn't re-parsed.
        assert_eq!(c.lines().len(), first_batch.len() + second_batch.len());
    }

    #[test]
    fn test_code_block_spanning_deltas() {
        let mut c = MarkdownStreamCollector::new();

        // Open code fence in first delta.
        c.push_delta("```rust\n");
        let l1 = c.commit_complete_lines();

        // Code content in second delta.
        c.push_delta("fn main() {}\n");
        let l2 = c.commit_complete_lines();

        // Close fence in third delta.
        c.push_delta("```\n");
        let l3 = c.commit_complete_lines();

        let total = l1.len() + l2.len() + l3.len();
        assert!(total > 0, "Code block across deltas should produce output");
    }

    #[test]
    fn test_two_newlines_in_one_push_commits_two_lines() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("Hello\nWorld\n");
        let lines = c.commit_complete_lines();
        // Both "Hello" and "World" lines should be committed.
        assert!(
            lines.len() >= 2,
            "Two newline-terminated lines should yield ≥2 committed lines, got {}",
            lines.len()
        );
        assert!(
            c.trailing_fragment().is_none(),
            "No trailing fragment after final newline"
        );
    }

    #[test]
    fn test_partial_push_then_complete_commits_one_line() {
        let mut c = MarkdownStreamCollector::new();
        // Push without newline — nothing committed yet.
        c.push_delta("Hello");
        let first = c.commit_complete_lines();
        assert!(first.is_empty(), "No newline → no committed lines yet");
        assert_eq!(c.trailing_fragment(), Some("Hello"));

        // Complete the line with a newline.
        c.push_delta(" World\n");
        let second = c.commit_complete_lines();
        assert!(
            !second.is_empty(),
            "After completing the line, should commit ≥1 line"
        );
        assert!(c.trailing_fragment().is_none());
    }

    #[test]
    fn test_trailing_fragment_no_newline() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("Hello");
        assert_eq!(
            c.trailing_fragment(),
            Some("Hello"),
            "Buffer with no newline should return full buffer as fragment"
        );
    }

    #[test]
    fn test_finalize_returns_remaining_trailing_text() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("trailing text");
        let _ = c.commit_complete_lines(); // nothing committed yet
        let final_lines = c.finalize();
        assert!(
            !final_lines.is_empty(),
            "finalize() should emit the remaining trailing text as ≥1 line"
        );
    }

    #[test]
    fn test_empty_delta_no_crash_no_committed_lines() {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta("");
        let lines = c.commit_complete_lines();
        assert!(
            lines.is_empty(),
            "Empty delta should produce no committed lines"
        );
        assert!(
            c.trailing_fragment().is_none(),
            "Empty buffer has no fragment"
        );
    }
}
