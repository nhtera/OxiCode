//! MCP elicitation dialog overlay.
//!
//! Renders a modal prompting the user for input requested by an MCP server
//! via `elicitation/create`. Supports four input types:
//!
//! - `Text`: single-line editable input.
//! - `Secret`: same as Text but renders as `●` mask.
//! - `Confirm`: Yes / No binary choice.
//! - `Select`: vertical list over `choices`, navigated with Up/Down.
//!
//! Submission is `Enter`. Cancellation is `Esc` or `Ctrl+C` → `approved = false`.
//!
//! The widget is a pure state machine; the surrounding `App` owns a
//! `oneshot::Sender<ElicitationResponse>` and fires the final response
//! back over the bridge channel on submit/cancel.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use oxicode_mcp::{ElicitationInputType, ElicitationRequest, ElicitationResponse};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use crate::widgets::modal_helpers::{
    begin_modal, DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT, PANEL_BG,
};

/// User outcome from a key event tick.
#[derive(Debug)]
pub enum ElicitationOutcome {
    /// Keep the dialog open, continue capturing input.
    Continue,
    /// User submitted a response (approved or not) — caller must route
    /// this to the MCP reply channel and close the overlay.
    Complete(ElicitationResponse),
}

/// Stateful elicitation dialog. One instance per in-flight request.
pub struct ElicitationDialog {
    request: ElicitationRequest,
    /// Buffer for `Text` / `Secret` input.
    input: String,
    /// Current selection index for `Select`.
    selected: usize,
    /// Current `Confirm` button: true = Yes, false = No.
    confirm_yes: bool,
}

impl ElicitationDialog {
    pub fn new(request: ElicitationRequest) -> Self {
        let initial_input = match request.input_type {
            ElicitationInputType::Text | ElicitationInputType::Secret => {
                request.default_value.clone().unwrap_or_default()
            }
            _ => String::new(),
        };
        // Pre-select the default choice for Select, if present.
        let selected = match request.input_type {
            ElicitationInputType::Select => request
                .default_value
                .as_ref()
                .and_then(|d| request.choices.iter().position(|c| c == d))
                .unwrap_or(0),
            _ => 0,
        };
        // Pre-fill Confirm state from default (truthy strings → Yes).
        let confirm_yes = matches!(
            request.default_value.as_deref(),
            Some("yes" | "y" | "true" | "1")
        );
        Self {
            request,
            input: initial_input,
            selected,
            confirm_yes,
        }
    }

    /// The underlying request (for test access / debug display).
    pub fn request(&self) -> &ElicitationRequest {
        &self.request
    }

    /// The current input buffer (for tests).
    #[cfg(test)]
    pub fn input_buffer(&self) -> &str {
        &self.input
    }

    /// Build a denial response with the current request id.
    fn denial(&self) -> ElicitationResponse {
        ElicitationResponse {
            id: self.request.id.clone(),
            approved: false,
            value: String::new(),
        }
    }

    /// Build an approval response with the given value.
    fn approval(&self, value: String) -> ElicitationResponse {
        ElicitationResponse {
            id: self.request.id.clone(),
            approved: true,
            value,
        }
    }

