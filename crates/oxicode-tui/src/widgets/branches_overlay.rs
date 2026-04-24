//! Branches overlay — browse, switch, rename, and delete session branches.
//!
//! Triggered by `/branches` slash command or `Ctrl+B` keybinding. Shows a
//! centered modal listing all branches of the current `SessionTree` with an
//! ASCII tree structure showing parent→child relationships.
//!
//! ## Interaction modes
//! - **Browse**: navigate with ↑↓, `Enter` switches branch, `d` deletes,
//!   `r` renames, `Esc` closes.
//! - **Rename**: inline edit for the selected branch title.
//! - **Confirm**: y/n confirmation for destructive delete.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use uuid::Uuid;

use super::modal_helpers::{
    begin_modal, render_modal_title, render_separator, DIALOG_MUTED, DIALOG_TEXT, PANEL_BG,
};

// ── Types ────────────────────────────────────────────────────────────────────

/// Interaction mode of the branches overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchesOverlayMode {
    /// Default: navigate the list with ↑↓.
    Browse,
    /// User is typing a new name for the selected branch.
    Rename,
    /// Awaiting y/n confirmation before deleting a branch.
    Confirm,
}

/// A single branch entry shown in the overlay.
#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub id: Uuid,
    /// Whether this is the active branch.
    pub is_current: bool,
    /// Human-readable label.
    pub title: String,
    /// Number of messages in this branch.
    pub message_count: usize,
    /// Turn index of parent at fork point (None for root).
    pub parent_turn: Option<u32>,
    /// Parent branch id (None for root).
    pub parent: Option<Uuid>,
    /// Indentation depth for ASCII tree rendering.
    pub depth: usize,
}

// ── State ────────────────────────────────────────────────────────────────────

/// Branches overlay state managed by `App`.
pub struct BranchesOverlayState {
    visible: bool,
    selected_idx: usize,
    entries: Vec<BranchEntry>,
    mode: BranchesOverlayMode,
    /// Buffer used during `Rename` mode.
    rename_input: String,
}

