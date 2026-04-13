//! Stats dashboard overlay — 2-tab session statistics display.
//!
//! Tab 1 (Overview): session duration, total cost, tokens, messages, turns.
//! Tab 2 (Models): per-model breakdown table sorted by cost descending.
//!
//! Triggered by `/stats` or `/cost` (no args). Follows the same modal pattern
//! as `SessionBrowser`, `ModelPicker`, and `RewindOverlay`.

use std::time::Duration;

use oxicode_common::{Message, Role};
use oxicode_state::cost_tracker::CostTracker;
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

/// Active tab in the stats dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsTab {
    Overview,
    Models,
}

// ── State ───────────────────────────────────────────────────────────────────

/// Stats dashboard overlay state tracked by `App`.
pub struct StatsDashboardState {
    visible: bool,
    active_tab: StatsTab,
}

impl StatsDashboardState {
    pub fn new() -> Self {
        Self {
            visible: false,
            active_tab: StatsTab::Overview,
        }
    }

    pub fn open(&mut self) {
        self.active_tab = StatsTab::Overview;
        self.visible = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn active_tab(&self) -> StatsTab {
        self.active_tab
    }

    pub fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            StatsTab::Overview => StatsTab::Models,
            StatsTab::Models => StatsTab::Overview,
        };
    }

    pub fn prev_tab(&mut self) {
        self.next_tab(); // Only 2 tabs — prev == next.
    }

    pub fn cancel(&mut self) {
        self.close();
    }
}

impl Default for StatsDashboardState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Format a duration as "Xh Ym" or "Ym Zs".
fn format_duration(dur: Duration) -> String {
    let secs = dur.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        format!("{m}m {s}s")
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{h}h {m}m")
    }
}

/// Format a token count with K/M suffixes for readability.
fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 10_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else if count >= 1_000 {
        format!("{:.2}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Count conversation turns (each user message starts a new turn).
fn count_turns(messages: &[Message]) -> usize {
    messages.iter().filter(|m| m.role == Role::User).count()
}

/// Truncate a model name, appending … if cut.
fn truncate_model(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_string();
    }
    if max <= 3 {
        return "...".to_string();
    }
    let truncated: String = name.chars().take(max - 1).collect();
    format!("{truncated}\u{2026}")
}

// ── Widget ──────────────────────────────────────────────────────────────────

/// Stats dashboard overlay widget.
pub struct StatsDashboard<'a> {
    state: &'a StatsDashboardState,
    cost_tracker: &'a CostTracker,
    messages: &'a [Message],
    session_duration: Duration,
}

impl<'a> StatsDashboard<'a> {
    pub fn new(
        state: &'a StatsDashboardState,
        cost_tracker: &'a CostTracker,
        messages: &'a [Message],
        session_duration: Duration,
    ) -> Self {
        Self {
            state,
            cost_tracker,
            messages,
            session_duration,
        }
    }
}

/// Build the tab header line with active tab highlighted.
fn build_tab_header(active: StatsTab) -> Line<'static> {
    let active_style = Style::default()
        .fg(DIALOG_TEXT)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let inactive_style = Style::default().fg(DIALOG_MUTED);
    let sep_style = Style::default().fg(DIALOG_MUTED);

    Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(
            "Overview",
            if active == StatsTab::Overview {
                active_style
            } else {
                inactive_style
            },
        ),
        Span::styled(" \u{2502} ", sep_style),
        Span::styled(
            "Models",
            if active == StatsTab::Models {
                active_style
            } else {
                inactive_style
            },
        ),
    ])
}

/// Build a labeled stat row: "  Label:  Value".
fn stat_row(label: &str, value: &str) -> Line<'static> {
    let label_w = 16;
    Line::from(vec![
        Span::styled(
            format!("  {label:<label_w$}"),
            Style::default().fg(DIALOG_MUTED),
        ),
        Span::styled(
            format!(" {value}"),
            Style::default()
                .fg(DIALOG_TEXT)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Build overview tab lines.
fn build_overview_lines(
    cost_tracker: &CostTracker,
    messages: &[Message],
    session_duration: Duration,
) -> Vec<Line<'static>> {
    let total_cost = CostTracker::format_cost(cost_tracker.total_cost());
    let (input_tok, output_tok) = cost_tracker.total_tokens();
    let cache_read: u64 = cost_tracker
        .models
        .values()
        .map(|m| m.cache_read_tokens)
        .sum();
    let cache_write: u64 = cost_tracker
        .models
        .values()
        .map(|m| m.cache_write_tokens)
        .sum();
    let msg_count = messages.len();
    let turn_count = count_turns(messages);
    let model_count = cost_tracker.models.len();

    let mut lines = vec![
        Line::from(""),
        stat_row("Session", &format_duration(session_duration)),
        stat_row("Total Cost", &total_cost),
        stat_row(
            "Tokens",
            &format!("{} in / {} out", format_tokens(input_tok), format_tokens(output_tok)),
        ),
    ];

    if cache_read > 0 || cache_write > 0 {
        lines.push(stat_row(
            "Cache",
            &format!(
                "{} read / {} write",
                format_tokens(cache_read),
                format_tokens(cache_write)
            ),
        ));
    }

    lines.push(stat_row(
        "Messages",
        &format!("{msg_count} ({turn_count} turns)"),
    ));
    lines.push(stat_row("Models used", &model_count.to_string()));

    if cost_tracker.has_unknown_model {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  * Cost may be inaccurate (unknown model pricing)",
            Style::default().fg(Color::Yellow),
        )));
    }

    lines
}

