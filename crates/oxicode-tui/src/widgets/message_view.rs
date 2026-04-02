use oxicode_common::{ContentBlock, Message, Role};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

/// Maximum lines of tool result output shown inline.
const MAX_RESULT_LINES: usize = 5;

/// Snapshot of an active tool call for rendering.
pub struct ActiveToolInfo<'a> {
    pub name: &'a str,
    pub input_summary: &'a str,
    /// `Some((content, is_error))` when the tool completed.
    pub result: Option<(&'a str, bool)>,
}

/// Widget to display the conversation message list.
pub struct MessageView<'a> {
    messages: &'a [Message],
    /// Current streaming text (if any).
    streaming_text: Option<&'a str>,
    /// Active tool calls during streaming.
    active_tools: &'a [ActiveToolInfo<'a>],
    scroll_offset: u16,
}

impl<'a> MessageView<'a> {
    pub fn new(
        messages: &'a [Message],
        streaming_text: Option<&'a str>,
        active_tools: &'a [ActiveToolInfo<'a>],
        scroll_offset: u16,
    ) -> Self {
        Self {
            messages,
            streaming_text,
            active_tools,
            scroll_offset,
        }
    }

    fn format_messages(&self) -> Text<'a> {
        let mut lines = Vec::new();

        for msg in self.messages {
            Self::render_message_header(msg, &mut lines);
            render_content_blocks(&msg.content, &mut lines);
            lines.push(Line::from(""));
        }

        self.render_streaming(&mut lines);
        self.render_active_tools(&mut lines);

        Text::from(lines)
    }

    fn render_message_header(msg: &Message, lines: &mut Vec<Line<'a>>) {
        let (prefix, style) = match msg.role {
            Role::User => (
                "▶ You",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Role::Assistant => (
                "◀ OxiCode",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Role::System => ("⚙ System", Style::default().fg(Color::Yellow)),
        };
        lines.push(Line::from(Span::styled(prefix, style)));
    }

    fn render_streaming(&self, lines: &mut Vec<Line<'a>>) {
        let Some(streaming) = self.streaming_text else {
            return;
        };
        if self
            .messages
            .last()
            .map_or(true, |m| m.role != Role::Assistant)
        {
            lines.push(Line::from(Span::styled(
                "◀ OxiCode",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        for line in streaming.lines() {
            lines.push(Line::from(format!("  {line}")));
        }
        lines.push(Line::from(Span::styled(
            "  ▍",
            Style::default().fg(Color::Cyan),
        )));
    }

    fn render_active_tools(&self, lines: &mut Vec<Line<'a>>) {
        for tool in self.active_tools {
            match tool.result {
                None => {
                    lines.push(Line::from(Span::styled(
                        format!("  [running] {}: {}", tool.name, tool.input_summary),
                        Style::default().fg(Color::Yellow),
                    )));
                }
                Some((content, is_error)) => {
                    let color = if is_error { Color::Red } else { Color::Green };
                    let icon = if is_error { "error" } else { "done" };
                    lines.push(Line::from(Span::styled(
                        format!("  [{icon}] {}: {}", tool.name, truncate_str(content, 60)),
                        Style::default().fg(color),
                    )));
                }
            }
        }
    }
}

/// Render content blocks (text, tool use, tool result, thinking) into lines.
fn render_content_blocks(blocks: &[ContentBlock], lines: &mut Vec<Line<'_>>) {
    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                for line in text.lines() {
                    lines.push(Line::from(format!("  {line}")));
                }
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let summary = tool_input_summary(input);
                lines.push(Line::from(Span::styled(
                    format!("  [tool] {name}: {summary}"),
                    Style::default().fg(Color::Yellow),
                )));
            }
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                let color = if *is_error { Color::Red } else { Color::Green };
                let tag = if *is_error { "error" } else { "result" };
                for (i, line) in content.lines().enumerate() {
                    if i >= MAX_RESULT_LINES {
                        lines.push(Line::from(Span::styled(
                            format!("  ... ({} more lines)", content.lines().count() - i),
                            Style::default().fg(Color::DarkGray),
                        )));
                        break;
                    }
                    let text = if i == 0 {
                        format!("  [{tag}] {line}")
                    } else {
                        format!("  {line}")
                    };
                    lines.push(Line::from(Span::styled(text, Style::default().fg(color))));
                }
            }
            ContentBlock::Thinking { thinking } => {
                let preview = truncate_str(thinking, 60);
                lines.push(Line::from(Span::styled(
                    format!("  [thinking] {preview}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
}

/// Summarize tool input JSON for inline display.
fn tool_input_summary(input: &serde_json::Value) -> String {
    let raw = if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
        cmd.to_string()
    } else if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
        path.to_string()
    } else if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        format!("{pattern} in {path}")
    } else {
        serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())
    };
    truncate_str(&raw, 80)
}

/// Truncate a string to `max` characters, appending "..." if truncated.
fn truncate_str(s: &str, max: usize) -> String {
    if let Some((idx, _)) = s.char_indices().nth(max) {
        format!("{}...", &s[..idx])
    } else {
        s.to_string()
    }
}

impl Widget for MessageView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Conversation ");

        let text = self.format_messages();

        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        paragraph.render(area, buf);
    }
}
