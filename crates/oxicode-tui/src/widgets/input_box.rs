use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use crate::render;

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
    /// Input metrics: (char_count, line_count). Shown in right title when set.
    metrics: Option<(usize, usize)>,
    /// Number of pending image attachments to show as a pill.
    image_count: usize,
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
            metrics: None,
            image_count: 0,
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

    /// Set input metrics (char count, line count) for display in right title.
    pub fn with_metrics(mut self, chars: usize, lines: usize) -> Self {
        self.metrics = Some((chars, lines));
        self
    }

    /// Set the number of pending image attachments.
    pub fn with_image_count(mut self, count: usize) -> Self {
        self.image_count = count;
        self
    }

    /// Calculate required height for the input box (including borders).
    /// Images are shown inline as `[Image #N]` tags, so they don't add height.
    pub fn required_height(text: &str, _image_count: usize) -> u16 {
        // Use split('\n') instead of lines() — lines() ignores a trailing '\n',
        // which would hide the empty line the cursor sits on after Alt+Enter.
        let line_count = text.split('\n').count().max(1) as u16;
        // +2 for top/bottom border.
        (line_count + 2).min(MAX_INPUT_LINES + 2)
    }
}

impl Widget for InputBox<'_> {
    #[allow(clippy::too_many_lines)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused {
            Style::default().fg(render::FOCUS_BORDER)
        } else {
            Style::default().fg(render::BORDER_COLOR)
        };

        // Build title with optional vim badge and multiline hint.
        let title = match self.vim_badge {
            Some(badge) => {
                format!(" [{badge}] Input (Enter to send, Esc for Normal) ")
            }
            None => " Input (Enter to send, Alt+Enter for newline) ".to_string(),
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        // Show input metrics on the right side of the border.
        if let Some((chars, lines)) = self.metrics {
            let metrics_text = if lines > 1 {
                format!(" {chars}c · {lines}L ")
            } else {
                format!(" {chars}c ")
            };
            block = block.title_bottom(
                Line::from(Span::styled(
                    metrics_text,
                    Style::default().fg(render::CHROME_MUTED),
                ))
                .alignment(ratatui::layout::Alignment::Right),
            );
        }

        // In command mode, show the command buffer instead of input text.
        if let Some(cmd) = self.command_line {
            let display_text = Line::from(vec![
                Span::styled(":", Style::default().fg(render::STATUS_YELLOW)),
                Span::styled(cmd, Style::default().fg(render::TRANSCRIPT_TEXT)),
                Span::styled("_", Style::default().fg(render::CHROME_MUTED)),
            ]);
            let paragraph = Paragraph::new(display_text).block(block);
            paragraph.render(area, buf);
            return;
        }

        if self.text.is_empty() && self.focused {
            if self.image_count > 0 {
                // Show inline [Image #N] tags when images are pending with empty text.
                let mut spans: Vec<Span<'_>> = Vec::new();
                for i in 1..=self.image_count {
                    spans.push(Span::styled(
                        format!("[Image #{i}]"),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ));
                    spans.push(Span::raw(" "));
                }
                let display_text = Line::from(spans);
                let paragraph = Paragraph::new(display_text).block(block);
                paragraph.render(area, buf);
            } else {
                let display_text = Line::from(Span::styled(
                    "Type your message...",
                    Style::default().fg(render::CHROME_MUTED),
                ));
                let paragraph = Paragraph::new(display_text).block(block);
                paragraph.render(area, buf);
            }
            // Render cursor at start (or after image tags).
            if area.width > 2 && area.height > 2 {
                let cursor_x = if self.image_count > 0 {
                    // Position cursor after all image tags: each is "[Image #N] " chars.
                    let total_chars: usize = (1..=self.image_count)
                        .map(|i| format!("[Image #{i}]").len() + 1)
                        .sum();
                    area.x + 1 + total_chars.min((area.width - 2) as usize) as u16
                } else {
                    area.x + 1
                };
                if let Some(cell) = buf.cell_mut((cursor_x, area.y + 1)) {
                    cell.set_style(
                        Style::default()
                            .fg(Color::Black)
                            .bg(render::TRANSCRIPT_TEXT)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
            return;
        }

        // Multiline: split by newlines for rendering.
        // Append ghost text to the last line if present.
        // Prepend [Image #N] tags to the first line if images are pending.
        let mut lines: Vec<Line<'_>> = self.text.split('\n').map(Line::from).collect();
        if self.image_count > 0 {
            if let Some(first_line) = lines.first_mut() {
                let mut image_spans: Vec<Span<'_>> = Vec::new();
                for i in 1..=self.image_count {
                    image_spans.push(Span::styled(
                        format!("[Image #{i}]"),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ));
                    image_spans.push(Span::raw(" "));
                }
                let mut new_spans = image_spans;
                new_spans.append(&mut first_line.spans);
                first_line.spans = new_spans;
            }
        }
        if let Some(ghost) = self.ghost_text {
            if let Some(last_line) = lines.last_mut() {
                last_line.spans.push(Span::styled(
                    ghost,
                    Style::default().fg(render::CHROME_MUTED),
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
                                    .bg(render::TRANSCRIPT_TEXT)
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
    let col = before
        .rfind('\n')
        .map_or(before.len(), |pos| before.len() - pos - 1);
    (row, col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn required_height_single_line() {
        assert_eq!(InputBox::required_height("hello", 0), 3); // 1 line + 2 borders
    }

    #[test]
    fn required_height_multiline() {
        assert_eq!(InputBox::required_height("line1\nline2\nline3", 0), 5); // 3 lines + 2 borders
    }

    #[test]
    fn required_height_empty() {
        assert_eq!(InputBox::required_height("", 0), 3); // min 1 line + 2 borders
    }

    #[test]
    fn required_height_caps_at_max() {
        let long = (0..20).map(|_| "x").collect::<Vec<_>>().join("\n");
        assert_eq!(InputBox::required_height(&long, 0), MAX_INPUT_LINES + 2);
    }

    #[test]
    fn required_height_trailing_newline() {
        // After Alt+Enter at end of line 2, text is "line1\nline2\n".
        // The trailing newline creates an empty 3rd line for the cursor.
        assert_eq!(InputBox::required_height("line1\nline2\n", 0), 5); // 3 lines + 2 borders
    }

    #[test]
    fn required_height_with_images() {
        // Images are inline [Image #N] tags — no extra row needed.
        assert_eq!(InputBox::required_height("hello", 1), 3); // same as without images
        assert_eq!(InputBox::required_height("hello", 3), 3); // same regardless of count
    }

    #[test]
    fn cursor_row_col_at_start() {
        assert_eq!(cursor_row_col("hello", 0), (0, 0));
    }

    #[test]
    fn cursor_row_col_at_end() {
        assert_eq!(cursor_row_col("hello", 5), (0, 5));
    }

    #[test]
    fn cursor_row_col_multiline() {
        assert_eq!(cursor_row_col("ab\ncd\nef", 4), (1, 1)); // at 'd'
        assert_eq!(cursor_row_col("ab\ncd\nef", 6), (2, 0)); // at 'e'
    }

    #[test]
    fn cursor_row_col_beyond_end() {
        // Clamp to text length.
        assert_eq!(cursor_row_col("hi", 100), (0, 2));
    }

    #[test]
    fn cursor_row_col_trailing_newline() {
        // After Alt+Enter at end of "ab\ncd", text becomes "ab\ncd\n" with cursor at byte 6.
        assert_eq!(cursor_row_col("ab\ncd\n", 6), (2, 0)); // row 2, col 0
    }

    #[test]
    fn cursor_row_col_utf8() {
        // Emoji is 4 bytes.
        let text = "hi\u{1f600}";
        assert_eq!(cursor_row_col(text, 6), (0, 6)); // after emoji
    }

    #[test]
    fn render_empty_focused_shows_placeholder() {
        let area = Rect::new(0, 0, 40, 3);
        let mut buf = Buffer::empty(area);
        let widget = InputBox::new("", 0, true);
        widget.render(area, &mut buf);
        let content: String = (0..40)
            .map(|x| buf.cell((x, 1)).unwrap().symbol().to_string())
            .collect();
        assert!(
            content.contains("Type your message"),
            "Should show placeholder: {content}"
        );
    }

    #[test]
    fn render_with_text_shows_text() {
        let area = Rect::new(0, 0, 40, 3);
        let mut buf = Buffer::empty(area);
        let widget = InputBox::new("hello world", 5, true);
        widget.render(area, &mut buf);
        let content: String = (0..40)
            .map(|x| buf.cell((x, 1)).unwrap().symbol().to_string())
            .collect();
        assert!(
            content.contains("hello world"),
            "Should show input text: {content}"
        );
    }

    #[test]
    fn render_does_not_panic_on_tiny_area() {
        let area = Rect::new(0, 0, 2, 2);
        let mut buf = Buffer::empty(area);
        let widget = InputBox::new("test", 2, true);
        // Should not panic.
        widget.render(area, &mut buf);
    }

    #[test]
    fn render_vim_badge_shows_in_title() {
        let area = Rect::new(0, 0, 60, 3);
        let mut buf = Buffer::empty(area);
        let widget = InputBox::new("", 0, true).with_vim_badge("N");
        widget.render(area, &mut buf);
        let content: String = (0..60)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert!(
            content.contains("[N]"),
            "Should show vim badge in title: {content}"
        );
    }

    #[test]
    fn render_metrics_single_line() {
        let area = Rect::new(0, 0, 60, 3);
        let mut buf = Buffer::empty(area);
        let widget = InputBox::new("hello", 5, true).with_metrics(5, 1);
        widget.render(area, &mut buf);
        // Metrics on bottom border row.
        let bottom: String = (0..60)
            .map(|x| buf.cell((x, 2)).unwrap().symbol().to_string())
            .collect();
        assert!(bottom.contains("5c"), "Should show char count: {bottom}");
    }

    #[test]
    fn render_metrics_multiline() {
        let area = Rect::new(0, 0, 60, 5);
        let mut buf = Buffer::empty(area);
        let widget = InputBox::new("line1\nline2\nline3", 17, true).with_metrics(17, 3);
        widget.render(area, &mut buf);
        let bottom: String = (0..60)
            .map(|x| buf.cell((x, 4)).unwrap().symbol().to_string())
            .collect();
        assert!(bottom.contains("17c"), "Should show char count: {bottom}");
        assert!(bottom.contains("3L"), "Should show line count: {bottom}");
    }
}
