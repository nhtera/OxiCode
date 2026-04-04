//! Auto-mode opt-in dialog: confirm before entering bypass-permissions mode.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// Auto-mode opt-in options.
const OPTIONS: [&str; 2] = ["Enable", "Cancel"];

/// Overlay dialog shown when switching to bypass-permissions mode.
pub struct AutoModeDialog {
    selected: usize,
}

impl AutoModeDialog {
    pub fn new() -> Self {
        Self { selected: 1 } // Default to Cancel for safety
    }

    pub fn with_selected(mut self, selected: usize) -> Self {
        self.selected = selected.min(OPTIONS.len() - 1);
        self
    }
}

impl Default for AutoModeDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for AutoModeDialog {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let dialog_width = 56u16.min(area.width.saturating_sub(4));
        let dialog_height = 10u16.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        Clear.render(dialog_area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title(Span::styled(
                " \u{26a0} Bypass Permissions ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center);

        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            "Bypass permissions mode allows ALL",
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(Span::styled(
            "tool calls without approval.",
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from("Enable bypass mode?"));
        lines.push(Line::from(""));

        for (i, option) in OPTIONS.iter().enumerate() {
            let style = if i == self.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(if i == 0 { Color::Red } else { Color::Green })
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if i == self.selected {
                "\u{25b8} "
            } else {
                "  "
            };
            lines.push(Line::from(Span::styled(format!("{prefix}{option}"), style)));
        }

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
    fn test_default_cancel() {
        let dialog = AutoModeDialog::new();
        assert_eq!(dialog.selected, 1);
    }
}
