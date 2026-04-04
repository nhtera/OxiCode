//! OAuth flow dialog: show URL, waiting spinner, success/failure states.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// OAuth flow state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthState {
    /// Browser opened with URL.
    Opening { url: String },
    /// Waiting for user to authorize.
    Waiting { elapsed_secs: u32 },
    /// Login succeeded.
    Success { email: String },
    /// Login failed.
    Failed { reason: String },
    /// Login timed out.
    TimedOut,
}

/// Overlay dialog for OAuth login flow.
pub struct OAuthDialog<'a> {
    state: &'a OAuthState,
}

impl<'a> OAuthDialog<'a> {
    pub fn new(state: &'a OAuthState) -> Self {
        Self { state }
    }
}

/// Simple spinner frames for the waiting state.
const SPINNER: [&str; 4] = ["\u{25dc}", "\u{25dd}", "\u{25de}", "\u{25df}"];

impl Widget for OAuthDialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let dialog_width = 56u16.min(area.width.saturating_sub(4));
        let dialog_height = 10u16.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        Clear.render(dialog_area, buf);

        let (title_text, title_color) = match self.state {
            OAuthState::Success { .. } => (" \u{2713} Login ", Color::Green),
            OAuthState::Failed { .. } | OAuthState::TimedOut => (" \u{2717} Login ", Color::Red),
            _ => (" OAuth Login ", Color::Cyan),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(title_color))
            .title(Span::styled(
                title_text,
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center);

        let lines = match self.state {
            OAuthState::Opening { url } => {
                let short_url: String = url.chars().take(45).collect();
                vec![
                    Line::from("Opening browser..."),
                    Line::from(""),
                    Line::from(Span::styled(
                        format!("URL: {short_url}"),
                        Style::default().fg(Color::Cyan),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Waiting for authorization...",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            }
            OAuthState::Waiting { elapsed_secs } => {
                let frame = SPINNER[(*elapsed_secs as usize) % SPINNER.len()];
                vec![
                    Line::from(Span::styled(
                        format!("{frame} Waiting for authorization..."),
                        Style::default().fg(Color::Yellow),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        format!("Elapsed: {elapsed_secs}s (timeout: 120s)"),
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            }
            OAuthState::Success { email } => {
                vec![
                    Line::from(Span::styled(
                        format!("\u{2713} Logged in as {email}"),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from("Press any key to continue."),
                ]
            }
            OAuthState::Failed { reason } => {
                vec![
                    Line::from(Span::styled(
                        format!("\u{2717} Login failed: {reason}"),
                        Style::default().fg(Color::Red),
                    )),
                    Line::from(""),
                    Line::from("Press any key to dismiss."),
                ]
            }
            OAuthState::TimedOut => {
                vec![
                    Line::from(Span::styled(
                        "Login timed out (120s).",
                        Style::default().fg(Color::Red),
                    )),
                    Line::from(""),
                    Line::from("Press any key to dismiss."),
                ]
            }
        };

        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .render(dialog_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_state_variants() {
        let _opening = OAuthState::Opening {
            url: "https://example.com".to_string(),
        };
        let _waiting = OAuthState::Waiting { elapsed_secs: 5 };
        let _success = OAuthState::Success {
            email: "user@test.com".to_string(),
        };
        let _failed = OAuthState::Failed {
            reason: "denied".to_string(),
        };
        let _timed_out = OAuthState::TimedOut;
    }
}