/// Build models tab lines (table of per-model stats).
fn build_models_lines(cost_tracker: &CostTracker, body_width: u16) -> Vec<Line<'static>> {
    let inner_w = body_width as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();

    let summary = cost_tracker.summary();
    if summary.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No model usage recorded",
            Style::default().fg(DIALOG_MUTED),
        )));
        return lines;
    }

    // Column widths.
    let in_w: usize = 8;
    let out_w: usize = 8;
    let cost_w: usize = 10;
    let fixed = in_w + out_w + cost_w + 8; // padding + separators
    let model_w = inner_w.saturating_sub(fixed).max(12);

    // Header row.
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        format!(
            "  {:<model_w$}  {:>in_w$}  {:>out_w$}  {:>cost_w$}",
            "Model", "Input", "Output", "Cost",
        ),
        Style::default()
            .fg(DIALOG_MUTED)
            .add_modifier(Modifier::UNDERLINED),
    )]));

    // Data rows.
    for (model_name, usage) in &summary {
        let model_cell = truncate_model(model_name, model_w);
        let in_cell = format_tokens(usage.input_tokens);
        let out_cell = format_tokens(usage.output_tokens);
        let cost_cell = CostTracker::format_cost(usage.cost_usd);

        lines.push(Line::from(vec![
            Span::styled(
                format!("  {model_cell:<model_w$}"),
                Style::default().fg(DIALOG_ACCENT),
            ),
            Span::styled(format!("  {in_cell:>in_w$}"), Style::default().fg(DIALOG_TEXT)),
            Span::styled(
                format!("  {out_cell:>out_w$}"),
                Style::default().fg(DIALOG_TEXT),
            ),
            Span::styled(
                format!("  {cost_cell:>cost_w$}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    if cost_tracker.has_unknown_model {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  * = estimated (unknown model pricing)",
            Style::default().fg(Color::Yellow),
        )));
    }

    lines
}

/// Build footer hints.
fn build_stats_footer() -> Vec<Line<'static>> {
    let key_style = Style::default()
        .fg(Color::Black)
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);

    vec![Line::from(vec![
        Span::styled(" \u{2190}\u{2192} ", key_style),
        Span::styled(" tabs  ", Style::default().fg(DIALOG_MUTED)),
        Span::styled(" Esc ", key_style),
        Span::styled(" close", Style::default().fg(DIALOG_MUTED)),
    ])]
}

