//! Rewind overlay — interactive turn selector for conversation rewind.
//!
//! Two-mode overlay: **Selecting** (browse turns with ↑↓) → **Confirming** (y/n).
//! Triggered by `/rewind` with no arguments. Follows the same modal pattern as
//! `SessionBrowser` and `ModelPicker`.

use oxicode_common::{ContentBlock, Message, Role};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::modal_helpers::{
    begin_modal, render_modal_title, render_separator, DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT,
    PANEL_BG,
};

// ── Types ───────────────────────────────────────────────────────────────────

/// Overlay interaction mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewindOverlayMode {
    /// User is browsing turns with ↑↓.
    Selecting,
    /// User confirmed a selection, awaiting y/n.
    Confirming,
}

/// A single conversation turn shown in the selector list.
#[derive(Debug, Clone)]
pub struct TurnEntry {
    /// 1-based turn number (oldest = 1).
    pub turn_number: usize,
    /// First ~60 chars of the user message.
    pub user_preview: String,
    /// First ~60 chars of the assistant response.
    pub assistant_preview: String,
    /// Total messages in this turn (user + assistant + tool blocks).
    pub message_count: usize,
    /// Whether any tool_use blocks appeared in this turn.
    pub has_tool_use: bool,
}

// ── State ───────────────────────────────────────────────────────────────────

/// Rewind overlay state tracked by `App`.
pub struct RewindOverlayState {
    visible: bool,
    mode: RewindOverlayMode,
    selected_idx: usize,
    turns: Vec<TurnEntry>,
}

impl RewindOverlayState {
    pub fn new() -> Self {
        Self {
            visible: false,
            mode: RewindOverlayMode::Selecting,
            selected_idx: 0,
            turns: Vec::new(),
        }
    }

