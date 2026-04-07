use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

/// Displays a unified diff with color-coded additions/deletions.
pub struct DiffView<'a> {
    old_text: &'a str,
    new_text: &'a str,
    file_path: &'a str,
}

impl<'a> DiffView<'a> {
    pub fn new(old_text: &'a str, new_text: &'a str, file_path: &'a str) -> Self {
        Self {
            old_text,
            new_text,
            file_path,
        }
    }

    /// Generate simple unified-style diff lines.
    fn diff_lines(&self) -> Vec<Line<'a>> {
        let old_lines: Vec<&str> = self.old_text.lines().collect();
        let new_lines: Vec<&str> = self.new_text.lines().collect();

        let mut lines = Vec::new();

        // Header.
        lines.push(Line::from(Span::styled(
            format!("--- a/{}", self.file_path),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(Span::styled(
            format!("+++ b/{}", self.file_path),
            Style::default().fg(Color::Green),
        )));

        // Simple line-by-line diff (not a proper LCS diff, but sufficient for display).
        let max_len = old_lines.len().max(new_lines.len());
        let mut i = 0;
        let mut j = 0;

        while i < old_lines.len() || j < new_lines.len() {
            if i < old_lines.len() && j < new_lines.len() {
                if old_lines[i] == new_lines[j] {
                    lines.push(Line::from(format!(" {}", old_lines[i])));
                    i += 1;
                    j += 1;
                } else {
                    // Show removal then addition.
                    lines.push(Line::from(Span::styled(
                        format!("-{}", old_lines[i]),
                        Style::default().fg(Color::Red),
                    )));
                    lines.push(Line::from(Span::styled(
                        format!("+{}", new_lines[j]),
                        Style::default().fg(Color::Green),
                    )));
                    i += 1;
                    j += 1;
                }
            } else if i < old_lines.len() {
                lines.push(Line::from(Span::styled(
                    format!("-{}", old_lines[i]),
                    Style::default().fg(Color::Red),
                )));
                i += 1;
            } else if j < new_lines.len() {
                lines.push(Line::from(Span::styled(
                    format!("+{}", new_lines[j]),
                    Style::default().fg(Color::Green),
                )));
                j += 1;
            }

            // Safety: prevent infinite loops on huge diffs.
            if lines.len() > max_len + 100 {
                lines.push(Line::from(Span::styled(
                    "... diff truncated",
                    Style::default().fg(Color::DarkGray),
                )));
                break;
            }
        }

        lines
    }
}

impl Widget for DiffView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(" Diff: {} ", self.file_path));

        let lines = self.diff_lines();
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_text(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn diff_header_shows_file_path() {
        let dv = DiffView::new("old", "new", "src/main.rs");
        let lines = dv.diff_lines();
        let raw = span_text(&lines);
        assert!(raw.contains("--- a/src/main.rs"));
        assert!(raw.contains("+++ b/src/main.rs"));
    }

    #[test]
    fn identical_content_shows_context() {
        let dv = DiffView::new("same\nline", "same\nline", "test.txt");
        let lines = dv.diff_lines();
        let raw = span_text(&lines);
        // Context lines start with space.
        assert!(raw.contains(" same"), "Identical lines shown as context");
    }

    #[test]
    fn changed_line_shows_red_and_green() {
        let dv = DiffView::new("hello OLD", "hello NEW", "test.txt");
        let lines = dv.diff_lines();

        let red_lines: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.fg == Some(Color::Red) && s.content.starts_with('-'))
            .collect();
        let green_lines: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.fg == Some(Color::Green) && s.content.starts_with('+'))
            .collect();

        assert!(!red_lines.is_empty(), "Should have red (removal) lines");
        assert!(!green_lines.is_empty(), "Should have green (addition) lines");
    }

    #[test]
    fn added_lines_are_green() {
        let dv = DiffView::new("line1", "line1\nline2", "test.txt");
        let lines = dv.diff_lines();
        let green: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.fg == Some(Color::Green) && s.content.contains("line2"))
            .collect();
        assert!(!green.is_empty(), "Added line should be green");
    }

    #[test]
    fn removed_lines_are_red() {
        let dv = DiffView::new("line1\nline2", "line1", "test.txt");
        let lines = dv.diff_lines();
        let red: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.fg == Some(Color::Red) && s.content.contains("line2"))
            .collect();
        assert!(!red.is_empty(), "Removed line should be red");
    }

    #[test]
    fn empty_diff_only_has_header() {
        let dv = DiffView::new("", "", "empty.txt");
        let lines = dv.diff_lines();
        // Should have exactly the 2 header lines.
        assert_eq!(lines.len(), 2, "Empty diff should only have header");
    }
}
