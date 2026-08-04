//! Help overlay: tabbed modal mirroring openclaude's `HelpV2` design.
//!
//! Three tabs: `general` (concept blurb + shortcuts grid), `commands` (built-in
//! slash commands), `custom-commands` (user-defined commands, future). Triggered
//! by `?`, `F1`, or `/help`. Tab/Shift+Tab cycles tabs; live search filters the
//! current tab.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::modal_helpers::{
    begin_modal, render_modal_title, render_separator, DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT,
    PANEL_BG,
};

/// Maximum number of visible rows in the body area before scrolling kicks in.
const MAX_BODY_ROWS: u16 = 28;

// ── Tab enum ─────────────────────────────────────────────────────────────────

/// Top-level tabs in the help overlay (matches openclaude `HelpV2` layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTab {
    General,
    Commands,
    CustomCommands,
}

impl HelpTab {
    pub fn label(self) -> &'static str {
        match self {
            HelpTab::General => "general",
            HelpTab::Commands => "commands",
            HelpTab::CustomCommands => "custom-commands",
        }
    }

    fn next(self) -> Self {
        match self {
            HelpTab::General => HelpTab::Commands,
            HelpTab::Commands => HelpTab::CustomCommands,
            HelpTab::CustomCommands => HelpTab::General,
        }
    }

    fn prev(self) -> Self {
        match self {
            HelpTab::General => HelpTab::CustomCommands,
            HelpTab::Commands => HelpTab::General,
            HelpTab::CustomCommands => HelpTab::Commands,
        }
    }
}

// ── State ────────────────────────────────────────────────────────────────────

/// Help overlay state tracked by `App`.
pub struct HelpOverlayState {
    visible: bool,
    /// Live search filter text (commands tab only).
    filter: String,
    /// Scroll offset for command lists.
    scroll_offset: u16,
    /// Currently selected tab.
    active_tab: HelpTab,
}

impl HelpOverlayState {
    pub fn new() -> Self {
        Self {
            visible: false,
            filter: String::new(),
            scroll_offset: 0,
            active_tab: HelpTab::General,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.filter.clear();
            self.scroll_offset = 0;
            self.active_tab = HelpTab::General;
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.filter.clear();
        self.scroll_offset = 0;
        self.active_tab = HelpTab::General;
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

    pub fn active_tab(&self) -> HelpTab {
        self.active_tab
    }

    pub fn next_tab(&mut self) {
        self.active_tab = self.active_tab.next();
        self.scroll_offset = 0;
        self.filter.clear();
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = self.active_tab.prev();
        self.scroll_offset = 0;
        self.filter.clear();
    }

    pub fn set_tab(&mut self, tab: HelpTab) {
        self.active_tab = tab;
        self.scroll_offset = 0;
        self.filter.clear();
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

/// Help overlay widget — tabbed modal rendered on top of content.
pub struct HelpOverlay<'a> {
    state: &'a HelpOverlayState,
    shortcuts: &'a [ShortcutEntry],
    /// Slash command entries: (name, description, category).
    commands: &'a [(String, String, String)],
    /// App version string shown in the title bar.
    version: &'a str,
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
            version: oxicode_common::constants::VERSION,
        }
    }

    pub fn with_version(mut self, version: &'a str) -> Self {
        self.version = version;
        self
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

// ── Tab strip ────────────────────────────────────────────────────────────────

fn render_tab_strip(buf: &mut Buffer, area: Rect, active: HelpTab) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
    for tab in [HelpTab::General, HelpTab::Commands, HelpTab::CustomCommands] {
        let label = format!(" {} ", tab.label());
        if tab == active {
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::Black)
                    .bg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                label,
                Style::default().fg(DIALOG_MUTED).bg(PANEL_BG),
            ));
        }
        spans.push(Span::raw(" "));
    }
    buf.set_line(area.x, area.y, &Line::from(spans), area.width);
}

// ── Body builders ────────────────────────────────────────────────────────────

/// Convert a slice of strings into styled text lines for the shortcuts grid.
fn make_lines(items: &[&str]) -> Vec<Line<'static>> {
    items
        .iter()
        .map(|s| {
            Line::from(Span::styled(
                (*s).to_string(),
                Style::default().fg(DIALOG_TEXT),
            ))
        })
        .collect()
}

/// Three columns of shortcuts shown on the general tab — mirrors openclaude's
/// `PromptInputHelpMenu` layout (input prefixes, edit shortcuts, app shortcuts).
const GENERAL_COL_PREFIXES: &[&str] = &[
    "! for bash mode",
    "/ for commands",
    "@ for file paths",
    "& for background",
    "/btw for side question",
];
const GENERAL_COL_EDITING: &[&str] = &[
    "double tap esc to clear input",
    "shift + tab to auto-accept edits",
    "ctrl + o for verbose output",
    "ctrl + t to toggle tasks",
    "shift + \u{23CE} for newline",
];
const GENERAL_COL_APP: &[&str] = &[
    "ctrl + shift + _ to undo",
    "ctrl + z to suspend",
    "ctrl + v to paste images",
    "alt + p to switch model",
    "alt + o to toggle fast mode",
    "ctrl + s to stash prompt",
    "ctrl + g to edit in $EDITOR",
    "/keybindings to customize",
];

