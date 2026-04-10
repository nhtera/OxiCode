//! Session browser overlay — browse, resume, and rename saved sessions.
//!
//! Triggered by `/sessions` slash command or `F3` keybinding. Displays a
//! centered modal with a scrollable list of sessions. Supports three modes:
//! Browse (navigate + resume), Rename (edit session title), Confirm (destructive ops).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::modal_helpers::{
    begin_modal, render_modal_title, render_separator, DIALOG_MUTED, DIALOG_TEXT,
    PANEL_BG,
};

// ── Types ───────────────────────────────────────────────────────────────────

/// The interaction mode of the session browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionBrowserMode {
    /// Default: list sessions, navigate with arrow keys.
    Browse,
    /// User is typing a new name for the selected session.
    Rename,
    /// Waiting for the user to confirm a destructive action.
    Confirm,
}

/// A single session entry shown in the browser list.
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub id: String,
    pub title: String,
    /// Human-readable relative time, e.g. "2 hours ago".
    pub last_updated: String,
    pub message_count: usize,
    /// Estimated USD cost for the session.
    pub cost_usd: f64,
}

// ── State ───────────────────────────────────────────────────────────────────

/// Session browser overlay state tracked by `App`.
pub struct SessionBrowserState {
    visible: bool,
    selected_idx: usize,
    sessions: Vec<SessionEntry>,
    mode: SessionBrowserMode,
    /// Input buffer used while in `Rename` mode.
    rename_input: String,
    /// Scroll offset for the session list.
    scroll_offset: u16,
}

impl SessionBrowserState {
    /// Create a new, hidden browser with an empty session list.
    pub fn new() -> Self {
        Self {
            visible: false,
            selected_idx: 0,
            sessions: Vec::new(),
            mode: SessionBrowserMode::Browse,
            rename_input: String::new(),
            scroll_offset: 0,
        }
    }

    /// Open the browser with the provided session list.
    pub fn open(&mut self, sessions: Vec<SessionEntry>) {
        self.sessions = sessions;
        self.selected_idx = 0;
        self.scroll_offset = 0;
        self.mode = SessionBrowserMode::Browse;
        self.rename_input.clear();
        self.visible = true;
    }

