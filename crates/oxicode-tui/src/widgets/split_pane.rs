use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Layout calculator that divides an area into left (main) and optional right (side) panels.
///
/// SplitPane is not a ratatui Widget itself — call [`SplitPane::split`] to obtain the
/// computed [`Rect`]s, then render your own widgets into them.
#[derive(Debug, Clone)]
pub struct SplitPane {
    /// Left pane percentage (clamped to 30–90), default 70.
    ratio: u16,
    /// Whether the right pane is visible.
    show_right: bool,
}

impl Default for SplitPane {
    fn default() -> Self {
        Self::new()
    }
}

impl SplitPane {
    pub fn new() -> Self {
        Self {
            ratio: 70,
            show_right: false,
        }
    }

    /// Set the left-pane percentage (clamped to 30–90).
    pub fn with_ratio(mut self, ratio: u16) -> Self {
        self.ratio = ratio.clamp(30, 90);
        self
    }

    /// Set right pane visibility.
    pub fn show_right(mut self, show: bool) -> Self {
        self.show_right = show;
        self
    }

    /// Toggle right pane visibility.
    pub fn toggle_right(&mut self) {
        self.show_right = !self.show_right;
    }

    /// Adjust the split ratio by `delta` percentage points (clamped to 30–90).
    pub fn adjust_ratio(&mut self, delta: i16) {
        // Safe: ratio is 30–90 (fits i16); result is clamped to 30–90 (fits u16).
        #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
        {
            self.ratio = (self.ratio as i16 + delta).clamp(30, 90) as u16;
        }
    }

    /// Returns current ratio.
    pub fn ratio(&self) -> u16 {
        self.ratio
    }

    /// Returns whether the right pane is shown.
    pub fn is_right_visible(&self) -> bool {
        self.show_right
    }

    /// Calculate split areas.
    ///
    /// Returns `(left_area, Some(right_area))` when the right pane is visible and the
    /// total width is at least 40 columns, otherwise returns `(area, None)`.
    pub fn split(&self, area: Rect) -> (Rect, Option<Rect>) {
        if !self.show_right || area.width < 40 {
            return (area, None);
        }
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(self.ratio),
                Constraint::Percentage(100 - self.ratio),
            ])
            .split(area);
        (chunks[0], Some(chunks[1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_clamped_on_construction() {
        assert_eq!(SplitPane::new().with_ratio(0).ratio(), 30);
        assert_eq!(SplitPane::new().with_ratio(100).ratio(), 90);
        assert_eq!(SplitPane::new().with_ratio(60).ratio(), 60);
    }

    #[test]
    fn adjust_ratio_clamps() {
        let mut pane = SplitPane::new(); // ratio = 70
        pane.adjust_ratio(-50);
        assert_eq!(pane.ratio(), 30);
        pane.adjust_ratio(100);
        assert_eq!(pane.ratio(), 90);
    }

    #[test]
    fn toggle_right_flips_visibility() {
        let mut pane = SplitPane::new();
        assert!(!pane.is_right_visible());
        pane.toggle_right();
        assert!(pane.is_right_visible());
        pane.toggle_right();
        assert!(!pane.is_right_visible());
    }

    #[test]
    fn split_returns_single_pane_when_right_hidden() {
        let pane = SplitPane::new(); // show_right = false
        let area = Rect::new(0, 0, 100, 40);
        let (left, right) = pane.split(area);
        assert_eq!(left, area);
        assert!(right.is_none());
    }

    #[test]
    fn split_returns_two_panes_when_right_visible_and_wide() {
        let pane = SplitPane::new().show_right(true).with_ratio(70);
        let area = Rect::new(0, 0, 100, 40);
        let (left, right) = pane.split(area);
        assert!(right.is_some());
        assert!(left.width > 0);
        assert!(right.unwrap().width > 0);
    }

    #[test]
    fn split_collapses_right_when_too_narrow() {
        let pane = SplitPane::new().show_right(true);
        let narrow = Rect::new(0, 0, 39, 20);
        let (_left, right) = pane.split(narrow);
        assert!(right.is_none());
    }
}