impl BranchesOverlayState {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected_idx: 0,
            entries: Vec::new(),
            mode: BranchesOverlayMode::Browse,
            rename_input: String::new(),
        }
    }

    /// Open the overlay with a pre-built list of `BranchEntry` items.
    pub fn open(&mut self, entries: Vec<BranchEntry>) {
        self.entries = entries;
        // Default selection to the current (active) branch.
        self.selected_idx = self
            .entries
            .iter()
            .position(|e| e.is_current)
            .unwrap_or(0);
        self.mode = BranchesOverlayMode::Browse;
        self.rename_input.clear();
        self.visible = true;
    }

    /// Close the overlay.
    pub fn close(&mut self) {
        self.visible = false;
        self.mode = BranchesOverlayMode::Browse;
        self.rename_input.clear();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn mode(&self) -> &BranchesOverlayMode {
        &self.mode
    }

    pub fn selected_entry(&self) -> Option<&BranchEntry> {
        self.entries.get(self.selected_idx)
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    pub fn select_prev(&mut self) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        if self.selected_idx == 0 {
            self.selected_idx = n - 1;
        } else {
            self.selected_idx -= 1;
        }
    }

    pub fn select_next(&mut self) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % n;
    }

    // ── Actions ───────────────────────────────────────────────────────────────

    /// Confirm switching to the selected branch. Returns `branch_id` and closes.
    pub fn confirm_switch(&mut self) -> Option<Uuid> {
        if self.mode != BranchesOverlayMode::Browse {
            return None;
        }
        let entry = self.entries.get(self.selected_idx)?;
        if entry.is_current {
            // Already on this branch — just close.
            self.close();
            return None;
        }
        let id = entry.id;
        self.close();
        Some(id)
    }

    /// Enter rename mode, pre-filling with the current title.
    pub fn start_rename(&mut self) {
        if let Some(entry) = self.entries.get(self.selected_idx) {
            self.rename_input = entry.title.clone();
            self.mode = BranchesOverlayMode::Rename;
        }
    }

    pub fn push_rename_char(&mut self, c: char) {
        if self.mode == BranchesOverlayMode::Rename {
            self.rename_input.push(c);
        }
    }

    pub fn pop_rename_char(&mut self) {
        if self.mode == BranchesOverlayMode::Rename {
            self.rename_input.pop();
        }
    }

    /// Confirm rename. Returns `(branch_id, new_title)` on success.
    pub fn confirm_rename(&mut self) -> Option<(Uuid, String)> {
        if self.mode != BranchesOverlayMode::Rename {
            return None;
        }
        let new_title = self.rename_input.trim().to_string();
        if new_title.is_empty() {
            return None;
        }
        let entry = self.entries.get_mut(self.selected_idx)?;
        let id = entry.id;
        entry.title.clone_from(&new_title);
        self.mode = BranchesOverlayMode::Browse;
        self.rename_input.clear();
        Some((id, new_title))
    }

    /// Enter confirm mode to prompt for deletion.
    pub fn start_delete_confirm(&mut self) {
        if let Some(entry) = self.entries.get(self.selected_idx) {
            if !entry.is_current {
                self.mode = BranchesOverlayMode::Confirm;
            }
        }
    }

    /// Confirm deletion. Returns `branch_id` when confirmed. Switches back to Browse.
    pub fn confirm_delete(&mut self) -> Option<Uuid> {
        if self.mode != BranchesOverlayMode::Confirm {
            return None;
        }
        let entry = self.entries.get(self.selected_idx)?;
        if entry.is_current {
            self.mode = BranchesOverlayMode::Browse;
            return None;
        }
        let id = entry.id;
        // Remove from local list; persistence is handled by the caller.
        self.entries.remove(self.selected_idx);
        if self.selected_idx >= self.entries.len() && !self.entries.is_empty() {
            self.selected_idx = self.entries.len() - 1;
        }
        self.mode = BranchesOverlayMode::Browse;
        Some(id)
    }

    /// Cancel current mode: from Rename/Confirm go to Browse; from Browse close.
    pub fn cancel(&mut self) {
        match self.mode {
            BranchesOverlayMode::Browse => self.close(),
            BranchesOverlayMode::Rename | BranchesOverlayMode::Confirm => {
                self.mode = BranchesOverlayMode::Browse;
                self.rename_input.clear();
            }
        }
    }
}

impl Default for BranchesOverlayState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Widget ───────────────────────────────────────────────────────────────────

/// Rendered branches overlay widget (centered modal on top of the TUI).
pub struct BranchesOverlay<'a> {
    state: &'a BranchesOverlayState,
}

impl<'a> BranchesOverlay<'a> {
    pub fn new(state: &'a BranchesOverlayState) -> Self {
        Self { state }
    }
}

impl Widget for BranchesOverlay<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible || area.height < 8 || area.width < 30 {
            return;
        }

        let dialog_w = 72.min(area.width.saturating_sub(4));
        let entry_count = self.state.entries.len() as u16;
        let footer_h: u16 = if self.state.mode == BranchesOverlayMode::Rename {
            2
        } else {
            1
        };
        let content_h = (entry_count + 6).clamp(8, 20);
        let dialog_h = content_h.min(area.height.saturating_sub(4));
        let layout = begin_modal(buf, area, dialog_w, dialog_h, 2, footer_h);

        // ── Header ────────────────────────────────────────────────────────────
        render_modal_title(
            buf,
            layout.header_area,
            &format!("Branches ({} total)", self.state.entries.len()),
            "esc",
        );
        if layout.header_area.height >= 2 {
            let sep_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 1,
                width: layout.header_area.width,
                height: 1,
            };
            render_separator(buf, sep_area);
        }

        // ── Body: branch list ─────────────────────────────────────────────────
        let body = layout.body_area;
        if body.height == 0 {
            return;
        }

        let lines = build_branch_lines(self.state, body.width);

        // Auto-scroll to keep selected item visible.
        let selected_line = self.state.selected_idx as u16;
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

        // ── Footer ────────────────────────────────────────────────────────────
        let footer = layout.footer_area;
        if footer.height > 0 {
            if self.state.mode == BranchesOverlayMode::Rename && footer.height >= 2 {
                let rename_area = Rect {
                    x: footer.x,
                    y: footer.y,
                    width: footer.width,
                    height: 1,
                };
                render_rename_input(buf, rename_area, &self.state.rename_input);
            }

            let hint_y =
                if self.state.mode == BranchesOverlayMode::Rename && footer.height >= 2 {
                    footer.y + 1
                } else {
                    footer.y
                };
            let footer_line = build_footer_line(&self.state.mode);
            buf.set_line(footer.x, hint_y, &footer_line, footer.width);
        }
    }
}

