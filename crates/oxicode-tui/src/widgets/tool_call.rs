use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

/// Status of a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    Running,
    Success,
    Error,
}

/// Collapsible widget displaying a tool call with its input and output.
pub struct ToolCallWidget<'a> {
    tool_name: &'a str,
    input_summary: &'a str,
    output: Option<&'a str>,
    status: ToolCallStatus,
    collapsed: bool,
}

impl<'a> ToolCallWidget<'a> {
    pub fn new(
        tool_name: &'a str,
        input_summary: &'a str,
        output: Option<&'a str>,
        status: ToolCallStatus,
        collapsed: bool,
    ) -> Self {
        Self {
            tool_name,
            input_summary,
            output,
            status,
            collapsed,
        }
    }
}

impl Widget for ToolCallWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (status_icon, status_color) = match self.status {
            ToolCallStatus::Running => ("⟳", Color::Yellow),
            ToolCallStatus::Success => ("✓", Color::Green),
            ToolCallStatus::Error => ("✗", Color::Red),
        };

        let collapse_icon = if self.collapsed { "▶" } else { "▼" };

        let title = format!(" {collapse_icon} {status_icon} {} ", self.tool_name);

        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(status_color))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ));

        let mut lines = Vec::new();

        // Input summary (always shown).
        let input_style = Style::default().fg(Color::DarkGray);
        let summary = if self.input_summary.chars().count() > 80 {
            let idx = self
                .input_summary
                .char_indices()
                .nth(77)
                .map_or(self.input_summary.len(), |(i, _)| i);
            format!("{}...", &self.input_summary[..idx])
        } else {
            self.input_summary.to_string()
        };
        lines.push(Line::from(Span::styled(summary, input_style)));

        // Output (shown when expanded).
        if !self.collapsed {
            if let Some(output) = self.output {
                lines.push(Line::from(""));
                let max_output_lines = 20;
                for (i, line) in output.lines().enumerate() {
                    if i >= max_output_lines {
                        lines.push(Line::from(Span::styled(
                            format!("... ({} more lines)", output.lines().count() - i),
                            Style::default().fg(Color::DarkGray),
                        )));
                        break;
                    }
                    lines.push(Line::from(line.to_string()));
                }
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_to_string(widget: ToolCallWidget, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(widget, f.area());
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..height {
            for x in 0..width {
                s.push(
                    buf.cell((x, y))
                        .unwrap()
                        .symbol()
                        .chars()
                        .next()
                        .unwrap_or(' '),
                );
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn running_tool_shows_spinner_icon() {
        let w = ToolCallWidget::new("bash", "echo hello", None, ToolCallStatus::Running, false);
        let rendered = render_to_string(w, 50, 5);
        assert!(rendered.contains("bash"), "Should show tool name");
        assert!(rendered.contains("⟳"), "Running should show ⟳");
    }

    #[test]
    fn success_tool_shows_check_icon() {
        let w = ToolCallWidget::new(
            "file_read",
            "/tmp/test.txt",
            Some("file content here"),
            ToolCallStatus::Success,
            false,
        );
        let rendered = render_to_string(w, 60, 8);
        assert!(rendered.contains("✓"), "Success should show ✓");
        assert!(rendered.contains("file_read"), "Should show tool name");
    }

    #[test]
    fn error_tool_shows_cross_icon() {
        let w = ToolCallWidget::new(
            "bash",
            "rm -rf /",
            Some("Permission denied"),
            ToolCallStatus::Error,
            false,
        );
        let rendered = render_to_string(w, 60, 8);
        assert!(rendered.contains("✗"), "Error should show ✗");
    }

    #[test]
    fn collapsed_hides_output() {
        let w = ToolCallWidget::new(
            "bash",
            "ls",
            Some("file1.txt\nfile2.txt"),
            ToolCallStatus::Success,
            true,
        );
        let rendered = render_to_string(w, 60, 4);
        assert!(rendered.contains("▶"), "Collapsed should show ▶");
        // Output should be hidden when collapsed.
        assert!(
            !rendered.contains("file1.txt"),
            "Output hidden when collapsed"
        );
    }

    #[test]
    fn expanded_shows_output() {
        let w = ToolCallWidget::new(
            "bash",
            "ls",
            Some("file1.txt\nfile2.txt"),
            ToolCallStatus::Success,
            false,
        );
        let rendered = render_to_string(w, 60, 8);
        assert!(rendered.contains("▼"), "Expanded should show ▼");
        assert!(rendered.contains("file1.txt"), "Output shown when expanded");
    }

    #[test]
    fn long_input_truncated() {
        let long_input = "a".repeat(100);
        let w = ToolCallWidget::new("bash", &long_input, None, ToolCallStatus::Running, false);
        let rendered = render_to_string(w, 90, 4);
        assert!(
            rendered.contains("..."),
            "Long input should be truncated with ..."
        );
    }
}
