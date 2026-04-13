//! Model picker overlay — `/model` command or `Ctrl+A`.
//!
//! Displays a centered modal with a searchable list of available AI models.
//! Supports effort levels for models with extended thinking, and highlights
//! the currently active model.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::modal_helpers::{
    begin_modal, render_modal_title, render_separator, DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT,
    PANEL_BG,
};

// ── Effort level ────────────────────────────────────────────────────────────

/// Controls extended-thinking `budget_tokens` sent to the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EffortLevel {
    Low,
    #[default]
    Normal,
    High,
    Max,
}

impl EffortLevel {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Low => "\u{25cb}",    // ○
            Self::Normal => "\u{25d0}", // ◐
            Self::High => "\u{25d5}",   // ◕
            Self::Max => "\u{25cf}",    // ●
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Max => "max",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Low => Self::Normal,
            Self::Normal => Self::High,
            Self::High => Self::Max,
            Self::Max => Self::Low,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Low => Self::Max,
            Self::Normal => Self::Low,
            Self::High => Self::Normal,
            Self::Max => Self::High,
        }
    }
}

/// Returns `true` for models supporting extended thinking / effort levels.
pub fn model_supports_effort(id: &str) -> bool {
    id.starts_with("claude-3-7")
        || id.starts_with("claude-opus-4")
        || id.starts_with("claude-sonnet-4")
}

// ── Model entry ─────────────────────────────────────────────────────────────

/// A single model shown in the picker.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_current: bool,
}

/// Build a `ModelEntry` with `is_current = false`.
fn model_entry(id: &str, name: &str, desc: &str) -> ModelEntry {
    ModelEntry {
        id: id.to_string(),
        display_name: name.to_string(),
        description: desc.to_string(),
        is_current: false,
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// Model picker overlay state tracked by `App`.
pub struct ModelPickerState {
    pub visible: bool,
    pub selected_idx: usize,
    pub models: Vec<ModelEntry>,
    pub filter: String,
    pub effort_level: EffortLevel,
}

impl ModelPickerState {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected_idx: 0,
            models: Self::default_models(),
            filter: String::new(),
            effort_level: EffortLevel::Normal,
        }
    }

    pub fn open(&mut self, current_model: &str) {
        for m in &mut self.models {
            m.is_current = m.id == current_model;
        }
        self.selected_idx = self.models.iter().position(|m| m.is_current).unwrap_or(0);
        self.filter.clear();
        self.visible = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.filter.clear();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn select_prev(&mut self) {
        let count = self.filtered_models().len();
        if count == 0 {
            return;
        }
        if self.selected_idx == 0 {
            self.selected_idx = count - 1;
        } else {
            self.selected_idx -= 1;
        }
    }

    pub fn select_next(&mut self) {
        let count = self.filtered_models().len();
        if count == 0 {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % count;
    }

    pub fn effort_next(&mut self) {
        self.effort_level = self.effort_level.next();
    }

    pub fn effort_prev(&mut self) {
        self.effort_level = self.effort_level.prev();
    }

    /// Confirm selection. Returns `(model_id, effort)` and closes the picker.
    pub fn confirm(&mut self) -> Option<(String, EffortLevel)> {
        let filtered = self.filtered_models();
        let entry = filtered.get(self.selected_idx)?;
        let id = entry.id.clone();
        let effort = self.effort_level;
        self.close();
        Some((id, effort))
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected_idx = 0;
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.selected_idx = 0;
    }

    pub fn filtered_models(&self) -> Vec<&ModelEntry> {
        if self.filter.is_empty() {
            return self.models.iter().collect();
        }
        let needle = self.filter.to_lowercase();
        self.models
            .iter()
            .filter(|m| {
                m.id.to_lowercase().contains(&needle)
                    || m.display_name.to_lowercase().contains(&needle)
                    || m.description.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// Replace the model list (e.g. after fetching from API).
    pub fn set_models(&mut self, entries: Vec<ModelEntry>) {
        self.models = entries;
        let count = self.filtered_models().len();
        if count > 0 && self.selected_idx >= count {
            self.selected_idx = count - 1;
        }
    }

    /// Default hardcoded Claude model list.
    pub fn default_models() -> Vec<ModelEntry> {
        vec![
            model_entry(
                "claude-opus-4-6",
                "Claude Opus 4.6",
                "Most capable — complex reasoning",
            ),
            model_entry(
                "claude-sonnet-4-6",
                "Claude Sonnet 4.6",
                "Balanced — great for coding",
            ),
            model_entry(
                "claude-haiku-4-5",
                "Claude Haiku 4.5",
                "Fast — quick completions",
            ),
            model_entry(
                "claude-opus-4-5",
                "Claude Opus 4.5",
                "Previous Opus — powerful reasoning",
            ),
            model_entry(
                "claude-sonnet-4-5",
                "Claude Sonnet 4.5",
                "Previous Sonnet — solid coding",
            ),
            model_entry(
                "claude-3-7-sonnet-20250219",
                "Claude 3.7 Sonnet",
                "Enhanced instruction following",
            ),
            model_entry(
                "claude-3-5-sonnet-20241022",
                "Claude 3.5 Sonnet",
                "Well-tested and reliable",
            ),
            model_entry(
                "claude-3-5-haiku-20241022",
                "Claude 3.5 Haiku",
                "Fast — high-throughput",
            ),
        ]
    }
}

impl Default for ModelPickerState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Widget ──────────────────────────────────────────────────────────────────

/// Model picker overlay widget — centered modal rendered on top of content.
pub struct ModelPicker<'a> {
    state: &'a ModelPickerState,
}

impl<'a> ModelPicker<'a> {
    pub fn new(state: &'a ModelPickerState) -> Self {
        Self { state }
    }
}

/// Build the model list lines for the body area.
#[allow(clippy::similar_names)]
fn build_model_lines(state: &ModelPickerState) -> Vec<Line<'static>> {
    let filtered = state.filtered_models();
    let active_bg = Color::Rgb(80, 45, 15); // Warm dark amber
    let active_fg = Color::Rgb(255, 235, 210); // Warm white
    let mut lines: Vec<Line<'static>> = Vec::new();

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            " No matching models",
            Style::default().fg(DIALOG_MUTED),
        )));
        return lines;
    }

    for (i, model) in filtered.iter().enumerate() {
        let is_selected = i == state.selected_idx;
        let supports_effort = model_supports_effort(&model.id);

        let (fg, bg) = if is_selected {
            (active_fg, active_bg)
        } else {
            (DIALOG_TEXT, PANEL_BG)
        };

        let mut spans: Vec<Span<'static>> = Vec::new();

        // Current model indicator
        if model.is_current {
            spans.push(Span::styled(
                " \u{25cf} ",
                Style::default().fg(DIALOG_ACCENT).bg(bg),
            ));
        } else {
            spans.push(Span::styled("   ", Style::default().bg(bg)));
        }

        // Model name
        spans.push(Span::styled(
            model.display_name.clone(),
            Style::default().fg(fg).bg(bg).add_modifier(if is_selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ));

        // Effort indicator (only on selected row for effort-supporting models)
        if supports_effort && is_selected {
            spans.push(Span::styled(
                format!(
                    "  {} {}",
                    state.effort_level.symbol(),
                    state.effort_level.label()
                ),
                Style::default().fg(DIALOG_ACCENT).bg(bg),
            ));
        }

        // Description
        if !model.description.is_empty() {
            let desc_fg = if is_selected {
                Color::Rgb(200, 192, 185)
            } else {
                DIALOG_MUTED
            };
            spans.push(Span::styled(
                format!("  {}", model.description),
                Style::default().fg(desc_fg).bg(bg),
            ));
        }

        lines.push(Line::from(spans));
    }

    lines
}