    /// Close the browser entirely.
    pub fn close(&mut self) {
        self.visible = false;
        self.mode = SessionBrowserMode::Browse;
        self.rename_input.clear();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn mode(&self) -> &SessionBrowserMode {
        &self.mode
    }

    /// Move selection up one row, wrapping to the end.
    pub fn select_prev(&mut self) {
        let count = self.sessions.len();
        if count == 0 {
            return;
        }
        if self.selected_idx == 0 {
            self.selected_idx = count - 1;
        } else {
            self.selected_idx -= 1;
        }
    }

    /// Move selection down one row, wrapping to the start.
    pub fn select_next(&mut self) {
        let count = self.sessions.len();
        if count == 0 {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % count;
    }

    /// Return a reference to the currently selected session, if any.
    pub fn selected_session(&self) -> Option<&SessionEntry> {
        self.sessions.get(self.selected_idx)
    }

    /// Confirm selection in Browse mode. Returns `session_id` and closes.
    pub fn confirm_resume(&mut self) -> Option<String> {
        if self.mode != SessionBrowserMode::Browse {
            return None;
        }
        let session_id = self.sessions.get(self.selected_idx)?.id.clone();
        self.close();
        Some(session_id)
    }

    /// Switch to rename mode, pre-populating the input with the current title.
    pub fn start_rename(&mut self) {
        if let Some(session) = self.sessions.get(self.selected_idx) {
            self.rename_input = session.title.clone();
            self.mode = SessionBrowserMode::Rename;
        }
    }

    /// Append a character to the rename input buffer.
    pub fn push_rename_char(&mut self, c: char) {
        if self.mode == SessionBrowserMode::Rename {
            self.rename_input.push(c);
        }
    }

    /// Remove the last character from the rename input buffer.
    pub fn pop_rename_char(&mut self) {
        if self.mode == SessionBrowserMode::Rename {
            self.rename_input.pop();
        }
    }

    /// Confirm the rename. Returns `(session_id, new_name)` when valid.
    /// Resets to browse mode.
    pub fn confirm_rename(&mut self) -> Option<(String, String)> {
        if self.mode != SessionBrowserMode::Rename {
            return None;
        }
        let new_name = self.rename_input.trim().to_string();
        if new_name.is_empty() {
            return None;
        }
        let session_id = self.sessions.get(self.selected_idx)?.id.clone();
        // Apply rename locally for immediate UI feedback.
        if let Some(session) = self.sessions.get_mut(self.selected_idx) {
            session.title.clone_from(&new_name);
        }
        self.mode = SessionBrowserMode::Browse;
        self.rename_input.clear();
        Some((session_id, new_name))
    }

    /// Cancel the current mode:
    /// - In `Rename` or `Confirm`: return to `Browse`.
    /// - In `Browse`: close the overlay.
    pub fn cancel(&mut self) {
        match self.mode {
            SessionBrowserMode::Browse => self.close(),
            SessionBrowserMode::Rename | SessionBrowserMode::Confirm => {
                self.mode = SessionBrowserMode::Browse;
                self.rename_input.clear();
            }
        }
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for SessionBrowserState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Widget ──────────────────────────────────────────────────────────────────

/// Session browser overlay widget — centered modal rendered on top of content.
pub struct SessionBrowser<'a> {
    state: &'a SessionBrowserState,
}

impl<'a> SessionBrowser<'a> {
    pub fn new(state: &'a SessionBrowserState) -> Self {
        Self { state }
    }
}

/// Format a cost as a dollar string with 4 decimal places.
fn fmt_cost(usd: f64) -> String {
    if usd < 0.0001 {
        "$0.00".to_string()
    } else {
        format!("${usd:.4}")
    }
}

/// Truncate a string to fit within `max_width` characters, appending `...` if cut.
fn truncate_str(s: &str, max_width: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_width {
        return s.to_string();
    }
    if max_width <= 3 {
        return "...".to_string();
    }
    let truncated: String = s.chars().take(max_width - 1).collect();
    format!("{truncated}\u{2026}")
}

/// Build session list lines for the body area.
#[allow(clippy::similar_names)]
fn build_session_lines(state: &SessionBrowserState, body_width: u16) -> Vec<Line<'static>> {
    let select_bg = Color::Rgb(40, 60, 100);
    let select_fg = Color::White;
    let mut lines: Vec<Line<'static>> = Vec::new();

    if state.sessions.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " No saved sessions",
            Style::default().fg(DIALOG_MUTED),
        )));
        return lines;
    }

    // Column widths (adapt to body width).
    let inner_w = body_width as usize;
    let date_w: usize = 14;
    let msgs_w: usize = 5;
    let cost_w: usize = 8;
    let fixed = date_w + msgs_w + cost_w + 8; // separators + padding
    let title_w = inner_w.saturating_sub(fixed).max(10);

    // Header row.
    lines.push(Line::from(vec![Span::styled(
        format!(
            " {:<title_w$}  {:<date_w$}  {:>msgs_w$}  {:>cost_w$}",
            "Session",
            "Last Updated",
            "Msgs",
            "Cost",
        ),
        Style::default()
            .fg(DIALOG_MUTED)
            .add_modifier(Modifier::UNDERLINED),
    )]));
    lines.push(Line::from(""));

    for (i, session) in state.sessions.iter().enumerate() {
        let is_selected = i == state.selected_idx;
        let (fg, bg) = if is_selected {
            (select_fg, select_bg)
        } else {
            (DIALOG_TEXT, PANEL_BG)
        };

        let title_cell = truncate_str(&session.title, title_w);
        let date_cell = truncate_str(&session.last_updated, date_w);
        let msgs_cell = format!("{:>msgs_w$}", session.message_count);
        let cost_cell = format!("{:>cost_w$}", fmt_cost(session.cost_usd));

        let title_style = Style::default()
            .fg(if is_selected { Color::Cyan } else { fg })
            .bg(bg)
            .add_modifier(if is_selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });

        let meta_style = Style::default()
            .fg(if is_selected {
                Color::Rgb(180, 200, 220)
            } else {
                DIALOG_MUTED
            })
            .bg(bg);

        lines.push(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(format!("{title_cell:<title_w$}"), title_style),
            Span::styled("  ", meta_style),
            Span::styled(format!("{date_cell:<date_w$}"), meta_style),
            Span::styled("  ", meta_style),
            Span::styled(msgs_cell, meta_style),
            Span::styled("  ", meta_style),
            Span::styled(cost_cell, meta_style),
        ]));
    }

    lines
}