    /// Process a key event. Returns `Complete(_)` when the dialog should close.
    pub fn handle_key(&mut self, key: KeyEvent) -> ElicitationOutcome {
        // Global cancel keys.
        if matches!(key.code, KeyCode::Esc)
            || (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c')))
        {
            return ElicitationOutcome::Complete(self.denial());
        }

        match self.request.input_type {
            ElicitationInputType::Text | ElicitationInputType::Secret => self.handle_text_key(key),
            ElicitationInputType::Confirm => self.handle_confirm_key(key),
            ElicitationInputType::Select => self.handle_select_key(key),
        }
    }

    fn handle_text_key(&mut self, key: KeyEvent) -> ElicitationOutcome {
        match key.code {
            KeyCode::Enter => {
                let value = std::mem::take(&mut self.input);
                ElicitationOutcome::Complete(self.approval(value))
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                ElicitationOutcome::Continue
            }
            KeyCode::Backspace => {
                self.input.pop();
                ElicitationOutcome::Continue
            }
            _ => ElicitationOutcome::Continue,
        }
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) -> ElicitationOutcome {
        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                self.confirm_yes = !self.confirm_yes;
                ElicitationOutcome::Continue
            }
            KeyCode::Char('y' | 'Y') => {
                ElicitationOutcome::Complete(self.approval("yes".to_string()))
            }
            KeyCode::Char('n' | 'N') => ElicitationOutcome::Complete(self.denial()),
            KeyCode::Enter => {
                if self.confirm_yes {
                    ElicitationOutcome::Complete(self.approval("yes".to_string()))
                } else {
                    ElicitationOutcome::Complete(self.denial())
                }
            }
            _ => ElicitationOutcome::Continue,
        }
    }

    fn handle_select_key(&mut self, key: KeyEvent) -> ElicitationOutcome {
        let n = self.request.choices.len();
        match key.code {
            KeyCode::Up => {
                if n > 0 {
                    self.selected = self.selected.saturating_sub(1);
                }
                ElicitationOutcome::Continue
            }
            KeyCode::Down => {
                if n > 0 {
                    self.selected = (self.selected + 1).min(n.saturating_sub(1));
                }
                ElicitationOutcome::Continue
            }
            KeyCode::Enter => {
                if let Some(choice) = self.request.choices.get(self.selected).cloned() {
                    ElicitationOutcome::Complete(self.approval(choice))
                } else {
                    ElicitationOutcome::Complete(self.denial())
                }
            }
            _ => ElicitationOutcome::Continue,
        }
    }
}

impl Widget for &ElicitationDialog {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Responsive: prefer 60 wide, minimum 40; height depends on content.
        let width = 60u16
            .min(area.width.saturating_sub(2))
            .max(40)
            .min(area.width);
        let choice_rows = match self.request.input_type {
            ElicitationInputType::Select => self.request.choices.len() as u16,
            _ => 0,
        };
        // message (wrapped) + body (~3 rows) + footer (1).
        let body_rows = match self.request.input_type {
            ElicitationInputType::Select => choice_rows.max(1),
            _ => 1,
        };
        let height = (6 + body_rows).min(area.height.saturating_sub(2)).max(8);

        let layout = begin_modal(buf, area, width, height, 2, 2);

        // Border + title.
        let title = match self.request.input_type {
            ElicitationInputType::Text => " Input requested ",
            ElicitationInputType::Confirm => " Confirm ",
            ElicitationInputType::Select => " Select ",
            ElicitationInputType::Secret => " Secret input ",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(DIALOG_ACCENT).bg(PANEL_BG))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(DIALOG_ACCENT)
                    .bg(PANEL_BG)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(PANEL_BG));
        block.render(layout.dialog_area, buf);

        // Header: the server's message (wrap to width).
        let header = Paragraph::new(Line::from(Span::styled(
            self.request.message.clone(),
            Style::default().fg(DIALOG_TEXT).bg(PANEL_BG),
        )))
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(PANEL_BG));
        header.render(layout.header_area, buf);

        // Body: type-specific content.
        self.render_body(layout.body_area, buf);

        // Footer: key hints.
        let hints = match self.request.input_type {
            ElicitationInputType::Select => "↑↓ select   Enter confirm   Esc cancel",
            ElicitationInputType::Confirm => "y/n   ←/→ toggle   Enter confirm   Esc cancel",
            _ => "type value   Enter submit   Esc cancel",
        };
        let footer = Paragraph::new(Line::from(Span::styled(
            hints,
            Style::default().fg(DIALOG_MUTED).bg(PANEL_BG),
        )))
        .alignment(Alignment::Center)
        .style(Style::default().bg(PANEL_BG));
        footer.render(layout.footer_area, buf);
    }
}

