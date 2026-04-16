use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::highlight;

/// Displays a code block with line numbers and syntax highlighting via `syntect`.
///
/// Falls back to plain text when the language is unknown or the input
/// exceeds safety limits (> 512 KB or > 10 000 lines).
/// No background color — terminal's own background shows through (Codex style).
pub struct CodeBlockWidget<'a> {
    code: &'a str,
    language: &'a str,
}

impl<'a> CodeBlockWidget<'a> {
    pub fn new(code: &'a str, language: &'a str) -> Self {
        Self { code, language }
    }

    fn to_lines(&self) -> Vec<Line<'static>> {
        highlight::highlight_code(self.code, self.language).unwrap_or_else(|| self.plain_lines())
    }

    /// Plain fallback: muted line numbers + default text, no background.
    fn plain_lines(&self) -> Vec<Line<'static>> {
        self.code
            .lines()
            .enumerate()
            .map(|(i, line)| {
                let line_num = format!("{:>4} ", i + 1);
                Line::from(vec![
                    Span::styled(
                        line_num,
                        Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                    ),
                    Span::styled(
                        line.to_string(),
                        Style::default().fg(crate::render::TRANSCRIPT_TEXT),
                    ),
                ])
            })
            .collect()
    }
}

impl Widget for CodeBlockWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = self.to_lines();

        // Language label (dim, above code).
        let mut all_lines = Vec::new();
        if !self.language.is_empty() {
            all_lines.push(Line::from(Span::styled(
                format!("  {}", self.language),
                Style::default()
                    .fg(crate::render::TRANSCRIPT_MUTED)
                    .add_modifier(Modifier::DIM),
            )));
        }
        all_lines.extend(lines);

        let paragraph = Paragraph::new(all_lines);
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_block_has_line_numbers() {
        let cb = CodeBlockWidget::new("fn main() {\n    println!(\"hi\");\n}", "rust");
        let lines = cb.to_lines();
        assert_eq!(lines.len(), 3);

        // First span of each line should be a line number.
        let first_span = &lines[0].spans[0];
        assert!(first_span.content.trim().starts_with('1'));
        let second_span = &lines[1].spans[0];
        assert!(second_span.content.trim().starts_with('2'));
    }

    #[test]
    fn line_numbers_are_muted() {
        let cb = CodeBlockWidget::new("hello", "");
        let lines = cb.to_lines();
        let num_span = &lines[0].spans[0];
        assert_eq!(num_span.style.fg, Some(crate::render::TRANSCRIPT_MUTED));
    }

    #[test]
    fn code_text_has_no_background() {
        let cb = CodeBlockWidget::new("let x = 1;", "rust");
        let lines = cb.to_lines();
        let code_span = &lines[0].spans[1];
        assert_eq!(code_span.style.bg, None);
    }

    #[test]
    fn empty_code_produces_no_lines() {
        let cb = CodeBlockWidget::new("", "");
        let lines = cb.to_lines();
        assert!(lines.is_empty());
    }

    #[test]
    fn single_line_code() {
        let cb = CodeBlockWidget::new("x = 42", "python");
        let lines = cb.to_lines();
        assert_eq!(lines.len(), 1);
        let raw: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(raw.contains("x = 42"));
    }
}
