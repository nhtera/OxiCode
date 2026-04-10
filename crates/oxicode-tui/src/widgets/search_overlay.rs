//! Search overlay widget: triggered by Ctrl+F, highlights matches in message view.

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
    /// Flattened line indices where matches occur (for scroll navigation).
    match_positions: Vec<usize>,
}

impl SearchOverlay {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            match_count: 0,
            current_match: 0,
            active: false,
            match_positions: Vec::new(),
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.query.clear();
        self.match_count = 0;
        self.current_match = 0;
        self.match_positions.clear();
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

    /// Update match positions from a scan of rendered lines.
    pub fn set_match_positions(&mut self, positions: Vec<usize>) {
        self.match_count = positions.len();
        self.match_positions = positions;
        if self.match_count > 0 && self.current_match >= self.match_count {
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
            self.current_match = self
                .current_match
                .checked_sub(1)
                .unwrap_or(self.match_count - 1);
        }
    }

    pub fn current_match_index(&self) -> usize {
        self.current_match
    }

    /// Get the flattened line index of the current match (for scroll-to).
    pub fn current_match_line(&self) -> Option<usize> {
        self.match_positions.get(self.current_match).copied()
    }

    pub fn match_count(&self) -> usize {
        self.match_count
    }
}

impl Default for SearchOverlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Scan rendered message cache lines for matches against the query.
/// Returns a `Vec<usize>` of flattened line indices where matches occur.
pub fn find_matches_in_cache(
    cached_lines: &[Vec<Line<'_>>],
    query: &str,
) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let mut positions = Vec::new();
    let mut flat_idx = 0;
    for msg_lines in cached_lines {
        for line in msg_lines {
            // Extract plain text from all spans in this line.
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            if text.to_lowercase().contains(&query_lower) {
                positions.push(flat_idx);
            }
            flat_idx += 1;
        }
    }
    positions
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

        let status_style = if self.overlay.match_count > 0 {
            Style::default().fg(Color::Green)
        } else if self.overlay.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Red)
        };

        let line = Line::from(vec![
            Span::styled("Search: ", Style::default().fg(Color::Yellow)),
            Span::styled(
                &self.overlay.query,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("\u{2588}", Style::default().fg(Color::Gray)),
            Span::styled(status, status_style),
            Span::styled(
                " (Ctrl+N/P: next/prev, Esc: close)",
                Style::default().fg(Color::DarkGray),
            ),
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

    #[test]
    fn test_match_positions() {
        let mut s = SearchOverlay::new();
        s.set_match_positions(vec![5, 12, 30]);
        assert_eq!(s.match_count(), 3);
        assert_eq!(s.current_match_line(), Some(5));
        s.next_match();
        assert_eq!(s.current_match_line(), Some(12));
        s.next_match();
        assert_eq!(s.current_match_line(), Some(30));
        s.next_match(); // wraps
        assert_eq!(s.current_match_line(), Some(5));
    }

    #[test]
    fn test_find_matches_in_cache() {
        let lines = vec![
            vec![
                Line::from("Hello world"),
                Line::from("foo bar"),
            ],
            vec![
                Line::from("world peace"),
                Line::from("baz qux"),
            ],
        ];
        let positions = find_matches_in_cache(&lines, "world");
        assert_eq!(positions, vec![0, 2]); // line 0 and line 2
    }

    #[test]
    fn test_find_matches_empty_query() {
        let lines = vec![vec![Line::from("Hello")]];
        let positions = find_matches_in_cache(&lines, "");
        assert!(positions.is_empty());
    }
}
