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
    #[allow(clippy::match_same_arms)]
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

/// Tool-specific dialog variant — drives content and option layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDialogKind {
    /// Default 4-option layout for unknown tools.
    Generic,
    /// Shell command — show command in bordered code block.
    Bash { command: String },
    /// Read-only file access — simpler 3-option layout.
    FileRead { path: String },
    /// File mutation (write/edit) — show file path, 4 options.
    FileWrite { path: String },
}

impl PermissionDialogKind {
    /// Detect the dialog kind from tool name and raw input summary.
    pub fn detect(tool_name: &str, input_summary: &str) -> Self {
        match tool_name.to_lowercase().as_str() {
            "bash" => Self::Bash {
                command: input_summary.to_string(),
            },
            "read" | "file_read" => Self::FileRead {
                path: input_summary.to_string(),
            },
            "write" | "file_write" | "edit" | "file_edit" | "multiedit" | "multi_edit" => {
                Self::FileWrite {
                    path: input_summary.to_string(),
                }
            }
            _ => Self::Generic,
        }
    }

    /// Number of selectable options for this dialog kind.
    pub fn option_count(&self) -> usize {
        match self {
            Self::FileRead { .. } => 3, // allow once, allow session, deny
            Self::Bash { .. } => 5, // allow once, allow session, prefix allow, deny, always deny
            _ => 4,                 // allow once, allow session, deny, always deny
        }
    }

    /// Option labels for this dialog kind.
    fn options(&self) -> &[&str] {
        match self {
            Self::FileRead { .. } => &["Allow once", "Allow for session", "Deny"],
            Self::Bash { .. } => &[
                "Allow once",
                "Allow for session",
                "Allow prefix for session",
                "Deny",
                "Always deny",
            ],
            _ => &["Allow once", "Allow for session", "Deny", "Always deny"],
        }
    }

    /// Hotkey labels matching options.
    fn hotkeys(&self) -> &[&str] {
        match self {
            Self::FileRead { .. } => &["y", "a", "n"],
            Self::Bash { .. } => &["y", "a", "p", "n", "N"],
            _ => &["y", "a", "n", "N"],
        }
    }

    /// Preview label shown above the content box.
    fn preview_label(&self) -> &str {
        match self {
            Self::Bash { .. } => "Command:",
            Self::FileRead { .. } | Self::FileWrite { .. } => "File:",
            Self::Generic => "Input:",
        }
    }

    /// Content text to display in the preview box.
    fn preview_content<'a>(&'a self, fallback: &'a str) -> &'a str {
        match self {
            Self::Bash { command } => command.as_str(),
            Self::FileRead { path } | Self::FileWrite { path } => path.as_str(),
            Self::Generic => fallback,
        }
    }
}

/// Overlay dialog for permission approval/denial.
///
/// Shows tool name, command/input preview in a nested box,
/// danger warning for risky operations, countdown timer, and keyboard shortcuts.
pub struct PermissionDialog<'a> {
    tool_name: &'a str,
    input_summary: &'a str,
    risk_description: &'a str,
    selected: usize,
    risk_level: RiskLevel,
    kind: &'a PermissionDialogKind,
    /// Remaining seconds before auto-deny (None = no countdown).
    countdown_secs: Option<u32>,
}

/// Default kind used when caller doesn't provide one.
const DEFAULT_KIND: PermissionDialogKind = PermissionDialogKind::Generic;

impl<'a> PermissionDialog<'a> {
    pub fn new(tool_name: &'a str, input_summary: &'a str, risk_description: &'a str) -> Self {
        Self {
            tool_name,
            input_summary,
            risk_description,
            selected: 0,
            risk_level: RiskLevel::from_tool(tool_name),
            kind: &DEFAULT_KIND,
            countdown_secs: None,
        }
    }

    pub fn with_selected(mut self, selected: usize) -> Self {
        self.selected = selected.min(self.kind.option_count().saturating_sub(1));
        self
    }

    /// Override the risk level (computed from tool name by default).
    pub fn with_risk_level(mut self, risk_level: RiskLevel) -> Self {
        self.risk_level = risk_level;
        self
    }

    /// Set the dialog kind for tool-specific rendering.
    pub fn with_kind(mut self, kind: &'a PermissionDialogKind) -> Self {
        self.kind = kind;
        // Re-clamp selection to new option count.
        self.selected = self.selected.min(kind.option_count().saturating_sub(1));
        self
    }

