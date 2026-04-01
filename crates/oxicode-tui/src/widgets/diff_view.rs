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
