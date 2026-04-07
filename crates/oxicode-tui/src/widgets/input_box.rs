use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

/// Maximum input box height in lines (excluding borders).
pub const MAX_INPUT_LINES: u16 = 10;

/// Text input widget for user messages (supports multiline via Alt+Enter).
pub struct InputBox<'a> {
    /// Current input text.
    text: &'a str,
    /// Cursor position in the text (byte offset).
    cursor: usize,
    /// Whether the input is focused.
    focused: bool,
    /// Vim mode badge (e.g. "N", "I", "V", "C") or None if vim is disabled.
    vim_badge: Option<&'a str>,
    /// Optional command-line buffer for vim command mode.
    command_line: Option<&'a str>,
    /// Ghost text completion shown after cursor (dimmed).
    ghost_text: Option<&'a str>,
}

impl<'a> InputBox<'a> {
    pub fn new(text: &'a str, cursor: usize, focused: bool) -> Self {
        Self {
            text,
            cursor,
            focused,
            vim_badge: None,
            command_line: None,
            ghost_text: None,
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

    /// Set ghost text completion (dimmed text shown after cursor).
    pub fn with_ghost_text(mut self, text: &'a str) -> Self {
        self.ghost_text = Some(text);
        self
    }

    /// Calculate required height for the input box (including borders).
    pub fn required_height(text: &str) -> u16 {
        let line_count = text.lines().count().max(1) as u16;
        // +2 for top/bottom border
        (line_count + 2).min(MAX_INPUT_LINES + 2)
    }
}

impl Widget for InputBox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Build title with optional vim badge and multiline hint.
        let title = match self.vim_badge {
            Some(badge) => {
                format!(" [{badge}] Input (Enter to send, Esc for Normal) ")
            }
            None => " Input (Enter to send, Alt+Enter for newline) ".to_string(),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        // In command mode, show the command buffer instead of input text.
        if let Some(cmd) = self.command_line {
            let display_text = Line::from(vec![
                Span::styled(":", Style::default().fg(Color::Yellow)),
                Span::styled(cmd, Style::default().fg(Color::White)),
                Span::styled("_", Style::default().fg(Color::Gray)),
            ]);
            let paragraph = Paragraph::new(display_text).block(block);
            paragraph.render(area, buf);
            return;
        }

        if self.text.is_empty() && self.focused {
            let display_text = Line::from(Span::styled(
                "Type your message...",
                Style::default().fg(Color::DarkGray),
            ));
            let paragraph = Paragraph::new(display_text).block(block);
            paragraph.render(area, buf);
            // Render cursor at start
            if area.width > 2 && area.height > 2 {
                if let Some(cell) = buf.cell_mut((area.x + 1, area.y + 1)) {
                    cell.set_style(
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
            return;
        }

        // Multiline: split by newlines for rendering.
        // Append ghost text to the last line if present.
        let mut lines: Vec<Line<'_>> = self.text.split('\n').map(Line::from).collect();
        if let Some(ghost) = self.ghost_text {
            if let Some(last_line) = lines.last_mut() {
                last_line.spans.push(Span::styled(
                    ghost,
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);

        // Render cursor — compute row/col from byte offset.
        if self.focused && area.width > 2 && area.height > 2 {
            let (cursor_row, cursor_col) = cursor_row_col(self.text, self.cursor);
            let inner_width = (area.width - 2) as usize;
            // Account for line wrapping within each row.
            let mut visual_row: u16 = 0;
            for (i, line_text) in self.text.split('\n').enumerate() {
                let wrapped_lines = (line_text.len() / inner_width.max(1)) as u16 + 1;
                if i == cursor_row {
                    // Cursor is in this line — add offset within wrapping.
                    let col_in_line = cursor_col.min(line_text.len());
                    visual_row += (col_in_line / inner_width.max(1)) as u16;
                    let visual_col = col_in_line % inner_width.max(1);
                    let cx = area.x + 1 + visual_col as u16;
                    let cy = area.y + 1 + visual_row;
                    if cy < area.y + area.height - 1 && cx < area.x + area.width - 1 {
                        if let Some(cell) = buf.cell_mut((cx, cy)) {
                            cell.set_style(
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            );
                        }
                    }
                    break;
                }
                visual_row += wrapped_lines;
            }
        }
    }
}

/// Convert a byte cursor offset into (row, col) for multiline text.
fn cursor_row_col(text: &str, byte_cursor: usize) -> (usize, usize) {
    let before = &text[..byte_cursor.min(text.len())];
    let row = before.matches('\n').count();
    let col = before.rfind('\n').map_or(before.len(), |pos| before.len() - pos - 1);
    (row, col)
}