    /// Open the overlay, building the turn list from current messages.
    pub fn open(&mut self, messages: &[Message]) {
        self.turns = build_turns(messages);
        // Select the most recent turn (last in list = top of visual display).
        self.selected_idx = self.turns.len().saturating_sub(1);
        self.mode = RewindOverlayMode::Selecting;
        self.visible = !self.turns.is_empty();
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.mode = RewindOverlayMode::Selecting;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn mode(&self) -> &RewindOverlayMode {
        &self.mode
    }

    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    /// Move selection up (toward more recent turns in visual display).
    pub fn select_prev(&mut self) {
        let count = self.turns.len();
        if count == 0 {
            return;
        }
        if self.selected_idx == 0 {
            self.selected_idx = count - 1;
        } else {
            self.selected_idx -= 1;
        }
    }

    /// Move selection down (toward older turns in visual display).
    pub fn select_next(&mut self) {
        let count = self.turns.len();
        if count == 0 {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % count;
    }

    /// Transition from Selecting → Confirming.
    pub fn begin_confirm(&mut self) {
        if self.mode == RewindOverlayMode::Selecting && !self.turns.is_empty() {
            self.mode = RewindOverlayMode::Confirming;
        }
    }

    /// Confirm the rewind. Returns the number of turns to remove (from the end).
    /// The selected turn is the one we rewind **to** (kept) — everything after it
    /// is removed.
    pub fn confirm_rewind(&mut self) -> Option<usize> {
        if self.mode != RewindOverlayMode::Confirming {
            return None;
        }
        let total = self.turns.len();
        if total == 0 {
            return None;
        }
        // selected_idx indexes into `turns` which is ordered oldest-first.
        // We keep turns 0..=selected_idx, remove turns (selected_idx+1)..total.
        let turns_to_remove = total - self.selected_idx - 1;
        self.close();
        if turns_to_remove == 0 {
            return None; // Nothing to remove — selected the last turn.
        }
        Some(turns_to_remove)
    }

    /// Cancel: Confirming → Selecting, Selecting → close.
    pub fn cancel(&mut self) {
        match self.mode {
            RewindOverlayMode::Selecting => self.close(),
            RewindOverlayMode::Confirming => {
                self.mode = RewindOverlayMode::Selecting;
            }
        }
    }

    /// Message count that would be removed if rewind is confirmed.
    /// Selected turn is kept — only turns after it are removed.
    pub fn messages_to_remove(&self) -> usize {
        let total = self.turns.len();
        if total == 0 || self.selected_idx + 1 >= total {
            return 0;
        }
        self.turns[self.selected_idx + 1..]
            .iter()
            .map(|t| t.message_count)
            .sum()
    }

    /// Number of turns that would be removed (everything after the selected turn).
    pub fn turns_to_remove(&self) -> usize {
        self.turns.len().saturating_sub(self.selected_idx + 1)
    }

    /// The selected turn entry, if any.
    pub fn selected_turn(&self) -> Option<&TurnEntry> {
        self.turns.get(self.selected_idx)
    }
}

impl Default for RewindOverlayState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Turn builder ────────────────────────────────────────────────────────────

/// Build a list of turns from the message history.
/// A turn starts at each `Role::User` message and includes all subsequent
/// assistant/tool messages until the next user message.
fn build_turns(messages: &[Message]) -> Vec<TurnEntry> {
    let mut turns: Vec<TurnEntry> = Vec::new();
    let mut current_user_preview = String::new();
    let mut current_assistant_preview = String::new();
    let mut current_msg_count: usize = 0;
    let mut current_has_tool = false;
    let mut in_turn = false;

    for msg in messages {
        if msg.role == Role::User {
            // Flush previous turn if any.
            if in_turn {
                turns.push(TurnEntry {
                    turn_number: turns.len() + 1,
                    user_preview: current_user_preview.clone(),
                    assistant_preview: current_assistant_preview.clone(),
                    message_count: current_msg_count,
                    has_tool_use: current_has_tool,
                });
            }
            // Start new turn.
            current_user_preview = extract_preview(&msg.content, 60);
            current_assistant_preview = String::new();
            current_msg_count = 1;
            current_has_tool = false;
            in_turn = true;
        } else if in_turn {
            current_msg_count += 1;
            // Capture first assistant text preview.
            if msg.role == Role::Assistant && current_assistant_preview.is_empty() {
                current_assistant_preview = extract_preview(&msg.content, 60);
            }
            // Detect tool use.
            if msg
                .content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
            {
                current_has_tool = true;
            }
        }
    }

    // Flush last turn.
    if in_turn {
        turns.push(TurnEntry {
            turn_number: turns.len() + 1,
            user_preview: current_user_preview,
            assistant_preview: current_assistant_preview,
            message_count: current_msg_count,
            has_tool_use: current_has_tool,
        });
    }

    turns
}

/// Extract a text preview from content blocks, truncated to `max_chars`.
fn extract_preview(content: &[ContentBlock], max_chars: usize) -> String {
    for block in content {
        if let ContentBlock::Text { text } = block {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            return truncate_str(trimmed, max_chars);
        }
    }
    String::new()
}

/// Truncate a string, appending … if cut.
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

// ── Widget ──────────────────────────────────────────────────────────────────

/// Rewind overlay widget — centered modal rendered on top of content.
pub struct RewindOverlay<'a> {
    state: &'a RewindOverlayState,
}

impl<'a> RewindOverlay<'a> {
    pub fn new(state: &'a RewindOverlayState) -> Self {
        Self { state }
    }
}

/// Build the turn list lines for Selecting mode.
fn build_turn_lines(state: &RewindOverlayState, body_width: u16) -> Vec<Line<'static>> {
    let select_bg = Color::Rgb(40, 60, 100);
    let select_fg = Color::White;
    let mut lines: Vec<Line<'static>> = Vec::new();

    if state.turns.is_empty() {
        lines.push(Line::from(Span::styled(
            " No turns to rewind",
            Style::default().fg(DIALOG_MUTED),
        )));
        return lines;
    }

