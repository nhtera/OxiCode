//! Help overlay: two-column modal with keyboard shortcuts (left) and slash
//! commands (right). Triggered by `?` or `F1`. Supports live search filtering.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::modal_helpers::{
    begin_modal, category_header, render_modal_title, render_separator, render_vertical_divider,
    shortcut_line, DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT, PANEL_BG,
};

/// Maximum number of visible rows in the body area before scrolling kicks in.
const MAX_BODY_ROWS: u16 = 28;

// ── State ────────────────────────────────────────────────────────────────────

/// Help overlay state tracked by `App`.
pub struct HelpOverlayState {
    visible: bool,
    /// Live search filter text.
    filter: String,
    /// Scroll offset for right-column command list.
    scroll_offset: u16,
}

impl HelpOverlayState {
    pub fn new() -> Self {
        Self {
            visible: false,
            filter: String::new(),
            scroll_offset: 0,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.filter.clear();
            self.scroll_offset = 0;
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.filter.clear();
        self.scroll_offset = 0;
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.scroll_offset = 0;
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.scroll_offset = 0;
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, max: u16) {
        if self.scroll_offset + 1 < max {
            self.scroll_offset += 1;
        }
    }

    pub fn scroll_offset(&self) -> u16 {
        self.scroll_offset
    }
}

impl Default for HelpOverlayState {
    fn default() -> Self {
        Self::new()
    }
}

/// Backward-compatible alias — used in app.rs `ShortcutsState` import.
pub type ShortcutsState = HelpOverlayState;

// ── Shortcut entry for keybinding display ────────────────────────────────────

/// A keyboard shortcut entry (key label + description + category).
pub struct ShortcutEntry {
    pub key: String,
    pub description: String,
    pub category: String,
}

/// Build the default keyboard shortcut entries. These are the hardcoded
/// well-known shortcuts — the keybinding registry may override key labels.
pub fn default_shortcut_entries() -> Vec<ShortcutEntry> {
    vec![
        // Navigation
        ShortcutEntry {
            key: "PageUp/PgDn".into(),
            description: "Scroll messages".into(),
            category: "Navigation".into(),
        },
        ShortcutEntry {
            key: "Up/Down".into(),
            description: "Input history".into(),
            category: "Navigation".into(),
        },
        ShortcutEntry {
            key: "Home/End".into(),
            description: "Jump to top/bottom".into(),
            category: "Navigation".into(),
        },
        // Input
        ShortcutEntry {
            key: "Enter".into(),
            description: "Send message".into(),
            category: "Input".into(),
        },
        ShortcutEntry {
            key: "Shift+Enter".into(),
            description: "Newline in input".into(),
            category: "Input".into(),
        },
        ShortcutEntry {
            key: "Ctrl+K".into(),
            description: "Clear input line".into(),
            category: "Input".into(),
        },
        ShortcutEntry {
            key: "Ctrl+W".into(),
            description: "Delete word backward".into(),
            category: "Input".into(),
        },
        ShortcutEntry {
            key: "Ctrl+U".into(),
            description: "Delete to line start".into(),
            category: "Input".into(),
        },
        ShortcutEntry {
            key: "Ctrl+A/E".into(),
            description: "Start / end of line".into(),
            category: "Input".into(),
        },
        ShortcutEntry {
            key: "Alt+←/→".into(),
            description: "Word left / right".into(),
            category: "Input".into(),
        },
        // App
        ShortcutEntry {
            key: "? / F1".into(),
            description: "Toggle help".into(),
            category: "App".into(),
        },
        ShortcutEntry {
            key: "F2".into(),
            description: "Model picker".into(),
            category: "App".into(),
        },
        ShortcutEntry {
            key: "F3".into(),
            description: "Session browser".into(),
            category: "App".into(),
        },
        ShortcutEntry {
            key: "Ctrl+F".into(),
            description: "Search messages".into(),
            category: "App".into(),
        },
        ShortcutEntry {
            key: "Ctrl+R".into(),
            description: "Search history".into(),
            category: "App".into(),
        },
        ShortcutEntry {
            key: "Ctrl+T".into(),
            description: "Toggle thinking".into(),
            category: "App".into(),
        },
        ShortcutEntry {
            key: "Tab".into(),
            description: "Side panel / ghost text".into(),
            category: "App".into(),
        },
        ShortcutEntry {
            key: "Ctrl+C".into(),
            description: "Cancel / quit".into(),
            category: "App".into(),
        },
        ShortcutEntry {
            key: "Esc".into(),
            description: "Cancel / close overlay".into(),
            category: "App".into(),
        },
    ]
}

// ── Widget ───────────────────────────────────────────────────────────────────

/// Help overlay widget — two-column modal rendered on top of content.
///
/// Left column: keyboard shortcuts grouped by category.
/// Right column: slash commands filtered by search query.
pub struct HelpOverlay<'a> {
    state: &'a HelpOverlayState,
    shortcuts: &'a [ShortcutEntry],
    /// Slash command entries: (name, description, category).
    commands: &'a [(String, String, String)],
}