    /// Set countdown timer remaining seconds.
    pub fn with_countdown(mut self, remaining_secs: u32) -> Self {
        self.countdown_secs = Some(remaining_secs);
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

impl Widget for PermissionDialog<'_> {
    #[allow(clippy::too_many_lines)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Responsive width: prefer 40..70 but never exceed terminal.
        let dialog_width = 70u16
            .min(area.width.saturating_sub(2))
            .max(40)
            .min(area.width);
        let dialog_height = 22u16.min(area.height.saturating_sub(2)).min(area.height);
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
        let preview_label = self.kind.preview_label();
        let preview_text = self.kind.preview_content(self.input_summary);

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
        let preview_lines: Vec<&str> = preview_text.lines().take(3).collect();
        // Bash commands get highlighted text color.
        let content_color = if matches!(self.kind, PermissionDialogKind::Bash { .. }) {
            crate::render::CLAUDE_ORANGE
        } else {
            crate::render::TRANSCRIPT_TEXT
        };
        for pline in &preview_lines {
            let display = if pline.chars().count() > preview_width {
                // Char-safe truncation — avoid panicking on multi-byte UTF-8.
                let truncated: String = pline
                    .chars()
                    .take(preview_width.saturating_sub(3))
                    .collect();
                format!("{truncated}...")
            } else {
                (*pline).to_string()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    "  \u{2502} ".to_string(),
                    Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                ),
                Span::styled(display, Style::default().fg(content_color)),
            ]));
        }
        // Show truncation indicator if input has more lines.
        let total_input_lines = preview_text.lines().count();
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

        // Danger warning — shown for High risk level with context-specific text.
        if self.risk_level == RiskLevel::High {
            let danger_text = danger_explanation(self.kind, self.risk_description);
            lines.push(Line::from(vec![
                Span::styled(
                    "  \u{26a0} ",
                    Style::default().fg(crate::render::STATUS_RED),
                ),
                Span::styled(
                    danger_text,
                    Style::default()
                        .fg(crate::render::STATUS_RED)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
        }

        // Options with hotkey badges.
        let options = self.kind.options();
        let hotkeys = self.kind.hotkeys();
        let selected_bg = crate::render::CLAUDE_ORANGE;
        for (i, (option, hotkey)) in options.iter().zip(hotkeys.iter()).enumerate() {
            let (prefix, style) = if i == self.selected {
                (
                    "\u{25b8} ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(selected_bg)
                        .add_modifier(Modifier::BOLD),
                )
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

        // Footer: countdown timer + keybinding hints.
        render_footer(&mut lines, self.countdown_secs, self.kind);

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(dialog_area, buf);
    }
}

/// Build context-specific danger explanation text.
fn danger_explanation(kind: &PermissionDialogKind, fallback: &str) -> String {
    match kind {
        PermissionDialogKind::Bash { command } => {
            let lower = command.to_lowercase();
            if lower.contains("rm -rf") || lower.contains("rm -r") || lower.contains("rmdir") {
                "Command will delete files or directories".to_string()
            } else if lower.contains("sudo ") {
                "Command requires elevated privileges".to_string()
            } else if lower.contains("git push --force") || lower.contains("git reset --hard") {
                "Command may cause irreversible git changes".to_string()
            } else if lower.contains("chmod") || lower.contains("chown") {
                "Command modifies file permissions".to_string()
            } else {
                "Command contains potentially destructive operations".to_string()
            }
        }
        PermissionDialogKind::FileWrite { path } => {
            let sensitive = ["/etc/", "/usr/", "/bin/", ".env", "credentials", ".ssh/"];
            if sensitive.iter().any(|s| path.contains(s)) {
                "Writing to a sensitive system path".to_string()
            } else {
                "This operation may modify or delete files".to_string()
            }
        }
        _ => {
            if fallback.is_empty() {
                "This operation may have side effects".to_string()
            } else {
                fallback.to_string()
            }
        }
    }
}

/// Render footer with countdown timer and keybinding hints.
fn render_footer(
    lines: &mut Vec<Line<'_>>,
    countdown_secs: Option<u32>,
    kind: &PermissionDialogKind,
) {
    let muted = crate::render::TRANSCRIPT_MUTED;
    let confirm_color = crate::render::STATUS_GREEN;
    let deny_color = crate::render::STATUS_RED;

    // Countdown timer line (shown above keybinding hints when active).
    if let Some(secs) = countdown_secs {
        let timer_color = if secs <= 5 {
            crate::render::STATUS_RED
        } else if secs <= 10 {
            crate::render::STATUS_YELLOW
        } else {
            muted
        };
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("Auto-deny in {secs}s"),
                Style::default()
                    .fg(timer_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Keybinding hints.
    let mut hints = vec![
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
    ];

    // "N" hotkey only for non-FileRead dialogs (they have no "Always deny").
    if !matches!(kind, PermissionDialogKind::FileRead { .. }) {
        hints.extend([
            Span::styled("/", Style::default().fg(muted)),
            Span::styled("N", Style::default().fg(deny_color)),
        ]);
    }

    hints.extend([
        Span::styled("  ", Style::default().fg(muted)),
        Span::styled("Esc", Style::default().fg(deny_color)),
        Span::styled(" deny", Style::default().fg(muted)),
    ]);

    lines.push(Line::from(hints));
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
        assert_eq!(dialog.selected, DEFAULT_KIND.option_count() - 1);
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
        let long_input = (0..10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
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
        assert_eq!(
            RiskLevel::Moderate.border_color(),
            crate::render::STATUS_YELLOW
        );
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

    // --- New tests for Phase 01 features ---

    #[test]
    fn dialog_kind_detect_bash() {
        let kind = PermissionDialogKind::detect("bash", "echo hello");
        assert!(matches!(kind, PermissionDialogKind::Bash { command } if command == "echo hello"));
    }

    #[test]
    fn dialog_kind_detect_file_read() {
        let kind = PermissionDialogKind::detect("read", "/src/main.rs");
        assert!(matches!(kind, PermissionDialogKind::FileRead { path } if path == "/src/main.rs"));
    }

    #[test]
    fn dialog_kind_detect_file_write() {
        let kind = PermissionDialogKind::detect("write", "/tmp/out.txt");
        assert!(matches!(kind, PermissionDialogKind::FileWrite { path } if path == "/tmp/out.txt"));
    }

    #[test]
    fn dialog_kind_detect_generic() {
        let kind = PermissionDialogKind::detect("custom_tool", "data");
        assert!(matches!(kind, PermissionDialogKind::Generic));
    }

    #[test]
    fn file_read_has_3_options() {
        let kind = PermissionDialogKind::FileRead {
            path: "/test".to_string(),
        };
        assert_eq!(kind.option_count(), 3);
        assert_eq!(kind.options().len(), 3);
        assert_eq!(kind.hotkeys().len(), 3);
    }

    #[test]
    fn bash_has_5_options() {
        let kind = PermissionDialogKind::Bash {
            command: "echo hi".to_string(),
        };
        assert_eq!(kind.option_count(), 5);
    }

    #[test]
    fn dialog_with_kind_renders_without_panic() {
        let kind = PermissionDialogKind::Bash {
            command: "cargo test".to_string(),
        };
        let dialog = PermissionDialog::new("bash", "cargo test", "Allow?").with_kind(&kind);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }

    #[test]
    fn dialog_with_countdown_renders_without_panic() {
        let dialog = PermissionDialog::new("bash", "cmd", "Allow?").with_countdown(25);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }

    #[test]
    fn dialog_with_low_countdown_renders_without_panic() {
        let dialog = PermissionDialog::new("bash", "rm -rf /", "Danger!").with_countdown(3);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }

    #[test]
    fn file_read_kind_clamps_selection() {
        let kind = PermissionDialogKind::FileRead {
            path: "/test".to_string(),
        };
        let dialog = PermissionDialog::new("read", "/test", "Read")
            .with_kind(&kind)
            .with_selected(99);
        assert_eq!(dialog.selected, 2); // max index = 2 for 3 options
    }

    #[test]
    fn danger_explanation_rm_rf() {
        let kind = PermissionDialogKind::Bash {
            command: "rm -rf /tmp".to_string(),
        };
        let text = danger_explanation(&kind, "");
        assert!(text.contains("delete"));
    }

    #[test]
    fn danger_explanation_sudo() {
        let kind = PermissionDialogKind::Bash {
            command: "sudo apt install".to_string(),
        };
        let text = danger_explanation(&kind, "");
        assert!(text.contains("elevated"));
    }

    #[test]
    fn danger_explanation_sensitive_file() {
        let kind = PermissionDialogKind::FileWrite {
            path: "/etc/passwd".to_string(),
        };
        let text = danger_explanation(&kind, "");
        assert!(text.contains("sensitive"));
    }

    #[test]
    fn danger_explanation_generic_fallback() {
        let text = danger_explanation(&PermissionDialogKind::Generic, "Custom warning");
        assert_eq!(text, "Custom warning");
    }
}