// ── Rendering helpers ────────────────────────────────────────────────────────

fn build_branch_lines(state: &BranchesOverlayState, width: u16) -> Vec<Line<'static>> {
    let select_bg = Color::Rgb(40, 60, 100);
    let mut lines: Vec<Line<'static>> = Vec::new();

    if state.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            " No branches found.",
            Style::default().fg(DIALOG_MUTED),
        )));
        return lines;
    }

    let inner_w = width as usize;
    // Reserve space for tree prefix + short-id + msg count.
    let prefix_w = 4; // "  > " or "    "
    let id_w = 10;    // "[a1b2c3d4]"
    let msgs_w = 8;   // " 42 msgs"
    let title_w = inner_w.saturating_sub(prefix_w + id_w + msgs_w + 2).max(8);

    for (i, entry) in state.entries.iter().enumerate() {
        let is_selected = i == state.selected_idx;
        let bg = if is_selected { select_bg } else { PANEL_BG };
        let fg = if is_selected {
            Color::White
        } else {
            DIALOG_TEXT
        };

        let active_marker = if entry.is_current { "*" } else { " " };
        let selected_marker = if is_selected { ">" } else { " " };
        let tree_prefix = build_tree_prefix(entry.depth);
        let prefix = format!("{selected_marker}{active_marker}{tree_prefix}");

        let short_id: String = entry.id.to_string().chars().take(8).collect();
        let id_cell = format!("[{short_id}]");

        let fork_suffix = entry
            .parent_turn
            .map(|t| format!(" t{t}"))
            .unwrap_or_default();
        let title_cell = truncate(
            &format!("{}{}", entry.title, fork_suffix),
            title_w,
        );

        let msgs_cell = format!("{:>5} msgs", entry.message_count);

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
            Span::styled(prefix, meta_style),
            Span::styled(format!("{title_cell:<title_w$}"), title_style),
            Span::styled(" ", meta_style),
            Span::styled(id_cell, meta_style),
            Span::styled(msgs_cell, meta_style),
        ]));
    }

    lines
}

/// Build a tree-prefix string based on indentation depth.
fn build_tree_prefix(depth: usize) -> String {
    match depth {
        0 => String::new(),
        1 => "\u{2514}\u{2500} ".to_string(), // "└─ "
        _ => {
            let pad = "   ".repeat(depth - 1);
            format!("{pad}\u{2514}\u{2500} ")
        }
    }
}

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

fn build_footer_line(mode: &BranchesOverlayMode) -> Line<'static> {
    let key = Style::default()
        .fg(Color::Black)
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);
    let hint = Style::default().fg(DIALOG_MUTED);

    match mode {
        BranchesOverlayMode::Browse => Line::from(vec![
            Span::styled(" Enter ", key),
            Span::styled(" switch  ", hint),
            Span::styled(" r ", key),
            Span::styled(" rename  ", hint),
            Span::styled(" d ", key),
            Span::styled(" delete  ", hint),
            Span::styled(" Esc ", key),
            Span::styled(" close", hint),
        ]),
        BranchesOverlayMode::Rename => Line::from(vec![
            Span::styled(" Enter ", key),
            Span::styled(" confirm  ", hint),
            Span::styled(" Esc ", key),
            Span::styled(" cancel", hint),
        ]),
        BranchesOverlayMode::Confirm => Line::from(vec![
            Span::styled(" y ", key),
            Span::styled(" delete  ", hint),
            Span::styled(" n/Esc ", key),
            Span::styled(" cancel", hint),
        ]),
    }
}

