use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// Overlay dialog for permission approval/denial.
pub struct PermissionDialog<'a> {
    tool_name: &'a str,
    input_summary: &'a str,
    risk_description: &'a str,
    selected: usize,
}

impl<'a> PermissionDialog<'a> {
    pub fn new(tool_name: &'a str, input_summary: &'a str, risk_description: &'a str) -> Self {
        Self {
            tool_name,
            input_summary,
            risk_description,
            selected: 0,
        }
    }

    pub fn with_selected(mut self, selected: usize) -> Self {
        self.selected = selected.min(OPTIONS.len() - 1);
        self
    }
}

const OPTIONS: [&str; 2] = ["Allow", "Deny"];

impl Widget for PermissionDialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Center the dialog — 60 wide, 14 tall.
        let dialog_width = 60u16.min(area.width.saturating_sub(4));
        let dialog_height = 14u16.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        // Clear the area behind the dialog.
        Clear.render(dialog_area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                " ⚠ Permission Required ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center);

        let mut lines = Vec::new();

        lines.push(Line::from(Span::styled(
            format!("Tool: {}", self.tool_name),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // Input summary (char-safe truncation to avoid panic on multibyte).
        let summary = if self.input_summary.chars().count() > 50 {
            let idx = self
                .input_summary
                .char_indices()
                .nth(47)
                .map_or(self.input_summary.len(), |(i, _)| i);
            format!("{}...", &self.input_summary[..idx])
        } else {
            self.input_summary.to_string()
        };
        lines.push(Line::from(format!("Input: {summary}")));
        lines.push(Line::from(""));

        // Risk description.
        lines.push(Line::from(Span::styled(
            self.risk_description,
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));

        // Options.
        for (i, option) in OPTIONS.iter().enumerate() {
            let style = if i == self.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if i == self.selected { "▸ " } else { "  " };
            lines.push(Line::from(Span::styled(format!("{prefix}{option}"), style)));
        }

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(dialog_area, buf);
    }
}
