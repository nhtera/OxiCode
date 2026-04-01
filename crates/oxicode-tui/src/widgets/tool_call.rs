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
        let summary = if self.input_summary.len() > 80 {
            format!("{}...", &self.input_summary[..77])
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