fn truncate(s: &str, max_w: usize) -> String {
    let count = s.chars().count();
    if count <= max_w {
        return s.to_string();
    }
    if max_w <= 1 {
        return "\u{2026}".to_string();
    }
    let t: String = s.chars().take(max_w - 1).collect();
    format!("{t}\u{2026}")
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<BranchEntry> {
        let root_id = Uuid::new_v4();
        let child_id = Uuid::new_v4();
        vec![
            BranchEntry {
                id: root_id,
                is_current: true,
                title: "main".to_string(),
                message_count: 10,
                parent_turn: None,
                parent: None,
                depth: 0,
            },
            BranchEntry {
                id: child_id,
                is_current: false,
                title: "alt-approach".to_string(),
                message_count: 3,
                parent_turn: Some(5),
                parent: Some(root_id),
                depth: 1,
            },
        ]
    }

    #[test]
    fn new_starts_hidden() {
        let s = BranchesOverlayState::new();
        assert!(!s.is_visible());
        assert_eq!(s.entry_count(), 0);
        assert_eq!(*s.mode(), BranchesOverlayMode::Browse);
    }

    #[test]
    fn open_selects_current_branch() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        assert!(s.is_visible());
        // Current branch is index 0 in sample_entries().
        assert_eq!(s.selected_idx, 0);
    }

    #[test]
    fn select_next_and_prev_wrap() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        s.select_next();
        assert_eq!(s.selected_idx, 1);
        s.select_next();
        assert_eq!(s.selected_idx, 0); // wrap
        s.select_prev();
        assert_eq!(s.selected_idx, 1); // wrap
    }

    #[test]
    fn confirm_switch_returns_id_for_non_current() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        s.select_next(); // select child (not current)
        let id = s.confirm_switch();
        assert!(id.is_some());
        assert!(!s.is_visible()); // closed
    }

    #[test]
    fn confirm_switch_on_current_just_closes() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        // selected_idx = 0 = current branch
        let id = s.confirm_switch();
        assert!(id.is_none());
        assert!(!s.is_visible());
    }

    #[test]
    fn rename_mode_prefills_title() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        s.start_rename();
        assert_eq!(*s.mode(), BranchesOverlayMode::Rename);
        assert_eq!(s.rename_input, "main");
    }

    #[test]
    fn confirm_rename_empty_returns_none() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        s.start_rename();
        s.rename_input.clear();
        assert!(s.confirm_rename().is_none());
    }

    #[test]
    fn confirm_rename_returns_id_and_title() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        s.start_rename();
        s.rename_input = "new-title".to_string();
        let result = s.confirm_rename();
        assert!(result.is_some());
        let (_, title) = result.unwrap();
        assert_eq!(title, "new-title");
        assert_eq!(*s.mode(), BranchesOverlayMode::Browse);
    }

    #[test]
    fn delete_confirm_removes_entry_and_returns_id() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        s.select_next(); // select child (not current)
        s.start_delete_confirm();
        assert_eq!(*s.mode(), BranchesOverlayMode::Confirm);
        let id = s.confirm_delete();
        assert!(id.is_some());
        assert_eq!(s.entry_count(), 1); // child removed
        assert_eq!(*s.mode(), BranchesOverlayMode::Browse);
    }

    #[test]
    fn delete_confirm_on_current_is_noop() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        // selected = 0 = current; start_delete_confirm should not enter Confirm.
        s.start_delete_confirm();
        assert_eq!(*s.mode(), BranchesOverlayMode::Browse);
    }

    #[test]
    fn cancel_from_rename_goes_to_browse() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        s.start_rename();
        s.cancel();
        assert_eq!(*s.mode(), BranchesOverlayMode::Browse);
        assert!(s.is_visible());
    }

    #[test]
    fn cancel_from_browse_closes() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        s.cancel();
        assert!(!s.is_visible());
    }

    #[test]
    fn widget_renders_without_panic() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        BranchesOverlay::new(&s).render(area, &mut buf);
    }

    #[test]
    fn widget_skips_small_terminal() {
        let mut s = BranchesOverlayState::new();
        s.open(sample_entries());
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        BranchesOverlay::new(&s).render(area, &mut buf);
    }

    #[test]
    fn widget_handles_empty_entries() {
        let mut s = BranchesOverlayState::new();
        s.open(vec![]);
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        BranchesOverlay::new(&s).render(area, &mut buf);
    }
}
