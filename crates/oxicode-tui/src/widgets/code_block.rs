use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use super::highlight;

/// Displays a code block with line numbers and syntax highlighting via `syntect`.
///
/// Falls back to plain white-on-dark when the language is unknown or the input
/// exceeds safety limits (> 512 KB or > 10 000 lines).
pub struct CodeBlockWidget<'a> {
    code: &'a str,
    language: &'a str,
}

impl<'a> CodeBlockWidget<'a> {
    pub fn new(code: &'a str, language: &'a str) -> Self {
        Self { code, language }
    }

    fn to_lines(&self) -> Vec<Line<'static>> {
        highlight::highlight_code(self.code, self.language)
            .unwrap_or_else(|| self.plain_lines())
    }

    /// Plain fallback: white text on dark background with line numbers.
    fn plain_lines(&self) -> Vec<Line<'static>> {
        let bg = Style::default().bg(Color::Rgb(30, 30, 30));
        self.code
            .lines()
            .enumerate()
            .map(|(i, line)| {
                let line_num = format!("{:>4} ", i + 1);
                Line::from(vec![
                    Span::styled(
                        line_num,
                        Style::default()
                            .fg(Color::DarkGray)
                            .bg(Color::Rgb(30, 30, 30)),
                    ),
                    Span::styled(line.to_string(), bg.fg(Color::White)),
                ])
            })
            .collect()
    }
}

impl Widget for CodeBlockWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = if self.language.is_empty() {
            " Code ".to_string()
        } else {
            format!(" {} ", self.language)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(title);

        let lines = self.to_lines();
        let paragraph = Paragraph::new(lines).block(block);

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
    fn line_numbers_are_dark_gray() {
        let cb = CodeBlockWidget::new("hello", "");
        let lines = cb.to_lines();
        let num_span = &lines[0].spans[0];
        assert_eq!(num_span.style.fg, Some(Color::DarkGray));
    }

    #[test]
    fn code_text_has_dark_background() {
        let cb = CodeBlockWidget::new("let x = 1;", "rust");
        let lines = cb.to_lines();
        let code_span = &lines[0].spans[1];
        assert_eq!(code_span.style.bg, Some(Color::Rgb(30, 30, 30)));
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
