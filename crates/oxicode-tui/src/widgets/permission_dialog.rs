use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// Risk level assigned to a tool, drives border color and visual emphasis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    /// Read-only / search tools — safe, green border.
    Low,
    /// File mutation or network tools — moderate, yellow border.
    Moderate,
    /// Shell execution — high risk, red border.
    High,
}

impl RiskLevel {
    /// Map a tool name to its risk level.
    pub fn from_tool(tool_name: &str) -> Self {
        match tool_name.to_lowercase().as_str() {
            // Low-risk: read-only / search
            "read" | "file_read" | "glob" | "grep" | "lsp" | "list_directory" | "ls" => Self::Low,
            // High-risk: shell execution
            "bash" => Self::High,
            // Moderate-risk: file mutation, web, multi-edit
            "write" | "file_write" | "edit" | "file_edit" | "multiedit" | "multi_edit"
            | "webfetch" | "web_fetch" | "web_search" => Self::Moderate,
            // Default: moderate
            _ => Self::Moderate,
        }
    }

    /// Border / title color for this risk level.
    pub fn border_color(self) -> Color {
        match self {
            Self::Low => crate::render::STATUS_GREEN,
            Self::Moderate => crate::render::STATUS_YELLOW,
            Self::High => crate::render::STATUS_RED,
        }
    }
}

/// Overlay dialog for permission approval/denial.
///
/// Shows tool name, command/input preview in a nested box,
/// danger warning for risky operations, and keyboard shortcuts.
pub struct PermissionDialog<'a> {
    tool_name: &'a str,
    input_summary: &'a str,
    risk_description: &'a str,
    selected: usize,
    risk_level: RiskLevel,
}

impl<'a> PermissionDialog<'a> {
    pub fn new(tool_name: &'a str, input_summary: &'a str, risk_description: &'a str) -> Self {
        Self {
            tool_name,
            input_summary,
            risk_description,
            selected: 0,
            risk_level: RiskLevel::from_tool(tool_name),
        }
    }

    pub fn with_selected(mut self, selected: usize) -> Self {
        self.selected = selected.min(OPTIONS.len() - 1);
        self
    }

    /// Override the risk level (computed from tool name by default).
    pub fn with_risk_level(mut self, risk_level: RiskLevel) -> Self {
        self.risk_level = risk_level;
        self
    }

    /// Convenience setter kept for callers that only know whether an op is dangerous.
    /// Maps `true` → `High`, `false` → preserves current level (no downgrade).
    pub fn with_dangerous(mut self, is_dangerous: bool) -> Self {
        if is_dangerous {
            self.risk_level = RiskLevel::High;
        }
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
    #[allow(clippy::too_many_lines)]
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

        let border_color = self.risk_level.border_color();
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
            "file_read" | "read" | "file_write" | "write" | "file_edit" | "edit"
            | "multiedit" | "multi_edit" => "File:",
            "glob" | "grep" => "Pattern:",
            _ => "Input:",
        };

        lines.push(Line::from(Span::styled(
            format!("  {preview_label}"),
            Style::default().fg(crate::render::TRANSCRIPT_MUTED),
        )));