/// General tab: blurb + 3-column shortcuts grid (matches openclaude PromptInputHelpMenu).
fn render_general_tab(buf: &mut Buffer, area: Rect) {
    if area.height == 0 || area.width < 10 {
        return;
    }

    let blurb = "OxiCode understands your codebase, makes edits with your permission, and executes commands — right from your terminal.";
    let blurb_height = ((blurb.len() as u16 / area.width.max(1)) + 1).min(3);
    let blurb_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: blurb_height,
    };
    Paragraph::new(Line::from(Span::styled(
        blurb,
        Style::default().fg(DIALOG_TEXT),
    )))
    .wrap(Wrap { trim: true })
    .style(Style::default().bg(PANEL_BG))
    .render(blurb_area, buf);

    let header_y = area.y + blurb_height + 1;
    if header_y >= area.y + area.height {
        return;
    }
    let header_area = Rect {
        x: area.x,
        y: header_y,
        width: area.width,
        height: 1,
    };
    buf.set_line(
        header_area.x,
        header_area.y,
        &Line::from(Span::styled(
            " Shortcuts",
            Style::default()
                .fg(DIALOG_ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        header_area.width,
    );

    let grid_y = header_y + 1;
    let avail_h = area.height.saturating_sub(grid_y - area.y);
    if avail_h == 0 {
        return;
    }
    let grid_area = Rect {
        x: area.x,
        y: grid_y,
        width: area.width,
        height: avail_h,
    };

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(28),
            Constraint::Length(36),
            Constraint::Min(20),
        ])
        .split(grid_area);

    let columns = [
        (layout[0], GENERAL_COL_PREFIXES),
        (layout[1], GENERAL_COL_EDITING),
        (layout[2], GENERAL_COL_APP),
    ];
    for (rect, items) in columns {
        Paragraph::new(make_lines(items))
            .style(Style::default().bg(PANEL_BG))
            .render(rect, buf);
    }
}