impl ElicitationDialog {
    fn render_body(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        match self.request.input_type {
            ElicitationInputType::Text => self.render_text_body(area, buf, false),
            ElicitationInputType::Secret => self.render_text_body(area, buf, true),
            ElicitationInputType::Confirm => self.render_confirm_body(area, buf),
            ElicitationInputType::Select => self.render_select_body(area, buf),
        }
    }

    fn render_text_body(&self, area: Rect, buf: &mut Buffer, mask: bool) {
        let display: String = if mask {
            "\u{25CF}".repeat(self.input.chars().count())
        } else {
            self.input.clone()
        };
        let line = Line::from(vec![
            Span::styled("› ", Style::default().fg(DIALOG_ACCENT).bg(PANEL_BG)),
            Span::styled(display, Style::default().fg(DIALOG_TEXT).bg(PANEL_BG)),
            // Cursor block.
            Span::styled(
                "\u{2588}",
                Style::default()
                    .fg(DIALOG_ACCENT)
                    .bg(PANEL_BG)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ]);
        Paragraph::new(line)
            .style(Style::default().bg(PANEL_BG))
            .render(area, buf);
    }

    fn render_confirm_body(&self, area: Rect, buf: &mut Buffer) {
        let (yes_style, no_style) = if self.confirm_yes {
            (
                Style::default()
                    .fg(PANEL_BG)
                    .bg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(DIALOG_TEXT).bg(PANEL_BG),
            )
        } else {
            (
                Style::default().fg(DIALOG_TEXT).bg(PANEL_BG),
                Style::default()
                    .fg(PANEL_BG)
                    .bg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
        };
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(" [y] Yes ", yes_style),
            Span::raw("    "),
            Span::styled(" [n] No ", no_style),
        ]);
        Paragraph::new(line)
            .alignment(Alignment::Center)
            .style(Style::default().bg(PANEL_BG))
            .render(area, buf);
    }

    fn render_select_body(&self, area: Rect, buf: &mut Buffer) {
        let lines: Vec<Line<'_>> = self
            .request
            .choices
            .iter()
            .enumerate()
            .map(|(i, choice)| {
                let (prefix, style) = if i == self.selected {
                    (
                        "\u{25b8} ",
                        Style::default()
                            .fg(PANEL_BG)
                            .bg(DIALOG_ACCENT)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    ("  ", Style::default().fg(DIALOG_TEXT).bg(PANEL_BG))
                };
                Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(choice.clone(), style),
                ])
            })
            .collect();
        Paragraph::new(lines)
            .style(Style::default().bg(PANEL_BG))
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_req() -> ElicitationRequest {
        ElicitationRequest {
            id: "r1".to_string(),
            message: "Enter API key".to_string(),
            input_type: ElicitationInputType::Text,
            choices: vec![],
            default_value: None,
        }
    }

    fn secret_req() -> ElicitationRequest {
        ElicitationRequest {
            id: "r2".to_string(),
            message: "Enter password".to_string(),
            input_type: ElicitationInputType::Secret,
            choices: vec![],
            default_value: None,
        }
    }

    fn confirm_req() -> ElicitationRequest {
        ElicitationRequest {
            id: "r3".to_string(),
            message: "Delete file?".to_string(),
            input_type: ElicitationInputType::Confirm,
            choices: vec![],
            default_value: None,
        }
    }