/// Render the model picker search filter line into the header area.
fn render_picker_search(buf: &mut Buffer, header: Rect, filter: &str) {
    if header.height >= 2 {
        let sep_area = Rect {
            x: header.x,
            y: header.y + 1,
            width: header.width,
            height: 1,
        };
        render_separator(buf, sep_area);
    }

    if header.height >= 3 {
        let search_area = Rect {
            x: header.x,
            y: header.y + 2,
            width: header.width,
            height: 1,
        };
        let search_line = if filter.is_empty() {
            Line::from(vec![
                Span::styled(" \u{1F50D} ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(
                    "Type to filter models...",
                    Style::default().fg(DIALOG_MUTED),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(" \u{1F50D} ", Style::default().fg(DIALOG_ACCENT)),
                Span::styled(filter.to_string(), Style::default().fg(DIALOG_TEXT)),
                Span::styled("\u{2588}", Style::default().fg(Color::White)),
            ])
        };
        buf.set_line(
            search_area.x,
            search_area.y,
            &search_line,
            search_area.width,
        );
    }
}

/// Build the footer hint line for the model picker.
fn build_picker_footer(show_effort: bool) -> Line<'static> {
    let key_style = Style::default()
        .fg(Color::Black)
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![
        Span::styled(" Enter ", key_style),
        Span::styled(" select  ", Style::default().fg(DIALOG_MUTED)),
        Span::styled(" Esc ", key_style),
        Span::styled(" close", Style::default().fg(DIALOG_MUTED)),
    ];
    if show_effort {
        spans.push(Span::styled("  ", Style::default().fg(DIALOG_MUTED)));
        spans.push(Span::styled(" \u{2190}/\u{2192} ", key_style));
        spans.push(Span::styled(" effort", Style::default().fg(DIALOG_MUTED)));
    }
    Line::from(spans)
}

