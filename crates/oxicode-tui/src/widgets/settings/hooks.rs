//! Hooks readonly browser tab.
//!
//! Displays a tree of loaded hooks grouped by event type:
//!
//! ```text
//! PreToolUse
//!   ├─ [user ~/.oxicode]  matcher: Bash      command: ~/bin/log.sh  timeout: 10s
//!   └─ [project]          matcher: FileWrite  agent: check-pii       timeout: 60s
//! PostToolUse
//!   └─ [user ~/.oxicode]  matcher: .*         command: post.sh       timeout: 10s
//! ```
//!
//! Arrow keys navigate; Enter shows the source path in a hint line.
//! Editing is explicitly out of scope for v1 (readonly browser only).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::widgets::modal_helpers::{DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT};

/// Source scope label for a hook entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookSource {
    UserOxicode,
    UserClaude,
    Project,
}

impl HookSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::UserOxicode => "user ~/.oxicode",
            Self::UserClaude => "user ~/.claude",
            Self::Project => "project",
        }
    }
}

/// A single hook entry shown in the tree.
#[derive(Debug, Clone)]
pub struct HookEntry {
    pub source: HookSource,
    /// Regex matcher string (empty = ".*" / always).
    pub matcher: String,
    /// Human-readable hook description (command path, agent name, or URL).
    pub command_summary: String,
    pub timeout_secs: u64,
    /// Full path to the config file defining this hook (shown on Enter).
    pub source_path: String,
}

/// A node in the tree: event group + its child entries.
#[derive(Debug, Clone)]
pub struct HookGroup {
    pub event: String,
    pub entries: Vec<HookEntry>,
    /// Whether this group is expanded in the tree view.
    pub expanded: bool,
}

impl HookGroup {
    pub fn new(event: String, entries: Vec<HookEntry>) -> Self {
        Self {
            event,
            entries,
            expanded: true,
        }
    }
}

/// Represents one visible line in the rendered tree (for cursor tracking).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeLine {
    Group(usize),        // group index
    Entry(usize, usize), // (group index, entry index)
}

/// Hooks tab state — readonly tree view.
#[derive(Debug, Clone)]
pub struct HooksTab {
    pub groups: Vec<HookGroup>,
    /// Flat cursor index into the visible tree lines.
    pub cursor: usize,
    /// Source path shown in the hint area when Enter is pressed.
    pub hint_path: Option<String>,
}

impl HooksTab {
    pub fn new(groups: Vec<HookGroup>) -> Self {
        Self {
            groups,
            cursor: 0,
            hint_path: None,
        }
    }

    /// Build the flat list of currently visible tree lines (respects collapsed state).
    fn visible_lines(&self) -> Vec<TreeLine> {
        let mut lines = Vec::new();
        for (gi, group) in self.groups.iter().enumerate() {
            lines.push(TreeLine::Group(gi));
            if group.expanded {
                for ei in 0..group.entries.len() {
                    lines.push(TreeLine::Entry(gi, ei));
                }
            }
        }
        lines
    }

    pub fn select_next(&mut self) {
        let count = self.visible_lines().len();
        if count > 0 {
            self.cursor = (self.cursor + 1).min(count - 1);
        }
        self.hint_path = None;
    }

    pub fn select_prev(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.hint_path = None;
    }

    /// Toggle expand/collapse on group lines; show source path for entry lines.
    pub fn activate(&mut self) {
        let lines = self.visible_lines();
        let Some(&line) = lines.get(self.cursor) else {
            return;
        };
        match line {
            TreeLine::Group(gi) => {
                if let Some(g) = self.groups.get_mut(gi) {
                    g.expanded = !g.expanded;
                    // Clamp cursor so it doesn't sit on a now-hidden entry.
                    let new_count = self.visible_lines().len();
                    self.cursor = self.cursor.min(new_count.saturating_sub(1));
                }
            }
            TreeLine::Entry(gi, ei) => {
                if let Some(entry) = self.groups.get(gi).and_then(|g| g.entries.get(ei)) {
                    self.hint_path = Some(entry.source_path.clone());
                }
            }
        }
    }