    fn select_req() -> ElicitationRequest {
        ElicitationRequest {
            id: "r4".to_string(),
            message: "Pick color".to_string(),
            input_type: ElicitationInputType::Select,
            choices: vec!["red".into(), "blue".into(), "green".into()],
            default_value: Some("blue".into()),
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn text_type_and_enter_approves_with_value() {
        let mut d = ElicitationDialog::new(text_req());
        for c in "sk-abc".chars() {
            assert!(matches!(
                d.handle_key(key(KeyCode::Char(c))),
                ElicitationOutcome::Continue
            ));
        }
        match d.handle_key(key(KeyCode::Enter)) {
            ElicitationOutcome::Complete(r) => {
                assert!(r.approved);
                assert_eq!(r.value, "sk-abc");
                assert_eq!(r.id, "r1");
            }
            ElicitationOutcome::Continue => panic!("expected Complete"),
        }
    }

    #[test]
    fn text_backspace_deletes_last_char() {
        let mut d = ElicitationDialog::new(text_req());
        for c in "abc".chars() {
            d.handle_key(key(KeyCode::Char(c)));
        }
        d.handle_key(key(KeyCode::Backspace));
        assert_eq!(d.input_buffer(), "ab");
    }

    #[test]
    fn esc_denies() {
        let mut d = ElicitationDialog::new(text_req());
        d.handle_key(key(KeyCode::Char('x')));
        match d.handle_key(key(KeyCode::Esc)) {
            ElicitationOutcome::Complete(r) => {
                assert!(!r.approved);
                assert!(r.value.is_empty());
            }
            ElicitationOutcome::Continue => panic!("expected Complete"),
        }
    }

    #[test]
    fn ctrl_c_denies() {
        let mut d = ElicitationDialog::new(secret_req());
        let k = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        match d.handle_key(k) {
            ElicitationOutcome::Complete(r) => assert!(!r.approved),
            ElicitationOutcome::Continue => panic!("expected Complete"),
        }
    }

    #[test]
    fn confirm_default_no_then_enter_denies() {
        let mut d = ElicitationDialog::new(confirm_req());
        match d.handle_key(key(KeyCode::Enter)) {
            ElicitationOutcome::Complete(r) => assert!(!r.approved),
            ElicitationOutcome::Continue => panic!("expected Complete"),
        }
    }

    #[test]
    fn confirm_y_hotkey_approves() {
        let mut d = ElicitationDialog::new(confirm_req());
        match d.handle_key(key(KeyCode::Char('y'))) {
            ElicitationOutcome::Complete(r) => {
                assert!(r.approved);
                assert_eq!(r.value, "yes");
            }
            ElicitationOutcome::Continue => panic!("expected Complete"),
        }
    }

    #[test]
    fn confirm_left_toggles_then_enter_approves() {
        let mut d = ElicitationDialog::new(confirm_req());
        d.handle_key(key(KeyCode::Left));
        match d.handle_key(key(KeyCode::Enter)) {
            ElicitationOutcome::Complete(r) => assert!(r.approved),
            ElicitationOutcome::Continue => panic!("expected Complete"),
        }
    }

    #[test]
    fn select_default_preselects_from_default_value() {
        let d = ElicitationDialog::new(select_req());
        assert_eq!(d.selected, 1); // "blue" is at index 1.
    }

    #[test]
    fn select_down_and_enter_returns_choice() {
        let mut d = ElicitationDialog::new(select_req());
        // Start at "blue" (1), move down to "green" (2).
        d.handle_key(key(KeyCode::Down));
        match d.handle_key(key(KeyCode::Enter)) {
            ElicitationOutcome::Complete(r) => {
                assert!(r.approved);
                assert_eq!(r.value, "green");
            }
            ElicitationOutcome::Continue => panic!("expected Complete"),
        }
    }

    #[test]
    fn select_up_clamps_at_zero() {
        let mut d = ElicitationDialog::new(select_req());
        // From index 1 ("blue"), go up twice → clamps at 0.
        d.handle_key(key(KeyCode::Up));
        d.handle_key(key(KeyCode::Up));
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn secret_masks_input_when_rendered() {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;

        let mut d = ElicitationDialog::new(secret_req());
        for c in "hunter2".chars() {
            d.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(d.input_buffer(), "hunter2");

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        (&d).render(area, &mut buf);

        // The rendered buffer should contain mask chars, not the raw secret.
        let rendered: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(rendered.contains('\u{25CF}'));
        assert!(!rendered.contains("hunter2"));
    }

    #[test]
    fn text_prefills_default_value() {
        let req = ElicitationRequest {
            id: "r-pref".into(),
            message: "Your name?".into(),
            input_type: ElicitationInputType::Text,
            choices: vec![],
            default_value: Some("alice".into()),
        };
        let d = ElicitationDialog::new(req);
        assert_eq!(d.input_buffer(), "alice");
    }
}