impl<'a> HelpOverlay<'a> {
    pub fn new(
        state: &'a HelpOverlayState,
        shortcuts: &'a [ShortcutEntry],
        commands: &'a [(String, String, String)],
    ) -> Self {
        Self {
            state,
            shortcuts,
            commands,
        }
    }
}

/// Backward-compatible alias — `ShortcutsPanel` is still imported in app.rs.
/// This renders nothing by itself; the real rendering goes through `HelpOverlay`.
pub struct ShortcutsPanel;

impl Widget for ShortcutsPanel {
    fn render(self, _area: Rect, _buf: &mut Buffer) {
        // No-op: use HelpOverlay instead.
    }
}

/// Build left-column lines: keyboard shortcuts grouped by category, filtered.
fn build_shortcut_lines(shortcuts: &[ShortcutEntry], filter: &str) -> Vec<Line<'static>> {
    let filter_lc = filter.to_lowercase();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_category = "";

    for entry in shortcuts {
        if !filter_lc.is_empty()
            && !entry.key.to_lowercase().contains(&filter_lc)
            && !entry.description.to_lowercase().contains(&filter_lc)
        {
            continue;
        }
        if entry.category.as_str() != current_category {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(category_header(&entry.category));
            current_category = entry.category.as_str();
        }
        lines.push(shortcut_line(&entry.key, &entry.description));
    }
    lines
}

/// Build right-column lines: slash commands grouped by category, filtered.
/// Returns (lines, filtered_count).
fn build_command_lines(
    commands: &[(String, String, String)],
    filter: &str,
) -> (Vec<Line<'static>>, usize) {
    let filter_lc = filter.to_lowercase();
    let filtered: Vec<&(String, String, String)> = commands
        .iter()
        .filter(|(name, desc, _)| {
            filter_lc.is_empty()
                || name.to_lowercase().contains(&filter_lc)
                || desc.to_lowercase().contains(&filter_lc)
        })
        .collect();

    let count = filtered.len();
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(
        " Slash Commands",
        Style::default()
            .fg(DIALOG_ACCENT)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let mut current_cat = "";
    for (name, desc, cat) in &filtered {
        if cat.as_str() != current_cat {
            if lines.len() > 2 {
                lines.push(Line::from(""));
            }
            lines.push(category_header(cat));
            current_cat = cat.as_str();
        }
        lines.push(Line::from(vec![
            Span::styled(
                format!(" /{name:<16}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(desc.clone(), Style::default().fg(DIALOG_TEXT)),
        ]));
    }

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            " No matching commands",
            Style::default().fg(DIALOG_MUTED),
        )));
    }
    (lines, count)
}

