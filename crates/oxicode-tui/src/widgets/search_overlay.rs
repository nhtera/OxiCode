//! Search overlay widget: triggered by `/` key, highlights matches in message view.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

/// Search overlay state.
pub struct SearchOverlay {
    /// Current search query text.
    query: String,
    /// Number of matches found.
    match_count: usize,
    /// Index of currently highlighted match.
    current_match: usize,
    /// Whether the search is active.
    active: bool,
}

impl SearchOverlay {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            match_count: 0,
            current_match: 0,
            active: false,
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.query.clear();
        self.match_count = 0;
        self.current_match = 0;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
    }

    pub fn set_match_count(&mut self, count: usize) {
        self.match_count = count;
        if self.current_match >= count && count > 0 {
            self.current_match = 0;
        }
    }

    pub fn next_match(&mut self) {
        if self.match_count > 0 {
            self.current_match = (self.current_match + 1) % self.match_count;
        }
    }

    pub fn prev_match(&mut self) {
        if self.match_count > 0 {
            self.current_match = self.current_match.checked_sub(1).unwrap_or(self.match_count - 1);
        }
    }

    pub fn current_match_index(&self) -> usize {
        self.current_match
    }
}

impl Default for SearchOverlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Render the search bar at the bottom of the screen.
pub struct SearchBar<'a> {
    overlay: &'a SearchOverlay,
}

impl<'a> SearchBar<'a> {
    pub fn new(overlay: &'a SearchOverlay) -> Self {
        Self { overlay }
    }
}

impl Widget for SearchBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the area first.
        Clear.render(area, buf);

        let status = if self.overlay.match_count > 0 {
            format!(
                " {}/{} ",
                self.overlay.current_match + 1,
                self.overlay.match_count
            )
        } else if self.overlay.query.is_empty() {
            String::new()
        } else {
            " no matches ".to_string()
        };

        let line = Line::from(vec![
            Span::styled("Search: ", Style::default().fg(Color::Yellow)),
            Span::styled(&self.overlay.query, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("_", Style::default().fg(Color::Gray)),
            Span::styled(status, Style::default().fg(Color::DarkGray)),
        ]);

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        Paragraph::new(line)
            .block(block)
            .alignment(Alignment::Left)
            .render(area, buf);
    }
}

/// Check if a line of text contains the search query (case-insensitive).
pub fn line_matches(text: &str, query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    text.to_lowercase().contains(&query.to_lowercase())
}

/// Style for highlighted search matches.
pub fn match_style() -> Style {
    Style::default()
        .bg(Color::Yellow)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_lifecycle() {
        let mut s = SearchOverlay::new();
        assert!(!s.is_active());
        s.activate();
        assert!(s.is_active());
        s.push_char('h');
        s.push_char('i');
        assert_eq!(s.query(), "hi");
        s.pop_char();
        assert_eq!(s.query(), "h");
        s.deactivate();
        assert!(!s.is_active());
    }

    #[test]
    fn test_match_navigation() {
        let mut s = SearchOverlay::new();
        s.set_match_count(3);
        assert_eq!(s.current_match_index(), 0);
        s.next_match();
        assert_eq!(s.current_match_index(), 1);
        s.next_match();
        s.next_match();
        assert_eq!(s.current_match_index(), 0); // wraps
        s.prev_match();
        assert_eq!(s.current_match_index(), 2); // wraps back
    }

    #[test]
    fn test_line_matches() {
        assert!(line_matches("Hello World", "hello"));
        assert!(line_matches("Hello World", "WORLD"));
        assert!(!line_matches("Hello World", "xyz"));
        assert!(!line_matches("Hello World", ""));
    }
}