        // Top border of nested preview box.
        let box_fill = inner_width.saturating_sub(2);
        lines.push(Line::from(Span::styled(
            format!("  \u{250c}{}\u{2510}", "\u{2500}".repeat(box_fill)),
            Style::default().fg(crate::render::TRANSCRIPT_MUTED),
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
                Span::styled("  \u{2502} ".to_string(), Style::default().fg(crate::render::TRANSCRIPT_MUTED)),
                Span::styled(display, Style::default().fg(crate::render::TRANSCRIPT_TEXT)),
            ]));
        }
        // Show truncation indicator if input has more lines.
        let total_input_lines = self.input_summary.lines().count();
        if total_input_lines > 3 {
            lines.push(Line::from(Span::styled(
                format!("  \u{2502} ... ({} more lines)", total_input_lines - 3),
                Style::default().fg(crate::render::TRANSCRIPT_MUTED),
            )));
        }

        // Bottom border of nested preview box.
        lines.push(Line::from(Span::styled(
            format!("  \u{2514}{}\u{2518}", "\u{2500}".repeat(box_fill)),
            Style::default().fg(crate::render::TRANSCRIPT_MUTED),
        )));

        lines.push(Line::from(""));

        // Danger warning — shown for High risk level.
        if self.risk_level == RiskLevel::High {
            lines.push(Line::from(vec![
                Span::styled("  \u{26a0} ", Style::default().fg(crate::render::STATUS_RED)),
                Span::styled(
                    self.risk_description,
                    Style::default()
                        .fg(crate::render::STATUS_RED)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
        }

        // Options with hotkey badges.
        let selected_bg = crate::render::CLAUDE_ORANGE;
        for (i, (option, hotkey)) in OPTIONS.iter().zip(HOTKEYS.iter()).enumerate() {
            let (prefix, style) = if i == self.selected {
                ("\u{25b8} ", Style::default()
                    .fg(Color::Black)
                    .bg(selected_bg)
                    .add_modifier(Modifier::BOLD))
            } else {
                ("  ", Style::default().fg(crate::render::TRANSCRIPT_TEXT))
            };
            let hotkey_style = if i == self.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(selected_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(crate::render::STATUS_GREEN)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {prefix}"), style),
                Span::styled(format!("[{hotkey}] "), hotkey_style),
                Span::styled((*option).to_string(), style),
            ]));
        }

        lines.push(Line::from(""));

        // Footer: explicit keybinding hints so users know all options at a glance.
        let muted = crate::render::TRANSCRIPT_MUTED;
        let confirm_color = crate::render::STATUS_GREEN;
        let deny_color = crate::render::STATUS_RED;
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("\u{2191}\u{2193}", Style::default().fg(muted)),
            Span::styled(" nav  ", Style::default().fg(muted)),
            Span::styled("Enter", Style::default().fg(confirm_color)),
            Span::styled(" confirm  ", Style::default().fg(muted)),
            Span::styled("y", Style::default().fg(confirm_color)),
            Span::styled("/", Style::default().fg(muted)),
            Span::styled("a", Style::default().fg(confirm_color)),
            Span::styled("/", Style::default().fg(muted)),
            Span::styled("n", Style::default().fg(deny_color)),
            Span::styled("/", Style::default().fg(muted)),
            Span::styled("N", Style::default().fg(deny_color)),
            Span::styled("  ", Style::default().fg(muted)),
            Span::styled("Esc", Style::default().fg(deny_color)),
            Span::styled(" deny", Style::default().fg(muted)),
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

    #[test]
    fn risk_level_from_tool_bash_is_high() {
        assert_eq!(RiskLevel::from_tool("bash"), RiskLevel::High);
        assert_eq!(RiskLevel::from_tool("Bash"), RiskLevel::High);
    }

    #[test]
    fn risk_level_from_tool_read_is_low() {
        assert_eq!(RiskLevel::from_tool("read"), RiskLevel::Low);
        assert_eq!(RiskLevel::from_tool("glob"), RiskLevel::Low);
        assert_eq!(RiskLevel::from_tool("grep"), RiskLevel::Low);
    }

    #[test]
    fn risk_level_from_tool_write_is_moderate() {
        assert_eq!(RiskLevel::from_tool("write"), RiskLevel::Moderate);
        assert_eq!(RiskLevel::from_tool("edit"), RiskLevel::Moderate);
    }

    #[test]
    fn risk_level_border_colors() {
        assert_eq!(RiskLevel::Low.border_color(), crate::render::STATUS_GREEN);
        assert_eq!(RiskLevel::Moderate.border_color(), crate::render::STATUS_YELLOW);
        assert_eq!(RiskLevel::High.border_color(), crate::render::STATUS_RED);
    }

    #[test]
    fn with_dangerous_upgrades_to_high() {
        let dialog = PermissionDialog::new("read", "file.txt", "desc").with_dangerous(true);
        assert_eq!(dialog.risk_level, RiskLevel::High);
    }

    #[test]
    fn with_dangerous_false_keeps_computed_level() {
        // "read" → Low; with_dangerous(false) should not change it
        let dialog = PermissionDialog::new("read", "file.txt", "desc").with_dangerous(false);
        assert_eq!(dialog.risk_level, RiskLevel::Low);
    }

    #[test]
    fn read_tool_renders_green_border_without_panic() {
        let dialog = PermissionDialog::new("read", "/src/main.rs", "Read file");
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }
}
