//! Keyboard shortcuts overlay: shown when `?` key is pressed.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

/// Keyboard shortcut definition.
struct Shortcut {
    key: &'static str,
    description: &'static str,
}

const SHORTCUTS: &[Shortcut] = &[
    Shortcut { key: "Enter", description: "Send message / submit" },
    Shortcut { key: "Ctrl+C", description: "Cancel / interrupt" },
    Shortcut { key: "Ctrl+D", description: "Quit OxiCode" },
    Shortcut { key: "Ctrl+L", description: "Clear screen" },
    Shortcut { key: "Esc", description: "Cancel input / close overlay" },
    Shortcut { key: "Tab", description: "Accept autocomplete" },
    Shortcut { key: "/", description: "Open search" },
    Shortcut { key: "?", description: "Toggle shortcuts overlay" },
    Shortcut { key: "Up/Down", description: "Scroll messages" },
    Shortcut { key: "PgUp/PgDn", description: "Page scroll" },
    Shortcut { key: "Home/End", description: "Jump to top/bottom" },
    Shortcut { key: "Ctrl+K", description: "Clear input line" },
    Shortcut { key: "Ctrl+W", description: "Delete word backward" },
    Shortcut { key: "Ctrl+U", description: "Delete to line start" },
    Shortcut { key: "Alt+Enter", description: "Newline in input" },
];

/// Whether the shortcuts overlay is currently visible.
pub struct ShortcutsState {
    visible: bool,
}

impl ShortcutsState {
    pub fn new() -> Self {
        Self { visible: false }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }
}

impl Default for ShortcutsState {
    fn default() -> Self {
        Self::new()
    }
}

/// Renders the shortcuts overlay as a centered panel.
pub struct ShortcutsPanel;

impl Widget for ShortcutsPanel {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let lines: Vec<Line> = SHORTCUTS
            .iter()
            .map(|s| {
                Line::from(vec![
                    Span::styled(
                        format!("{:<14}", s.key),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(s.description, Style::default().fg(Color::White)),
                ])
            })
            .collect();

        let block = Block::default()
            .title(" Keyboard Shortcuts ")
            .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue));

        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left)
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shortcuts_toggle() {
        let mut s = ShortcutsState::new();
        assert!(!s.is_visible());
        s.toggle();
        assert!(s.is_visible());
        s.toggle();
        assert!(!s.is_visible());
    }
}
