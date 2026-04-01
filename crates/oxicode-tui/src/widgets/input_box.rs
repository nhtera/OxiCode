use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

/// Text input widget for user messages.
pub struct InputBox<'a> {
    /// Current input text.
    text: &'a str,
    /// Cursor position in the text.
    cursor: usize,
    /// Whether the input is focused.
    focused: bool,
}

impl<'a> InputBox<'a> {
    pub fn new(text: &'a str, cursor: usize, focused: bool) -> Self {
        Self {
            text,
            cursor,
            focused,
        }
    }
}

impl Widget for InputBox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Input (Enter to send, Ctrl+C to quit) ");

        let display_text = if self.text.is_empty() && self.focused {
            Line::from(Span::styled(
                "Type your message...",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(self.text)
        };

        let paragraph = Paragraph::new(display_text).block(block);
        paragraph.render(area, buf);

        // Render cursor
        if self.focused && area.width > 2 && area.height > 2 {
            #[allow(clippy::cast_possible_truncation)]
            let cursor_x = area.x + 1 + self.cursor.min(area.width as usize - 2) as u16;
            let cursor_y = area.y + 1;
            if let Some(cell) = buf.cell_mut((cursor_x, cursor_y)) {
                cell.set_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
    }
}