/// Build the mode-sensitive footer line.
fn build_browser_footer(mode: &SessionBrowserMode) -> Vec<Line<'static>> {
    let key_style = Style::default()
        .fg(Color::Black)
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);

    match mode {
        SessionBrowserMode::Browse => {
            vec![Line::from(vec![
                Span::styled(" Enter ", key_style),
                Span::styled(" resume  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(" r ", key_style),
                Span::styled(" rename  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(" Esc ", key_style),
                Span::styled(" close", Style::default().fg(DIALOG_MUTED)),
            ])]
        }
        SessionBrowserMode::Rename => {
            vec![Line::from(vec![
                Span::styled(" Enter ", key_style),
                Span::styled(" confirm  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(" Esc ", key_style),
                Span::styled(" cancel", Style::default().fg(DIALOG_MUTED)),
            ])]
        }
        SessionBrowserMode::Confirm => {
            vec![Line::from(vec![
                Span::styled(" Enter ", key_style),
                Span::styled(" yes  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(" Esc ", key_style),
                Span::styled(" no", Style::default().fg(DIALOG_MUTED)),
            ])]
        }
    }
}

/// Render the rename input field into a body-area subregion.
fn render_rename_input(buf: &mut Buffer, area: Rect, rename_input: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let line = Line::from(vec![
        Span::styled(
            " Rename: ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            rename_input.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("\u{2588}", Style::default().fg(Color::White)),
    ]);
    buf.set_line(area.x, area.y, &line, area.width);
}

impl Widget for SessionBrowser<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible || area.height < 8 || area.width < 30 {
            return;
        }

        let dialog_w = 76.min(area.width.saturating_sub(4));
        let session_count = self.state.sessions.len() as u16;
        // header(1 title + 1 sep) = 2, footer = 1 or 2 (rename adds input line)
        let footer_h: u16 = if self.state.mode == SessionBrowserMode::Rename {
            2
        } else {
            1
        };
        let content_h = (session_count + 6).clamp(8, 22);
        let dialog_h = content_h.min(area.height.saturating_sub(4));
        let layout = begin_modal(buf, area, dialog_w, dialog_h, 2, footer_h);

        // ── Header ───────────────────────────────────────────────────────────
        let session_label = format!(
            "Sessions ({} total)",
            self.state.sessions.len()
        );
        render_modal_title(buf, layout.header_area, &session_label, "esc");
        if layout.header_area.height >= 2 {
            let sep_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 1,
                width: layout.header_area.width,
                height: 1,
            };
            render_separator(buf, sep_area);
        }

        // ── Body: session list ───────────────────────────────────────────────
        let body = layout.body_area;
        if body.height == 0 {
            return;
        }

        let lines = build_session_lines(self.state, body.width);

        // Auto-scroll to keep selected item visible.
        let selected_line = (self.state.selected_idx as u16) + 2; // +2 for header row + blank
        let scroll_y = if lines.len() as u16 <= body.height {
            0
        } else if selected_line + 2 >= body.height {
            (selected_line + 2).saturating_sub(body.height)
        } else {
            0
        };

        Paragraph::new(lines)
            .scroll((scroll_y, 0))
            .style(Style::default().bg(PANEL_BG))
            .render(body, buf);

        // ── Footer ───────────────────────────────────────────────────────────
        let footer = layout.footer_area;
        if footer.height > 0 {
            // Rename input line above the hint bar.
            if self.state.mode == SessionBrowserMode::Rename && footer.height >= 2 {
                let rename_area = Rect {
                    x: footer.x,
                    y: footer.y,
                    width: footer.width,
                    height: 1,
                };
                render_rename_input(buf, rename_area, &self.state.rename_input);
            }

            let footer_lines = build_browser_footer(&self.state.mode);
            let hint_y = if self.state.mode == SessionBrowserMode::Rename && footer.height >= 2 {
                footer.y + 1
            } else {
                footer.y
            };
            if let Some(first_line) = footer_lines.first() {
                buf.set_line(footer.x, hint_y, first_line, footer.width);
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_sessions() -> Vec<SessionEntry> {
        vec![
            SessionEntry {
                id: "sess-001".to_string(),
                title: "Refactor auth module".to_string(),
                last_updated: "2 hours ago".to_string(),
                message_count: 34,
                cost_usd: 0.0124,
            },
            SessionEntry {
                id: "sess-002".to_string(),
                title: "Write unit tests".to_string(),
                last_updated: "yesterday".to_string(),
                message_count: 12,
                cost_usd: 0.0045,
            },
            SessionEntry {
                id: "sess-003".to_string(),
                title: "Debug memory leak".to_string(),
                last_updated: "3 days ago".to_string(),
                message_count: 57,
                cost_usd: 0.0289,
            },
        ]
    }

    #[test]
    fn new_starts_hidden() {
        let s = SessionBrowserState::new();
        assert!(!s.is_visible());
        assert_eq!(s.session_count(), 0);
        assert_eq!(*s.mode(), SessionBrowserMode::Browse);
    }

    #[test]
    fn open_populates_and_shows() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        assert!(s.is_visible());
        assert_eq!(s.session_count(), 3);
        assert_eq!(s.selected_idx, 0);
    }

    #[test]
    fn select_next_wraps() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.select_next();
        assert_eq!(s.selected_idx, 1);
        s.select_next();
        assert_eq!(s.selected_idx, 2);
        s.select_next();
        assert_eq!(s.selected_idx, 0);
    }

    #[test]
    fn select_prev_wraps() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.select_prev();
        assert_eq!(s.selected_idx, 2);
    }

    #[test]
    fn selected_session_correct() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.selected_idx = 1;
        let sess = s.selected_session().unwrap();
        assert_eq!(sess.id, "sess-002");
    }

    #[test]
    fn confirm_resume_returns_id_and_closes() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.selected_idx = 1;
        let result = s.confirm_resume();
        assert_eq!(result, Some("sess-002".to_string()));
        assert!(!s.is_visible());
    }

    #[test]
    fn start_rename_prefills_title() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.selected_idx = 0;
        s.start_rename();
        assert_eq!(*s.mode(), SessionBrowserMode::Rename);
        assert_eq!(s.rename_input, "Refactor auth module");
    }

    #[test]
    fn rename_char_editing() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.start_rename();
        s.rename_input.clear();
        s.push_rename_char('H');
        s.push_rename_char('i');
        assert_eq!(s.rename_input, "Hi");
        s.pop_rename_char();
        assert_eq!(s.rename_input, "H");
    }

    #[test]
    fn confirm_rename_returns_pair() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.selected_idx = 0;
        s.start_rename();
        s.rename_input = "  New Title  ".to_string();
        let result = s.confirm_rename();
        assert_eq!(
            result,
            Some(("sess-001".to_string(), "New Title".to_string()))
        );
        assert_eq!(*s.mode(), SessionBrowserMode::Browse);
        assert!(s.rename_input.is_empty());
        assert_eq!(s.sessions[0].title, "New Title");
    }

    #[test]
    fn confirm_rename_empty_returns_none() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.start_rename();
        s.rename_input = "   ".to_string();
        let result = s.confirm_rename();
        assert!(result.is_none());
    }

    #[test]
    fn cancel_rename_goes_to_browse() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.start_rename();
        s.cancel();
        assert_eq!(*s.mode(), SessionBrowserMode::Browse);
        assert!(s.is_visible());
    }

    #[test]
    fn cancel_browse_closes() {
        let mut s = SessionBrowserState::new();
        s.open(sample_sessions());
        s.cancel();
        assert!(!s.is_visible());
    }

    #[test]
    fn fmt_cost_formats() {
        assert_eq!(fmt_cost(0.0), "$0.00");
        assert_eq!(fmt_cost(0.0124), "$0.0124");
        assert_eq!(fmt_cost(1.5), "$1.5000");
    }

    #[test]
    fn truncate_str_trims() {
        let long = "abcdefghij";
        let result = truncate_str(long, 5);
        assert!(result.chars().count() <= 5);
        assert!(result.ends_with('\u{2026}'));
    }

    #[test]
    fn truncate_str_short_passthrough() {
        assert_eq!(truncate_str("abc", 10), "abc");
    }

    #[test]
    fn session_browser_renders_without_panic() {
        let mut state = SessionBrowserState::new();
        state.open(sample_sessions());
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        SessionBrowser::new(&state).render(area, &mut buf);
    }

    #[test]
    fn session_browser_skips_small_terminal() {
        let mut state = SessionBrowserState::new();
        state.open(sample_sessions());
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        SessionBrowser::new(&state).render(area, &mut buf);
    }

    #[test]
    fn session_browser_empty_sessions() {
        let mut state = SessionBrowserState::new();
        state.open(vec![]);
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        SessionBrowser::new(&state).render(area, &mut buf);
    }

    #[test]
    fn select_on_empty_sessions_noop() {
        let mut s = SessionBrowserState::new();
        s.open(vec![]);
        s.select_next(); // should not panic
        s.select_prev(); // should not panic
        assert_eq!(s.selected_idx, 0);
    }

    #[test]
    fn confirm_resume_empty_returns_none() {
        let mut s = SessionBrowserState::new();
        s.open(vec![]);
        assert!(s.confirm_resume().is_none());
    }
}