impl Widget for StatsDashboard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible || area.height < 8 || area.width < 30 {
            return;
        }

        let dialog_w = 68.min(area.width.saturating_sub(4));
        // header (title + tabs + separator) = 3, footer = 1
        let content_h = 16u16.clamp(10, 22);
        let dialog_h = content_h.min(area.height.saturating_sub(4));
        let layout = begin_modal(buf, area, dialog_w, dialog_h, 3, 1);

        // ── Header ──────────────────────────────────────────────────────
        render_modal_title(buf, layout.header_area, "Stats", "esc");

        // Tab bar (line 2 of header).
        if layout.header_area.height >= 2 {
            let tab_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 1,
                width: layout.header_area.width,
                height: 1,
            };
            let tab_line = build_tab_header(self.state.active_tab);
            buf.set_line(tab_area.x, tab_area.y, &tab_line, tab_area.width);
        }

        // Separator (line 3 of header).
        if layout.header_area.height >= 3 {
            let sep_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 2,
                width: layout.header_area.width,
                height: 1,
            };
            render_separator(buf, sep_area);
        }

        // ── Body ────────────────────────────────────────────────────────
        let body = layout.body_area;
        if body.height > 0 {
            let lines = match self.state.active_tab {
                StatsTab::Overview => {
                    build_overview_lines(self.cost_tracker, self.messages, self.session_duration)
                }
                StatsTab::Models => build_models_lines(self.cost_tracker, body.width),
            };
            Paragraph::new(lines)
                .style(Style::default().bg(PANEL_BG))
                .render(body, buf);
        }

        // ── Footer ──────────────────────────────────────────────────────
        let footer = layout.footer_area;
        if footer.height > 0 {
            let footer_lines = build_stats_footer();
            if let Some(first) = footer_lines.first() {
                buf.set_line(footer.x, footer.y, first, footer.width);
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use oxicode_common::Usage;

    fn make_tracker() -> CostTracker {
        let mut tracker = CostTracker::new("test-session".to_string());
        tracker.record(
            "claude-sonnet-4-20250514",
            &Usage {
                input_tokens: 50_000,
                output_tokens: 10_000,
                cache_read_input_tokens: Some(5_000),
                cache_creation_input_tokens: Some(1_000),
            },
        );
        tracker.record(
            "claude-opus-4-20260101",
            &Usage {
                input_tokens: 10_000,
                output_tokens: 2_000,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
            },
        );
        tracker
    }

    fn sample_messages() -> Vec<Message> {
        vec![
            Message::user("hello"),
            Message::assistant(),
            Message::user("do something"),
            Message::assistant(),
        ]
    }

    #[test]
    fn new_starts_hidden() {
        let s = StatsDashboardState::new();
        assert!(!s.is_visible());
        assert_eq!(s.active_tab(), StatsTab::Overview);
    }

    #[test]
    fn open_shows_and_resets_tab() {
        let mut s = StatsDashboardState::new();
        s.active_tab = StatsTab::Models;
        s.open();
        assert!(s.is_visible());
        assert_eq!(s.active_tab(), StatsTab::Overview);
    }

    #[test]
    fn close_hides() {
        let mut s = StatsDashboardState::new();
        s.open();
        s.close();
        assert!(!s.is_visible());
    }

    #[test]
    fn next_tab_cycles() {
        let mut s = StatsDashboardState::new();
        s.open();
        assert_eq!(s.active_tab(), StatsTab::Overview);
        s.next_tab();
        assert_eq!(s.active_tab(), StatsTab::Models);
        s.next_tab();
        assert_eq!(s.active_tab(), StatsTab::Overview);
    }

    #[test]
    fn prev_tab_cycles() {
        let mut s = StatsDashboardState::new();
        s.open();
        s.prev_tab();
        assert_eq!(s.active_tab(), StatsTab::Models);
    }

    #[test]
    fn cancel_closes() {
        let mut s = StatsDashboardState::new();
        s.open();
        s.cancel();
        assert!(!s.is_visible());
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(125)), "2m 5s");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(Duration::from_secs(7500)), "2h 5m");
    }

    #[test]
    fn format_tokens_small() {
        assert_eq!(format_tokens(500), "500");
    }

    #[test]
    fn format_tokens_thousands() {
        assert_eq!(format_tokens(5_500), "5.50K");
    }

    #[test]
    fn format_tokens_large_k() {
        assert_eq!(format_tokens(45_230), "45.2K");
    }

    #[test]
    fn format_tokens_millions() {
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn count_turns_correct() {
        let msgs = sample_messages();
        assert_eq!(count_turns(&msgs), 2);
    }

    #[test]
    fn count_turns_empty() {
        assert_eq!(count_turns(&[]), 0);
    }

    #[test]
    fn truncate_model_short() {
        assert_eq!(truncate_model("short", 20), "short");
    }

    #[test]
    fn truncate_model_long() {
        let name = "claude-sonnet-4-20250514-extended-version";
        let result = truncate_model(name, 20);
        assert!(result.chars().count() <= 20);
        assert!(result.ends_with('\u{2026}'));
    }

    #[test]
    fn overview_renders_without_panic() {
        let mut state = StatsDashboardState::new();
        state.open();
        let tracker = make_tracker();
        let msgs = sample_messages();
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        StatsDashboard::new(&state, &tracker, &msgs, Duration::from_secs(3600))
            .render(area, &mut buf);
    }

    #[test]
    fn models_tab_renders_without_panic() {
        let mut state = StatsDashboardState::new();
        state.open();
        state.next_tab();
        let tracker = make_tracker();
        let msgs = sample_messages();
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        StatsDashboard::new(&state, &tracker, &msgs, Duration::from_secs(3600))
            .render(area, &mut buf);
    }

    #[test]
    fn small_terminal_no_panic() {
        let mut state = StatsDashboardState::new();
        state.open();
        let tracker = make_tracker();
        let msgs = sample_messages();
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        StatsDashboard::new(&state, &tracker, &msgs, Duration::from_secs(60))
            .render(area, &mut buf);
    }

    #[test]
    fn empty_tracker_renders() {
        let mut state = StatsDashboardState::new();
        state.open();
        state.next_tab(); // Models tab with empty tracker.
        let tracker = CostTracker::new("empty".to_string());
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        StatsDashboard::new(&state, &tracker, &[], Duration::from_secs(0))
            .render(area, &mut buf);
    }
}
