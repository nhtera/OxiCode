use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// Overlay dialog for permission approval/denial.
///
/// Shows tool name, command/input preview in a nested box,
/// danger warning for risky operations, and keyboard shortcuts.
pub struct PermissionDialog<'a> {
    tool_name: &'a str,
    input_summary: &'a str,
    risk_description: &'a str,
    selected: usize,
    is_dangerous: bool,
}

impl<'a> PermissionDialog<'a> {
    pub fn new(tool_name: &'a str, input_summary: &'a str, risk_description: &'a str) -> Self {
        Self {
            tool_name,
            input_summary,
            risk_description,
            selected: 0,
            is_dangerous: false,
        }
    }

    pub fn with_selected(mut self, selected: usize) -> Self {
        self.selected = selected.min(OPTIONS.len() - 1);
        self
    }

    pub fn with_dangerous(mut self, is_dangerous: bool) -> Self {
        self.is_dangerous = is_dangerous;
        self
    }
}

const OPTIONS: [&str; 4] = [
    "Allow once",
    "Always allow",
    "Deny",
    "Always deny",
];

const HOTKEYS: [&str; 4] = ["y", "a", "n", "N"];

impl Widget for PermissionDialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Responsive width: prefer 40..70 but never exceed terminal.
        let dialog_width = 70u16
            .min(area.width.saturating_sub(2))
            .max(40)
            .min(area.width);
        let dialog_height = 20u16
            .min(area.height.saturating_sub(2))
            .min(area.height);
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        // Clear the area behind the dialog.
        Clear.render(dialog_area, buf);

        let border_color = if self.is_dangerous {
            Color::Red
        } else {
            Color::Yellow
        };
        let title = format!(" Allow {} ", self.tool_name);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center);

        let mut lines = Vec::new();
        lines.push(Line::from(""));

        // Command/input preview in a nested box.
        let inner_width = (dialog_width as usize).saturating_sub(6);
        let preview_label = match self.tool_name.to_lowercase().as_str() {
            "bash" => "Command:",
            "file_read" | "read" => "File:",
            "file_write" | "write" | "file_edit" | "edit" => "File:",
            "glob" => "Pattern:",
            "grep" => "Pattern:",
            _ => "Input:",
        };

        lines.push(Line::from(Span::styled(
            format!("  {preview_label}"),
            Style::default().fg(Color::DarkGray),
        )));

        // Top border of nested preview box.
        let box_fill = inner_width.saturating_sub(2);
        lines.push(Line::from(Span::styled(
            format!("  \u{250c}{}\u{2510}", "\u{2500}".repeat(box_fill)),
            Style::default().fg(Color::DarkGray),
        )));

        // Preview content — truncate to 3 lines max.
        let preview_width = inner_width.saturating_sub(4);
        let preview_lines: Vec<&str> = self.input_summary.lines().take(3).collect();
        for pline in &preview_lines {
            let display = if pline.len() > preview_width {
                format!("{}...", &pline[..preview_width.saturating_sub(3)])
            } else {
                (*pline).to_string()
            };
            lines.push(Line::from(vec![
                Span::styled("  \u{2502} ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(display, Style::default().fg(Color::White)),
            ]));
        }
        // Show truncation indicator if input has more lines.
        let total_input_lines = self.input_summary.lines().count();
        if total_input_lines > 3 {
            lines.push(Line::from(Span::styled(
                format!("  \u{2502} ... ({} more lines)", total_input_lines - 3),
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Bottom border of nested preview box.
        lines.push(Line::from(Span::styled(
            format!("  \u{2514}{}\u{2518}", "\u{2500}".repeat(box_fill)),
            Style::default().fg(Color::DarkGray),
        )));

        lines.push(Line::from(""));

        // Danger warning.
        if self.is_dangerous {
            lines.push(Line::from(vec![
                Span::styled("  \u{26a0} ", Style::default().fg(Color::Red)),
                Span::styled(
                    self.risk_description,
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
        }

        // Options with hotkey badges.
        for (i, (option, hotkey)) in OPTIONS.iter().zip(HOTKEYS.iter()).enumerate() {
            let (prefix, style) = if i == self.selected {
                ("\u{25b8} ", Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD))
            } else {
                ("  ", Style::default())
            };
            let hotkey_style = if i == self.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {prefix}"), style),
                Span::styled(format!("[{hotkey}] "), hotkey_style),
                Span::styled((*option).to_string(), style),
            ]));
        }

        lines.push(Line::from(""));

        // Footer: navigation hints.
        lines.push(Line::from(vec![
            Span::styled("  \u{2191}\u{2193}", Style::default().fg(Color::DarkGray)),
            Span::styled(" navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::styled(" confirm  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::Red)),
            Span::styled(" deny", Style::default().fg(Color::DarkGray)),
        ]));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(dialog_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_renders_without_panic() {
        let dialog = PermissionDialog::new("bash", "echo hello", "Allow shell command?");
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }

    #[test]
    fn dangerous_dialog_renders_without_panic() {
        let dialog = PermissionDialog::new("bash", "rm -rf /tmp/cache", "Destructive command")
            .with_dangerous(true);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }

    #[test]
    fn selected_option_clamped() {
        let dialog = PermissionDialog::new("bash", "cmd", "Allow?").with_selected(99);
        assert_eq!(dialog.selected, OPTIONS.len() - 1);
    }

    #[test]
    fn small_terminal_does_not_panic() {
        let dialog = PermissionDialog::new("bash", "cmd", "Allow?");
        let area = Rect::new(0, 0, 30, 10);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }

    #[test]
    fn multiline_input_truncated() {
        let long_input = (0..10).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let dialog = PermissionDialog::new("bash", &long_input, "Allow?");
        let area = Rect::new(0, 0, 80, 30);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }
}
