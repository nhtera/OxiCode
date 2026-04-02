//! Pager widget: less-like scrollable view for long outputs.
//!
//! Supports scrolling (up/down/page-up/page-down), jump to top/bottom, and quit.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

/// Pager state for long output display.
pub struct Pager {
    /// Lines of content to display.
    lines: Vec<String>,
    /// Current scroll offset (first visible line).
    scroll: usize,
    /// Title shown in the border.
    title: String,
    /// Whether the pager is active.
    active: bool,
}

impl Pager {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            scroll: 0,
            title: String::new(),
            active: false,
        }
    }

    /// Open the pager with content.
    pub fn open(&mut self, content: &str, title: &str) {
        self.lines = content.lines().map(String::from).collect();
        self.scroll = 0;
        self.title = title.to_string();
        self.active = true;
    }

    pub fn close(&mut self) {
        self.active = false;
        self.lines.clear();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let max = self.lines.len().saturating_sub(1);
        self.scroll = (self.scroll + amount).min(max);
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn page_down(&mut self, page_height: usize) {
        self.scroll_down(page_height.saturating_sub(2));
    }

    pub fn page_up(&mut self, page_height: usize) {
        self.scroll_up(page_height.saturating_sub(2));
    }

    pub fn jump_top(&mut self) {
        self.scroll = 0;
    }

    pub fn jump_bottom(&mut self) {
        self.scroll = self.lines.len().saturating_sub(1);
    }

    pub fn total_lines(&self) -> usize {
        self.lines.len()
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll
    }
}

impl Default for Pager {
    fn default() -> Self {
        Self::new()
    }
}

/// Renders the pager as a full-screen overlay.
pub struct PagerView<'a> {
    pager: &'a Pager,
}

impl<'a> PagerView<'a> {
    pub fn new(pager: &'a Pager) -> Self {
        Self { pager }
    }
}

impl Widget for PagerView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let visible_height = area.height.saturating_sub(3) as usize; // borders + status line
        let total = self.pager.lines.len();
        let offset = self.pager.scroll;

        let visible_lines: Vec<Line> = self
            .pager
            .lines
            .iter()
            .skip(offset)
            .take(visible_height)
            .map(|l| Line::from(l.as_str()))
            .collect();

        let progress = if total > 0 {
            let pct = ((offset + visible_height).min(total) * 100) / total;
            format!(" {pct}% | line {}/{total} ", offset + 1)
        } else {
            " empty ".to_string()
        };

        let title = if self.pager.title.is_empty() {
            " Pager ".to_string()
        } else {
            format!(" {} ", self.pager.title)
        };

        let block = Block::default()
            .title(title)
            .title_bottom(Line::from(vec![
                Span::styled(progress, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    " q:quit j/k:scroll g/G:top/bottom ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ),
            ]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue));

        Paragraph::new(visible_lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pager_scroll() {
        let mut p = Pager::new();
        p.open("line1\nline2\nline3\nline4\nline5", "test");
        assert!(p.is_active());
        assert_eq!(p.scroll_offset(), 0);
        assert_eq!(p.total_lines(), 5);

        p.scroll_down(2);
        assert_eq!(p.scroll_offset(), 2);
        p.scroll_up(1);
        assert_eq!(p.scroll_offset(), 1);
        p.jump_bottom();
        assert_eq!(p.scroll_offset(), 4);
        p.jump_top();
        assert_eq!(p.scroll_offset(), 0);
    }
}