impl Widget for HelpOverlay<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible || area.height < 8 || area.width < 30 {
            return;
        }

        // Modal dimensions — scale with terminal size.
        let dialog_w = 90.min(area.width.saturating_sub(4));
        let dialog_h = MAX_BODY_ROWS.min(area.height.saturating_sub(4));
        // header: title(1) + separator(1) + search(1) = 3, footer: hint(1) = 1
        let layout = begin_modal(buf, area, dialog_w, dialog_h, 3, 1);

        // ── Header ───────────────────────────────────────────────────────────
        render_modal_title(buf, layout.header_area, "Help — Shortcuts & Commands", "esc");

        if layout.header_area.height >= 2 {
            let sep_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 1,
                width: layout.header_area.width,
                height: 1,
            };
            render_separator(buf, sep_area);
        }

        // Search filter line.
        if layout.header_area.height >= 3 {
            let search_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 2,
                width: layout.header_area.width,
                height: 1,
            };
            let search_line = if self.state.filter.is_empty() {
                Line::from(vec![
                    Span::styled(" \u{1F50D} ", Style::default().fg(DIALOG_MUTED)),
                    Span::styled("Type to filter...", Style::default().fg(DIALOG_MUTED)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(" \u{1F50D} ", Style::default().fg(DIALOG_ACCENT)),
                    Span::styled(self.state.filter.clone(), Style::default().fg(DIALOG_TEXT)),
                    Span::styled("\u{2588}", Style::default().fg(Color::White)),
                ])
            };
            buf.set_line(search_area.x, search_area.y, &search_line, search_area.width);
        }

        // ── Body: two-column layout ──────────────────────────────────────────
        let body = layout.body_area;
        if body.height == 0 || body.width < 10 {
            return;
        }

        let col_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(42),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(body);

        // Left: shortcuts, Center: divider, Right: commands.
        let left_lines = build_shortcut_lines(self.shortcuts, &self.state.filter);
        Paragraph::new(left_lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(PANEL_BG))
            .render(col_chunks[0], buf);

        render_vertical_divider(buf, col_chunks[1]);

        let (right_lines, cmd_count) = build_command_lines(self.commands, &self.state.filter);
        Paragraph::new(right_lines)
            .scroll((self.state.scroll_offset, 0))
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(PANEL_BG))
            .render(col_chunks[2], buf);

        // ── Footer ───────────────────────────────────────────────────────────
        let footer = layout.footer_area;
        if footer.height > 0 {
            let footer_line = Line::from(vec![
                Span::styled(
                    " Esc ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" close  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(
                    " ↑↓ ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" scroll  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(
                    format!(" {cmd_count} commands "),
                    Style::default().fg(DIALOG_MUTED),
                ),
            ]);
            buf.set_line(footer.x, footer.y, &footer_line, footer.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_state_toggle() {
        let mut s = HelpOverlayState::new();
        assert!(!s.is_visible());
        s.toggle();
        assert!(s.is_visible());
        s.push_filter_char('h');
        assert_eq!(s.filter(), "h");
        s.toggle(); // close → resets filter
        assert!(!s.is_visible());
        assert!(s.filter().is_empty());
    }

    #[test]
    fn help_state_scroll() {
        let mut s = HelpOverlayState::new();
        s.scroll_down(10);
        assert_eq!(s.scroll_offset(), 1);
        s.scroll_down(10);
        assert_eq!(s.scroll_offset(), 2);
        s.scroll_up();
        assert_eq!(s.scroll_offset(), 1);
        s.scroll_up();
        assert_eq!(s.scroll_offset(), 0);
        s.scroll_up(); // saturates at 0
        assert_eq!(s.scroll_offset(), 0);
    }

    #[test]
    fn help_state_filter_resets_scroll() {
        let mut s = HelpOverlayState::new();
        s.scroll_down(10);
        s.scroll_down(10);
        assert_eq!(s.scroll_offset(), 2);
        s.push_filter_char('x');
        assert_eq!(s.scroll_offset(), 0); // reset on filter change
    }

    #[test]
    fn default_shortcut_entries_non_empty() {
        let entries = default_shortcut_entries();
        assert!(entries.len() >= 10, "should have at least 10 shortcuts");
    }

    #[test]
    fn help_overlay_renders_without_panic() {
        let state = HelpOverlayState { visible: true, filter: String::new(), scroll_offset: 0 };
        let shortcuts = default_shortcut_entries();
        let commands = vec![
            ("clear".into(), "Clear conversation".into(), "Session".into()),
            ("help".into(), "Show help".into(), "Session".into()),
            ("model".into(), "Switch model".into(), "Model".into()),
        ];
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        HelpOverlay::new(&state, &shortcuts, &commands).render(area, &mut buf);
        // Should not panic — basic smoke test.
    }

    #[test]
    fn help_overlay_skips_small_terminal() {
        let state = HelpOverlayState { visible: true, filter: String::new(), scroll_offset: 0 };
        let shortcuts = default_shortcut_entries();
        let commands = vec![];
        let area = Rect::new(0, 0, 20, 5); // too small
        let mut buf = Buffer::empty(area);
        HelpOverlay::new(&state, &shortcuts, &commands).render(area, &mut buf);
        // Should render nothing (area too small).
    }
}