    let inner_w = body_width as usize;

    // Iterate in reverse so most recent turn appears at the top.
    for (visual_idx, turn) in state.turns.iter().rev().enumerate() {
        let real_idx = state.turns.len() - 1 - visual_idx;
        let is_selected = real_idx == state.selected_idx;
        let (fg, bg) = if is_selected {
            (select_fg, select_bg)
        } else {
            (DIALOG_TEXT, PANEL_BG)
        };

        // Turn number + tool indicator.
        let tool_tag = if turn.has_tool_use { " [tools]" } else { "" };
        let turn_label = format!("Turn {}{tool_tag}", turn.turn_number);

        // User preview (truncated to available width).
        let prefix_len = turn_label.len() + 4; // " ▸ " or "   " + turn_label + "  "
        let preview_max = inner_w.saturating_sub(prefix_len).max(10);
        let user_text = if turn.user_preview.is_empty() {
            "(empty)".to_string()
        } else {
            truncate_str(&turn.user_preview, preview_max)
        };

        let pointer = if is_selected { " \u{25B8} " } else { "   " };
        let turn_style = Style::default()
            .fg(if is_selected {
                DIALOG_ACCENT
            } else {
                DIALOG_MUTED
            })
            .bg(bg)
            .add_modifier(if is_selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let text_style = Style::default().fg(fg).bg(bg);

        lines.push(Line::from(vec![
            Span::styled(pointer.to_string(), text_style),
            Span::styled(format!("{turn_label:<12}"), turn_style),
            Span::styled(format!(" {user_text}"), text_style),
        ]));
    }

    lines
}

/// Build footer hints based on current mode.
fn build_rewind_footer(mode: &RewindOverlayMode) -> Vec<Line<'static>> {
    let key_style = Style::default()
        .fg(Color::Black)
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);

    match mode {
        RewindOverlayMode::Selecting => {
            vec![Line::from(vec![
                Span::styled(" Enter ", key_style),
                Span::styled(" select  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(" \u{2191}\u{2193} ", key_style),
                Span::styled(" navigate  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(" Esc ", key_style),
                Span::styled(" close", Style::default().fg(DIALOG_MUTED)),
            ])]
        }
        RewindOverlayMode::Confirming => {
            vec![Line::from(vec![
                Span::styled(" y ", key_style),
                Span::styled(" confirm  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(" n ", key_style),
                Span::styled(" cancel  ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(" Esc ", key_style),
                Span::styled(" back", Style::default().fg(DIALOG_MUTED)),
            ])]
        }
    }
}

impl Widget for RewindOverlay<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible || area.height < 8 || area.width < 30 {
            return;
        }

        let dialog_w = 72.min(area.width.saturating_sub(4));
        let turn_count = self.state.turns.len() as u16;

        match self.state.mode {
            RewindOverlayMode::Selecting => {
                // header(title + separator) = 2, footer = 1
                let content_h = (turn_count + 4).clamp(6, 20);
                let dialog_h = content_h.min(area.height.saturating_sub(4));
                let layout = begin_modal(buf, area, dialog_w, dialog_h, 2, 1);

                // Header.
                let title = format!("Rewind ({} turns)", self.state.turns.len());
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

                // Body: turn list.
                let body = layout.body_area;
                if body.height > 0 {
                    let lines = build_turn_lines(self.state, body.width);
                    // Auto-scroll to keep selected item visible.
                    let visual_selected = self
                        .state
                        .turns
                        .len()
                        .saturating_sub(1 + self.state.selected_idx);
                    let scroll_y = if lines.len() as u16 <= body.height {
                        0
                    } else if visual_selected as u16 + 2 >= body.height {
                        (visual_selected as u16 + 2).saturating_sub(body.height)
                    } else {
                        0
                    };
                    Paragraph::new(lines)
                        .scroll((scroll_y, 0))
                        .style(Style::default().bg(PANEL_BG))
                        .render(body, buf);
                }

                // Footer.
                let footer = layout.footer_area;
                if footer.height > 0 {
                    let footer_lines = build_rewind_footer(&self.state.mode);
                    if let Some(first) = footer_lines.first() {
                        buf.set_line(footer.x, footer.y, first, footer.width);
                    }
                }
            }
            RewindOverlayMode::Confirming => {
                let dialog_h = 8.min(area.height.saturating_sub(4));
                let layout = begin_modal(buf, area, dialog_w, dialog_h, 2, 1);

                // Header.
                render_modal_title(buf, layout.header_area, "Confirm Rewind", "esc");
                if layout.header_area.height >= 2 {
                    let sep_area = Rect {
                        x: layout.header_area.x,
                        y: layout.header_area.y + 1,
                        width: layout.header_area.width,
                        height: 1,
                    };
                    render_separator(buf, sep_area);
                }

                // Body: confirmation text.
                let body = layout.body_area;
                if body.height > 0 {
                    let turns_rm = self.state.turns_to_remove();
                    let msgs_rm = self.state.messages_to_remove();
                    let target = self
                        .state
                        .selected_turn()
                        .map(|t| t.turn_number)
                        .unwrap_or(0);
                    let lines = vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            format!("  Rewind to Turn {target}?"),
                            Style::default()
                                .fg(DIALOG_TEXT)
                                .add_modifier(Modifier::BOLD),
                        )),
                        Line::from(Span::styled(
                            format!("  This will remove {turns_rm} turn(s) ({msgs_rm} messages)."),
                            Style::default().fg(DIALOG_MUTED),
                        )),
                    ];
                    Paragraph::new(lines)
                        .style(Style::default().bg(PANEL_BG))
                        .render(body, buf);
                }

                // Footer.
                let footer = layout.footer_area;
                if footer.height > 0 {
                    let footer_lines = build_rewind_footer(&self.state.mode);
                    if let Some(first) = footer_lines.first() {
                        buf.set_line(footer.x, footer.y, first, footer.width);
                    }
                }
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn user_msg(text: &str) -> Message {
        Message::user(text)
    }

    fn assistant_msg() -> Message {
        Message::assistant()
    }

    fn sample_messages() -> Vec<Message> {
        vec![
            user_msg("hello"),
            assistant_msg(),
            user_msg("read the config file"),
            assistant_msg(),
            user_msg("fix the auth bug in login.rs"),
            assistant_msg(),
        ]
    }

    #[test]
    fn new_starts_hidden() {
        let s = RewindOverlayState::new();
        assert!(!s.is_visible());
        assert_eq!(s.turn_count(), 0);
        assert_eq!(*s.mode(), RewindOverlayMode::Selecting);
    }

    #[test]
    fn open_populates_turns() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        assert!(s.is_visible());
        assert_eq!(s.turn_count(), 3);
        // Selected should be the most recent turn (index 2).
        assert_eq!(s.selected_idx, 2);
    }

    #[test]
    fn open_empty_stays_hidden() {
        let mut s = RewindOverlayState::new();
        s.open(&[]);
        assert!(!s.is_visible());
        assert_eq!(s.turn_count(), 0);
    }

    #[test]
    fn select_next_wraps() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.selected_idx = 2;
        s.select_next();
        assert_eq!(s.selected_idx, 0);
    }

    #[test]
    fn select_prev_wraps() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.selected_idx = 0;
        s.select_prev();
        assert_eq!(s.selected_idx, 2);
    }

    #[test]
    fn begin_confirm_transitions_mode() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.begin_confirm();
        assert_eq!(*s.mode(), RewindOverlayMode::Confirming);
    }

    #[test]
    fn confirm_rewind_returns_turns_count() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.selected_idx = 1; // Rewind to turn 2 (keep turns 1-2, remove turn 3).
        s.begin_confirm();
        let result = s.confirm_rewind();
        assert_eq!(result, Some(1)); // 3 total - 1 selected_idx - 1 = 1 turn to remove
        assert!(!s.is_visible());
    }

    #[test]
    fn confirm_rewind_all() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.selected_idx = 0; // Rewind to turn 1 (keep turn 1, remove turns 2 and 3).
        s.begin_confirm();
        let result = s.confirm_rewind();
        assert_eq!(result, Some(2));
    }

    #[test]
    fn confirm_rewind_last_turn_returns_none() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.selected_idx = 2; // Select the last turn — nothing after it to remove.
        s.begin_confirm();
        let result = s.confirm_rewind();
        assert!(result.is_none()); // Nothing to remove.
    }

    #[test]
    fn cancel_confirming_returns_to_selecting() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.begin_confirm();
        s.cancel();
        assert_eq!(*s.mode(), RewindOverlayMode::Selecting);
        assert!(s.is_visible());
    }

    #[test]
    fn cancel_selecting_closes() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.cancel();
        assert!(!s.is_visible());
    }

    #[test]
    fn turns_to_remove_calculation() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.selected_idx = 1; // Rewind to turn 2: removes turn 3 only.
        assert_eq!(s.turns_to_remove(), 1);
    }

    #[test]
    fn messages_to_remove_calculation() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        // Each turn has 2 messages (user + assistant).
        // Selecting idx 1 (turn 2) → removes turn 3 only (2 messages).
        s.selected_idx = 1;
        assert_eq!(s.messages_to_remove(), 2);
    }

    #[test]
    fn build_turns_groups_correctly() {
        let turns = build_turns(&sample_messages());
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].turn_number, 1);
        assert_eq!(turns[0].user_preview, "hello");
        assert_eq!(turns[2].turn_number, 3);
        assert!(turns[2].user_preview.contains("fix the auth bug"));
    }

    #[test]
    fn build_turns_handles_tool_use() {
        let mut msgs = vec![
            user_msg("do something"),
            Message {
                id: "t1".to_string(),
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "bash".to_string(),
                    input: serde_json::json!({"command": "ls"}),
                }],
                model: None,
                stop_reason: None,
                created_at: chrono::Utc::now(),
                usage: None,
            },
            assistant_msg(),
        ];
        let turns = build_turns(&msgs);
        assert_eq!(turns.len(), 1);
        assert!(turns[0].has_tool_use);
        assert_eq!(turns[0].message_count, 3);

        // Without tool use.
        msgs = sample_messages();
        let turns = build_turns(&msgs);
        assert!(!turns[0].has_tool_use);
    }

    #[test]
    fn extract_preview_truncates() {
        let content = vec![ContentBlock::Text {
            text: "a".repeat(100),
        }];
        let preview = extract_preview(&content, 20);
        assert!(preview.chars().count() <= 20);
        assert!(preview.ends_with('\u{2026}'));
    }

    #[test]
    fn extract_preview_empty() {
        let preview = extract_preview(&[], 60);
        assert!(preview.is_empty());
    }

    #[test]
    fn selected_turn_returns_correct() {
        let mut s = RewindOverlayState::new();
        s.open(&sample_messages());
        s.selected_idx = 0;
        let turn = s.selected_turn().unwrap();
        assert_eq!(turn.turn_number, 1);
    }

    #[test]
    fn rewind_overlay_renders_without_panic() {
        let mut state = RewindOverlayState::new();
        state.open(&sample_messages());
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        RewindOverlay::new(&state).render(area, &mut buf);
    }

    #[test]
    fn rewind_overlay_confirming_renders_without_panic() {
        let mut state = RewindOverlayState::new();
        state.open(&sample_messages());
        state.begin_confirm();
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        RewindOverlay::new(&state).render(area, &mut buf);
    }

    #[test]
    fn rewind_overlay_skips_small_terminal() {
        let mut state = RewindOverlayState::new();
        state.open(&sample_messages());
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        RewindOverlay::new(&state).render(area, &mut buf);
    }
}