    /// Render the tab body into `area`.
    pub fn render(&self, buf: &mut Buffer, area: Rect) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let lines = self.visible_lines();
        let footer_rows: u16 = if self.hint_path.is_some() { 2 } else { 1 };
        let body_height = area.height.saturating_sub(footer_rows);

        // Render tree body. Zipping against the row range bounds the loop to
        // the visible area, so it stops at whichever runs out first.
        for (flat_idx, (&tree_line, row)) in
            lines.iter().zip(area.y..area.y + body_height).enumerate()
        {
            let is_cursor = flat_idx == self.cursor;
            match tree_line {
                TreeLine::Group(gi) => {
                    let group = &self.groups[gi];
                    let chevron = if group.expanded { "▼ " } else { "▶ " };
                    let count_hint = format!(
                        "  ({} hook{})",
                        group.entries.len(),
                        if group.entries.len() == 1 { "" } else { "s" }
                    );
                    let cursor_ch = if is_cursor { "▶" } else { " " };
                    let line = Line::from(vec![
                        Span::styled(cursor_ch, Style::default().fg(DIALOG_ACCENT)),
                        Span::styled(" ", Style::default()),
                        Span::styled(chevron, Style::default().fg(DIALOG_ACCENT)),
                        Span::styled(
                            group.event.clone(),
                            Style::default()
                                .fg(DIALOG_TEXT)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(count_hint, Style::default().fg(DIALOG_MUTED)),
                    ]);
                    buf.set_line(area.x, row, &line, area.width);
                }
                TreeLine::Entry(gi, ei) => {
                    let entry = &self.groups[gi].entries[ei];
                    let is_last = ei == self.groups[gi].entries.len() - 1;
                    let branch = if is_last { "  └─ " } else { "  ├─ " };
                    let cursor_ch = if is_cursor { "▶" } else { " " };

                    let matcher_display = if entry.matcher.is_empty() || entry.matcher == ".*" {
                        ".*".to_string()
                    } else {
                        entry.matcher.clone()
                    };

                    let src_label = format!("[{}]", entry.source.label());
                    let timeout_label = format!(" {}s", entry.timeout_secs);

                    let line = Line::from(vec![
                        Span::styled(cursor_ch, Style::default().fg(DIALOG_ACCENT)),
                        Span::styled(branch, Style::default().fg(DIALOG_MUTED)),
                        Span::styled(
                            format!("{src_label:<17} "),
                            Style::default().fg(DIALOG_MUTED),
                        ),
                        Span::styled("matcher: ", Style::default().fg(DIALOG_MUTED)),
                        Span::styled(
                            format!("{matcher_display:<14} "),
                            Style::default().fg(DIALOG_TEXT),
                        ),
                        Span::styled(
                            format!("{} ", entry.command_summary),
                            if is_cursor {
                                Style::default()
                                    .fg(DIALOG_TEXT)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(DIALOG_TEXT)
                            },
                        ),
                        Span::styled(timeout_label, Style::default().fg(DIALOG_MUTED)),
                    ]);
                    buf.set_line(area.x, row, &line, area.width);
                }
            }
        }

        // Footer: show hint path when Enter was pressed on an entry.
        let footer_y = area.y + area.height - footer_rows;
        if let Some(ref path) = self.hint_path {
            let hint_line = Line::from(vec![
                Span::styled("  source: ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(path.clone(), Style::default().fg(DIALOG_ACCENT)),
            ]);
            buf.set_line(area.x, footer_y, &hint_line, area.width);
        }

        // Readonly notice.
        let notice_y = area.y + area.height - 1;
        let notice = Line::from(Span::styled(
            "  Readonly — hook editing is not available in v1",
            Style::default().fg(DIALOG_MUTED),
        ));
        buf.set_line(area.x, notice_y, &notice, area.width);
    }
}
