use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

/// Displays a code block with line numbers and syntax-highlighted style.
///
/// Uses a simple color scheme rather than full syntect highlighting
/// to keep rendering fast during streaming.
pub struct CodeBlockWidget<'a> {
    code: &'a str,
    language: &'a str,
}

impl<'a> CodeBlockWidget<'a> {
    pub fn new(code: &'a str, language: &'a str) -> Self {
        Self { code, language }
    }

    fn to_lines(&self) -> Vec<Line<'a>> {
        let mut lines = Vec::new();
        let bg = Style::default().bg(Color::Rgb(30, 30, 30));

        for (i, line) in self.code.lines().enumerate() {
            let line_num = format!("{:>4} ", i + 1);
            let spans = vec![
                Span::styled(
                    line_num,
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(Color::Rgb(30, 30, 30)),
                ),
                Span::styled(line.to_string(), bg.fg(Color::White)),
            ];
            lines.push(Line::from(spans));
        }

        lines
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
