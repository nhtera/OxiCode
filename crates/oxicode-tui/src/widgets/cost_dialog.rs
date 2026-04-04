//! Cost threshold dialog: warn when session cost exceeds configurable limit.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// Cost threshold dialog options.
const OPTIONS: [&str; 3] = ["Continue", "Set new threshold", "Stop"];

/// Overlay dialog shown when session cost exceeds threshold.
pub struct CostDialog {
    current_cost: f64,
    threshold: f64,
    selected: usize,
}

impl CostDialog {
    pub fn new(current_cost: f64, threshold: f64) -> Self {
        Self {
            current_cost,
            threshold,
            selected: 0,
        }
    }

    pub fn with_selected(mut self, selected: usize) -> Self {
        self.selected = selected.min(OPTIONS.len() - 1);
        self
    }
}

impl Widget for CostDialog {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let dialog_width = 50u16.min(area.width.saturating_sub(4));
        let dialog_height = 12u16.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        Clear.render(dialog_area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                " Cost Warning ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center);

        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            format!("Session cost: ${:.2}", self.current_cost),
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "Exceeds threshold: ${:.2}",
            self.threshold
        )));
        lines.push(Line::from(""));

        for (i, option) in OPTIONS.iter().enumerate() {
            let style = if i == self.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if i == self.selected { "\u{25b8} " } else { "  " };
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
    fn test_cost_dialog_selected_clamp() {
        let dialog = CostDialog::new(10.0, 5.0).with_selected(99);
        assert_eq!(dialog.selected, 2);
    }
}
