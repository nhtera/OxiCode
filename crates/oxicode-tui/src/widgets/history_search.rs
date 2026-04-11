//! Ctrl+R reverse incremental history search overlay.
//!
//! Renders a centered popup with a search query and filtered history results.
//! Enter accepts the selected match, Esc cancels, Up/Down navigate results.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget};

use crate::render;

/// State for the reverse history search overlay.
pub struct HistorySearchState {
    /// The search query typed by the user.
    pub query: String,
    /// Indices into the history store that match the query (newest-first).
    pub matches: Vec<usize>,
    /// Current selection index within `matches`.
    pub selected: usize,
    /// Cached content strings for display (parallel to `matches`).
    pub display_items: Vec<String>,
    /// The input text before search was activated (for restore on cancel).
    pub saved_input: String,
    /// The cursor position before search was activated.
    pub saved_cursor: usize,
}

impl HistorySearchState {
    /// Create a new search state, saving current input for cancel-restore.
    pub fn new(saved_input: String, saved_cursor: usize) -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            selected: 0,
            display_items: Vec::new(),
            saved_input,
            saved_cursor,
        }
    }

    /// Push a character to the search query.
    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
    }

    /// Delete the last character from the search query.
    pub fn pop_char(&mut self) {
        self.query.pop();
    }

    /// Move selection to the next (older) match.
    pub fn select_next(&mut self) {
        if !self.matches.is_empty() {
            self.selected = (self.selected + 1) % self.matches.len();
        }
    }

    /// Move selection to the previous (newer) match.
    pub fn select_prev(&mut self) {
        if !self.matches.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.matches.len() - 1);
        }
    }

    /// Get the content of the currently selected match (if any).
    pub fn selected_content(&self) -> Option<&str> {
        self.display_items
            .get(self.selected)
            .map(String::as_str)
    }

    /// Update results from a history search. `items` are (index, content) pairs.
    pub fn update_results(&mut self, items: Vec<(usize, String)>) {
        self.matches = items.iter().map(|(i, _)| *i).collect();
        self.display_items = items.into_iter().map(|(_, c)| c).collect();
        // Clamp selection.
        if self.selected >= self.matches.len() {
            self.selected = 0;
        }
    }
}

/// Renders the reverse history search overlay.
pub struct HistorySearchWidget<'a> {
    state: &'a HistorySearchState,
}

impl<'a> HistorySearchWidget<'a> {
    pub fn new(state: &'a HistorySearchState) -> Self {
        Self { state }
    }

    /// Compute centered overlay area (60% width, max 15 lines height).
    fn overlay_area(area: Rect) -> Rect {
        let width = (area.width * 3 / 5).max(30).min(area.width.saturating_sub(4));
        let height = 12u16.min(area.height.saturating_sub(4));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        Rect::new(x, y, width, height)
    }
}

impl Widget for HistorySearchWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let overlay = Self::overlay_area(area);

        // Clear background.
        Clear.render(overlay, buf);

        let title = format!(
            " reverse-i-search ({}/{}) ",
            if self.state.matches.is_empty() {
                0
            } else {
                self.state.selected + 1
            },
            self.state.matches.len(),
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(render::FOCUS_BORDER))
            .title(title)
            .title_alignment(Alignment::Left);

        let inner = block.inner(overlay);
        block.render(overlay, buf);

        if inner.height < 3 || inner.width < 10 {
            return;
        }

        // Layout: query line (1) + separator (1) + results list (rest).
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(inner);

        // Query line.
        let query_line = Line::from(vec![
            Span::styled("search: ", Style::default().fg(render::CHROME_MUTED)),
            Span::styled(
                &self.state.query,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("_", Style::default().fg(render::CHROME_MUTED)),
        ]);
        Paragraph::new(query_line).render(chunks[0], buf);

        // Separator line.
        let sep = "─".repeat(chunks[1].width as usize);
        Paragraph::new(Line::from(Span::styled(
            sep,
            Style::default().fg(render::BORDER_COLOR),
        )))
        .render(chunks[1], buf);

        // Results list.
        if self.state.display_items.is_empty() {
            let empty_msg = if self.state.query.is_empty() {
                "Type to search history..."
            } else {
                "No matches"
            };
            Paragraph::new(Line::from(Span::styled(
                empty_msg,
                Style::default().fg(render::CHROME_MUTED),
            )))
            .render(chunks[2], buf);
            return;
        }

        let visible = chunks[2].height as usize;
        // Scroll so selected item is visible.
        let scroll_start = if self.state.selected >= visible {
            self.state.selected - visible + 1
        } else {
            0
        };

        let items: Vec<ListItem<'_>> = self
            .state
            .display_items
            .iter()
            .enumerate()
            .skip(scroll_start)
            .take(visible)
            .map(|(i, content)| {
                // Truncate long lines.
                let max_w = chunks[2].width.saturating_sub(2) as usize;
                let display = if content.len() > max_w {
                    format!("{}…", &content.chars().take(max_w - 1).collect::<String>())
                } else {
                    content.clone()
                };

                let style = if i == self.state.selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(render::FOCUS_BORDER)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(render::TRANSCRIPT_TEXT)
                };
                ListItem::new(Line::from(Span::styled(display, style)))
            })
            .collect();

        let list = List::new(items);
        list.render(chunks[2], buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_empty_query() {
        let state = HistorySearchState::new("saved".into(), 5);
        assert!(state.query.is_empty());
        assert_eq!(state.saved_input, "saved");
        assert_eq!(state.saved_cursor, 5);
    }

    #[test]
    fn select_next_wraps() {
        let mut state = HistorySearchState::new(String::new(), 0);
        state.update_results(vec![
            (2, "c".into()),
            (1, "b".into()),
            (0, "a".into()),
        ]);
        assert_eq!(state.selected, 0);
        state.select_next();
        assert_eq!(state.selected, 1);
        state.select_next();
        assert_eq!(state.selected, 2);
        state.select_next();
        assert_eq!(state.selected, 0); // wraps
    }

    #[test]
    fn select_prev_wraps() {
        let mut state = HistorySearchState::new(String::new(), 0);
        state.update_results(vec![(1, "b".into()), (0, "a".into())]);
        assert_eq!(state.selected, 0);
        state.select_prev();
        assert_eq!(state.selected, 1); // wraps to end
    }

    #[test]
    fn selected_content_returns_match() {
        let mut state = HistorySearchState::new(String::new(), 0);
        state.update_results(vec![(1, "hello".into()), (0, "world".into())]);
        assert_eq!(state.selected_content(), Some("hello"));
        state.select_next();
        assert_eq!(state.selected_content(), Some("world"));
    }

    #[test]
    fn empty_results_selected_content_none() {
        let state = HistorySearchState::new(String::new(), 0);
        assert!(state.selected_content().is_none());
    }

    #[test]
    fn render_does_not_panic_small_area() {
        let state = HistorySearchState::new(String::new(), 0);
        let widget = HistorySearchWidget::new(&state);
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }
}