/// Commands tab: filtered list of slash commands with descriptions.
/// Returns the visible row count for the footer.
fn render_commands_tab(
    buf: &mut Buffer,
    area: Rect,
    commands: &[(String, String, String)],
    filter: &str,
    scroll_offset: u16,
    title: &str,
    empty_msg: &str,
) -> usize {
    if area.height == 0 || area.width < 10 {
        return 0;
    }

    // Title row.
    let title_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    buf.set_line(
        title_area.x,
        title_area.y,
        &Line::from(Span::styled(
            format!(" {title}"),
            Style::default()
                .fg(DIALOG_ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        title_area.width,
    );

    // Filter status line (only if filter is set).
    let next_y = if filter.is_empty() {
        area.y + 1
    } else {
        let filter_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: 1,
        };
        let line = Line::from(vec![
            Span::styled(" \u{1F50D} ", Style::default().fg(DIALOG_ACCENT)),
            Span::styled(filter.to_string(), Style::default().fg(DIALOG_TEXT)),
            Span::styled("\u{2588}", Style::default().fg(Color::White)),
        ]);
        buf.set_line(filter_area.x, filter_area.y, &line, filter_area.width);
        area.y + 2
    };

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
    if count == 0 {
        let empty_area = Rect {
            x: area.x,
            y: next_y,
            width: area.width,
            height: 1,
        };
        buf.set_line(
            empty_area.x,
            empty_area.y,
            &Line::from(Span::styled(
                format!(" {empty_msg}"),
                Style::default().fg(DIALOG_MUTED),
            )),
            empty_area.width,
        );
        return 0;
    }

    // Render command list as a scrollable Paragraph (one line per command).
    let mut sorted: Vec<&(String, String, String)> = filtered;
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let lines: Vec<Line<'static>> = sorted
        .iter()
        .map(|(name, desc, _)| {
            Line::from(vec![
                Span::styled(
                    format!(" /{name:<18}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(desc.clone(), Style::default().fg(DIALOG_TEXT)),
            ])
        })
        .collect();

    let list_area = Rect {
        x: area.x,
        y: next_y,
        width: area.width,
        height: area.height.saturating_sub(next_y - area.y),
    };
    Paragraph::new(lines)
        .scroll((scroll_offset, 0))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(PANEL_BG))
        .render(list_area, buf);

    count
}

impl Widget for HelpOverlay<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible || area.height < 8 || area.width < 30 {
            return;
        }

        // Modal dimensions — scale with terminal size.
        let dialog_w = 100.min(area.width.saturating_sub(4));
        let dialog_h = MAX_BODY_ROWS.min(area.height.saturating_sub(4));
        // header: title(1) + separator(1) + tabs(1) = 3, footer: hint(1) = 1
        let layout = begin_modal(buf, area, dialog_w, dialog_h, 3, 1);

        // ── Header ───────────────────────────────────────────────────────────
        let title = format!("OxiCode v{}", self.version);
        render_modal_title(buf, layout.header_area, &title, "esc");

        if layout.header_area.height >= 2 {
            let sep_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 1,
                width: layout.header_area.width,
                height: 1,
            };
            render_separator(buf, sep_area);
        }

        // Tab strip (third header row).
        if layout.header_area.height >= 3 {
            let tab_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 2,
                width: layout.header_area.width,
                height: 1,
            };
            render_tab_strip(buf, tab_area, self.state.active_tab);
        }

        // ── Body: dispatch by active tab ─────────────────────────────────────
        let body = layout.body_area;
        if body.height == 0 || body.width < 10 {
            return;
        }

        // Padded inner body for breathing room.
        let inner = Rect {
            x: body.x + 1,
            y: body.y,
            width: body.width.saturating_sub(2),
            height: body.height,
        };

        let visible_count = match self.state.active_tab {
            HelpTab::General => {
                render_general_tab(buf, inner);
                self.shortcuts.len()
            }
            HelpTab::Commands => render_commands_tab(
                buf,
                inner,
                self.commands,
                &self.state.filter,
                self.state.scroll_offset,
                "Browse default commands:",
                "No commands found",
            ),
            HelpTab::CustomCommands => render_commands_tab(
                buf,
                inner,
                &[],
                &self.state.filter,
                self.state.scroll_offset,
                "Browse custom commands:",
                "No custom commands found",
            ),
        };

        // ── Footer ───────────────────────────────────────────────────────────
        let footer = layout.footer_area;
        if footer.height > 0 {
            let footer_line = Line::from(vec![
                Span::styled(
                    " tab ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" switch tab  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(
                    " ↑↓ ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" scroll  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(
                    " esc ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" close  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(
                    format!(" {visible_count} items "),
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
        assert_eq!(s.active_tab(), HelpTab::General);
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
    fn help_state_tab_cycle() {
        let mut s = HelpOverlayState::new();
        assert_eq!(s.active_tab(), HelpTab::General);
        s.next_tab();
        assert_eq!(s.active_tab(), HelpTab::Commands);
        s.next_tab();
        assert_eq!(s.active_tab(), HelpTab::CustomCommands);
        s.next_tab();
        assert_eq!(s.active_tab(), HelpTab::General); // wraps
        s.prev_tab();
        assert_eq!(s.active_tab(), HelpTab::CustomCommands);
    }

    #[test]
    fn help_state_tab_change_resets_filter_and_scroll() {
        let mut s = HelpOverlayState::new();
        s.set_tab(HelpTab::Commands);
        s.push_filter_char('m');
        s.scroll_down(5);
        assert_eq!(s.filter(), "m");
        assert_eq!(s.scroll_offset(), 1);
        s.next_tab();
        assert!(s.filter().is_empty());
        assert_eq!(s.scroll_offset(), 0);
    }

    #[test]
    fn default_shortcut_entries_non_empty() {
        let entries = default_shortcut_entries();
        assert!(entries.len() >= 10, "should have at least 10 shortcuts");
    }

    #[test]
    fn help_overlay_renders_general_tab_without_panic() {
        let mut state = HelpOverlayState::new();
        state.toggle();
        let shortcuts = default_shortcut_entries();
        let commands = vec![
            (
                "clear".into(),
                "Clear conversation".into(),
                "Session".into(),
            ),
            ("help".into(), "Show help".into(), "Session".into()),
        ];
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        HelpOverlay::new(&state, &shortcuts, &commands).render(area, &mut buf);
    }

    #[test]
    fn help_overlay_renders_commands_tab_without_panic() {
        let mut state = HelpOverlayState::new();
        state.toggle();
        state.set_tab(HelpTab::Commands);
        let shortcuts = default_shortcut_entries();
        let commands = vec![
            (
                "clear".into(),
                "Clear conversation".into(),
                "Session".into(),
            ),
            ("model".into(), "Switch model".into(), "Model".into()),
        ];
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        HelpOverlay::new(&state, &shortcuts, &commands).render(area, &mut buf);
    }

    #[test]
    fn help_overlay_renders_custom_tab_with_empty_state() {
        let mut state = HelpOverlayState::new();
        state.toggle();
        state.set_tab(HelpTab::CustomCommands);
        let shortcuts = default_shortcut_entries();
        let commands: Vec<(String, String, String)> = vec![];
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        HelpOverlay::new(&state, &shortcuts, &commands).render(area, &mut buf);
    }

    #[test]
    fn help_overlay_skips_small_terminal() {
        let mut state = HelpOverlayState::new();
        state.toggle();
        let shortcuts = default_shortcut_entries();
        let commands = vec![];
        let area = Rect::new(0, 0, 20, 5); // too small
        let mut buf = Buffer::empty(area);
        HelpOverlay::new(&state, &shortcuts, &commands).render(area, &mut buf);
        // Should render nothing (area too small).
    }
}
