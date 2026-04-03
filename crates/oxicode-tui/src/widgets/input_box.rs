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
    /// Vim mode badge (e.g. "N", "I", "V", "C") or None if vim is disabled.
    vim_badge: Option<&'a str>,
    /// Optional command-line buffer for vim command mode.
    command_line: Option<&'a str>,
}

impl<'a> InputBox<'a> {
    pub fn new(text: &'a str, cursor: usize, focused: bool) -> Self {
        Self {
            text,
            cursor,
            focused,
            vim_badge: None,
            command_line: None,
        }
    }

    /// Set vim mode badge (e.g. "N" for Normal).
    pub fn with_vim_badge(mut self, badge: &'a str) -> Self {
        self.vim_badge = Some(badge);
        self
    }

    /// Set command-line content for vim command mode display.
    pub fn with_command_line(mut self, cmd: &'a str) -> Self {
        self.command_line = Some(cmd);
        self
    }
}

impl Widget for InputBox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Build title with optional vim badge.
        let title = match self.vim_badge {
            Some(badge) => {
                format!(" [{badge}] Input (Enter to send, Esc for Normal) ")
            }
            None => " Input (Enter to send, Ctrl+C to quit) ".to_string(),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        // In command mode, show the command buffer instead of input text.
        let display_text = if let Some(cmd) = self.command_line {
            Line::from(vec![
                Span::styled(":", Style::default().fg(Color::Yellow)),
                Span::styled(cmd, Style::default().fg(Color::White)),
                Span::styled("_", Style::default().fg(Color::Gray)),
            ])
        } else if self.text.is_empty() && self.focused {
            Line::from(Span::styled(
                "Type your message...",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(self.text)
        };

        let paragraph = Paragraph::new(display_text).block(block);
        paragraph.render(area, buf);

        // Render cursor (skip in command mode — cursor is shown inline).
        if self.command_line.is_none() && self.focused && area.width > 2 && area.height > 2 {
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