impl Widget for ModelPicker<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible || area.height < 8 || area.width < 30 {
            return;
        }

        // Modal dimensions — scale with terminal size.
        let dialog_w = 70.min(area.width.saturating_sub(6));
        let filtered_count = self.state.filtered_models().len() as u16;
        let content_h = (filtered_count + 6).clamp(8, 24);
        let dialog_h = content_h.min(area.height.saturating_sub(4));
        // header: title(1) + separator(1) + search(1) = 3, footer: hint(1) = 1
        let layout = begin_modal(buf, area, dialog_w, dialog_h, 3, 1);

        // ── Header ───────────────────────────────────────────────────────────
        render_modal_title(buf, layout.header_area, "Select Model", "esc");
        render_picker_search(buf, layout.header_area, &self.state.filter);

        // ── Body: model list ─────────────────────────────────────────────────
        let body = layout.body_area;
        if body.height == 0 {
            return;
        }

        let lines = build_model_lines(self.state);

        // Auto-scroll to keep selected item visible
        let selected_line = self.state.selected_idx as u16;
        let scroll_y = if lines.len() as u16 <= body.height {
            0
        } else if selected_line + 3 >= body.height {
            (selected_line + 3).saturating_sub(body.height)
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
            let show_effort = self
                .state
                .filtered_models()
                .get(self.state.selected_idx)
                .is_some_and(|m| model_supports_effort(&m.id));
            let footer_line = build_picker_footer(show_effort);
            buf.set_line(footer.x, footer.y, &footer_line, footer.width);
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_models_non_empty() {
        let models = ModelPickerState::default_models();
        assert!(models.len() >= 5, "should have at least 5 default models");
    }

    #[test]
    fn open_marks_current_model() {
        let mut p = ModelPickerState::new();
        p.open("claude-sonnet-4-6");
        assert!(p.visible);
        let current_count = p.models.iter().filter(|m| m.is_current).count();
        assert_eq!(current_count, 1);
        assert!(p
            .models
            .iter()
            .any(|m| m.id == "claude-sonnet-4-6" && m.is_current));
    }

    #[test]
    fn open_unknown_model_selects_first() {
        let mut p = ModelPickerState::new();
        p.open("unknown-model");
        assert_eq!(p.selected_idx, 0);
        assert!(p.models.iter().all(|m| !m.is_current));
    }

    #[test]
    fn select_next_wraps() {
        let mut p = ModelPickerState::new();
        p.open("claude-opus-4-6");
        let total = p.filtered_models().len();
        p.selected_idx = total - 1;
        p.select_next();
        assert_eq!(p.selected_idx, 0);
    }

    #[test]
    fn select_prev_wraps() {
        let mut p = ModelPickerState::new();
        p.open("claude-opus-4-6");
        p.selected_idx = 0;
        p.select_prev();
        let total = p.filtered_models().len();
        assert_eq!(p.selected_idx, total - 1);
    }

    #[test]
    fn filter_reduces_results() {
        let mut p = ModelPickerState::new();
        p.open("claude-opus-4-6");
        for c in "sonnet".chars() {
            p.push_filter_char(c);
        }
        let filtered = p.filtered_models();
        assert!(filtered.len() < p.models.len());
        assert!(!filtered.is_empty());
    }

    #[test]
    fn confirm_returns_selection_and_closes() {
        let mut p = ModelPickerState::new();
        p.open("claude-sonnet-4-6");
        p.selected_idx = 0; // First model
        let result = p.confirm();
        assert!(result.is_some());
        assert!(!p.visible);
    }

    #[test]
    fn effort_cycles() {
        let mut p = ModelPickerState::new();
        assert_eq!(p.effort_level, EffortLevel::Normal);
        p.effort_next();
        assert_eq!(p.effort_level, EffortLevel::High);
        p.effort_next();
        assert_eq!(p.effort_level, EffortLevel::Max);
        p.effort_next();
        assert_eq!(p.effort_level, EffortLevel::Low);
        p.effort_prev();
        assert_eq!(p.effort_level, EffortLevel::Max);
    }

    #[test]
    fn model_picker_renders_without_panic() {
        let mut state = ModelPickerState::new();
        state.open("claude-sonnet-4-6");
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        ModelPicker::new(&state).render(area, &mut buf);
    }

    #[test]
    fn model_picker_skips_small_terminal() {
        let mut state = ModelPickerState::new();
        state.open("claude-sonnet-4-6");
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        ModelPicker::new(&state).render(area, &mut buf);
        // Should render nothing (area too small).
    }
}
