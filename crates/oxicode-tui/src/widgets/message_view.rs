use oxicode_common::{ContentBlock, Message, Role};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use super::markdown_view;

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
///
/// Supports pre-rendered markdown streaming lines from `MarkdownStreamCollector`
/// and renders completed messages with full markdown formatting.
pub struct MessageView<'a> {
    messages: &'a [Message],
    /// Pre-rendered streaming lines from `MarkdownStreamCollector`.
    streaming_lines: Option<&'a [Line<'static>]>,
    /// Trailing incomplete line fragment (text after last `\n` in stream).
    streaming_tail: Option<&'a str>,
    /// Active tool calls during streaming.
    active_tools: &'a [ActiveToolInfo<'a>],
    scroll_offset: u16,
}

impl<'a> MessageView<'a> {
    pub fn new(
        messages: &'a [Message],
        streaming_lines: Option<&'a [Line<'static>]>,
        streaming_tail: Option<&'a str>,
        active_tools: &'a [ActiveToolInfo<'a>],
        scroll_offset: u16,
    ) -> Self {
        Self {
            messages,
            streaming_lines,
            streaming_tail,
            active_tools,
            scroll_offset,
        }
    }

    fn format_messages(&self) -> Text<'a> {
        let mut lines = Vec::new();

        for (i, msg) in self.messages.iter().enumerate() {
            if i > 0 {
                render_separator(&mut lines);
            }
            Self::render_message_header(msg, &mut lines);
            render_content_blocks(&msg.content, &mut lines);
        }

        // Streaming section.
        let has_streaming = self
            .streaming_lines
            .map_or(false, |l| !l.is_empty())
            || self.streaming_tail.is_some();
        if has_streaming {
            if !self.messages.is_empty() {
                render_separator(&mut lines);
            }
            // Show assistant header if last message isn't already assistant.
            if self
                .messages
                .last()
                .map_or(true, |m| m.role != Role::Assistant)
            {
                lines.push(Line::from(Span::styled(
                    "\u{25c0} OxiCode",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            self.render_streaming(&mut lines);
        }

        self.render_active_tools(&mut lines);

        Text::from(lines)
    }

    fn render_message_header(msg: &Message, lines: &mut Vec<Line<'a>>) {
        let (prefix, style) = match msg.role {
            Role::User => (
                "\u{25b6} You",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Role::Assistant => (
                "\u{25c0} OxiCode",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Role::System => ("\u{2699} System", Style::default().fg(Color::Yellow)),
        };
        lines.push(Line::from(Span::styled(prefix, style)));
    }

    fn render_streaming(&self, lines: &mut Vec<Line<'a>>) {
        // Render pre-committed markdown lines.
        if let Some(committed) = self.streaming_lines {
            for line in committed {
                // Indent each committed line by 2 spaces.
                let mut spans = vec![Span::raw("  ")];
                spans.extend(line.spans.iter().cloned());
                lines.push(Line::from(spans));
            }
        }
        // Render trailing incomplete fragment as plain text.
        if let Some(tail) = self.streaming_tail {
            if !tail.is_empty() {
                lines.push(Line::from(format!("  {tail}")));
            }
        }
        // Blinking cursor.
        lines.push(Line::from(Span::styled(
            "  \u{258d}",
            Style::default().fg(Color::Cyan),
        )));
    }

    fn render_active_tools(&self, lines: &mut Vec<Line<'a>>) {
        for tool in self.active_tools {
            match tool.result {
                None => {
                    // Running: spinner icon + tool name + input summary.
                    lines.push(Line::from(vec![
                        Span::styled("  \u{27f3} ", Style::default().fg(Color::Yellow)),
                        Span::styled(
                            tool.name.to_string(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(" \u{2500} {}", tool.input_summary),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }
                Some((content, is_error)) => {
                    let (icon, color) = if is_error {
                        ("\u{2717}", Color::Red)
                    } else {
                        ("\u{2713}", Color::Green)
                    };
                    // Status icon + tool name.
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {icon} "), Style::default().fg(color)),
                        Span::styled(
                            tool.name.to_string(),
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(" \u{2500} {}", tool.input_summary),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                    // Show first MAX_RESULT_LINES of output.
                    let result_style = Style::default().fg(Color::DarkGray);
                    let total_lines = content.lines().count();
                    for (i, line) in content.lines().enumerate() {
                        if i >= MAX_RESULT_LINES {
                            lines.push(Line::from(Span::styled(
                                format!("    ... ({} more lines)", total_lines - i),
                                result_style,
                            )));
                            break;
                        }
                        lines.push(Line::from(Span::styled(
                            format!("    {line}"),
                            result_style,
                        )));
                    }
                }
            }
        }
    }

    /// Maximum safe vertical scroll offset for this content in `area`.
    pub fn max_scroll_offset(&self, area: Rect) -> u16 {
        let text = self.format_messages();
        max_content_scroll(area, text.lines.len())
    }
}

/// Render a horizontal separator line between messages.
fn render_separator(lines: &mut Vec<Line<'_>>) {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "\u{2500}".repeat(50),
        Style::default().fg(Color::Rgb(60, 60, 60)),
    )));
    lines.push(Line::from(""));
}

/// Render content blocks (text, tool use, tool result, thinking) into lines.
fn render_content_blocks(blocks: &[ContentBlock], lines: &mut Vec<Line<'_>>) {
    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                // Render text through markdown parser for styled output.
                let md_lines = markdown_view::parse_to_owned_lines(text);
                for md_line in md_lines {
                    let mut spans = vec![Span::raw("  ")];
                    spans.extend(md_line.spans);
                    lines.push(Line::from(spans));
                }
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let summary = tool_input_summary(input);
                lines.push(Line::from(vec![
                    Span::styled("  \u{27f3} ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        name.to_string(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" \u{2500} {summary}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                let (icon, color) = if *is_error {
                    ("\u{2717}", Color::Red)
                } else {
                    ("\u{2713}", Color::Green)
                };
                let tag = if *is_error { "error" } else { "result" };
                lines.push(Line::from(Span::styled(
                    format!("  {icon} [{tag}]"),
                    Style::default().fg(color),
                )));
                let result_style = Style::default().fg(Color::DarkGray);
                let total_lines = content.lines().count();
                for (i, line) in content.lines().enumerate() {
                    if i >= MAX_RESULT_LINES {
                        lines.push(Line::from(Span::styled(
                            format!("    ... ({} more lines)", total_lines - i),
                            result_style,
                        )));
                        break;
                    }
                    lines.push(Line::from(Span::styled(
                        format!("    {line}"),
                        result_style,
                    )));
                }
            }
            ContentBlock::Thinking { thinking } => {
                // Show first 2 lines of thinking block.
                lines.push(Line::from(Span::styled(
                    "  \u{1f4ad} thinking...",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
                for (i, line) in thinking.lines().enumerate() {
                    if i >= 2 {
                        let remaining = thinking.lines().count() - 2;
                        if remaining > 0 {
                            lines.push(Line::from(Span::styled(
                                format!("    ... ({remaining} more lines)"),
                                Style::default().fg(Color::DarkGray),
                            )));
                        }
                        break;
                    }
                    lines.push(Line::from(Span::styled(
                        format!("    {line}"),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
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
        let scroll_y = resolve_scroll_offset(self.scroll_offset, area, text.lines.len());

        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0));

        paragraph.render(area, buf);
    }
}

/// Convert requested scroll offset into a safe value for ratatui.
///
/// `u16::MAX` may be used by callers as an auto-scroll sentinel ("jump to
/// bottom"). Passing it directly to `Paragraph::scroll` can overflow internal
/// math (`area.height + scroll_y`) and panic. This helper resolves sentinel
/// values and clamps all offsets into a safe, visible range.
fn resolve_scroll_offset(requested: u16, area: Rect, line_count: usize) -> u16 {
    let max_content_scroll = max_content_scroll(area, line_count);

    let desired = if requested == u16::MAX {
        max_content_scroll
    } else {
        requested.min(max_content_scroll)
    };

    // Paragraph internally computes `area.height + scroll_y` (u16), so clamp to
    // prevent integer overflow even for very small/large terminal sizes.
    desired.min(u16::MAX.saturating_sub(area.height))
}

fn max_content_scroll(area: Rect, line_count: usize) -> u16 {
    let viewport_height = area.height.saturating_sub(2); // account for Block borders
    let content_height = u16::try_from(line_count).unwrap_or(u16::MAX);
    content_height.saturating_sub(viewport_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_text_message(text: &str) -> Message {
        let mut msg = Message::assistant();
        msg.content.push(ContentBlock::Text {
            text: text.to_string(),
        });
        msg
    }

    #[test]
    fn test_resolve_scroll_offset_auto_scroll_sentinel() {
        let area = Rect::new(0, 0, 80, 10);
        let scroll = resolve_scroll_offset(u16::MAX, area, 3);
        assert_eq!(scroll, 0);
    }

    #[test]
    fn test_render_with_max_scroll_offset_does_not_panic() {
        let messages = vec![Message::user("hi"), assistant_text_message("hello")];
        let active_tools: &[ActiveToolInfo<'_>] = &[];
        let streaming_lines: Vec<Line<'static>> = vec![Line::from("streaming...")];
        let widget = MessageView::new(
            &messages,
            Some(&streaming_lines),
            None,
            active_tools,
            u16::MAX,
        );
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            widget.render(area, &mut buf);
        }));

        assert!(result.is_ok(), "render should not panic with max scroll");
    }

    #[test]
    fn test_message_separator_between_messages() {
        let messages = vec![
            Message::user("hello"),
            assistant_text_message("world"),
        ];
        let active_tools: &[ActiveToolInfo<'_>] = &[];
        let view = MessageView::new(&messages, None, None, active_tools, 0);
        let text = view.format_messages();
        // Should contain separator character.
        let has_separator = text.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|s| s.content.contains('\u{2500}'))
        });
        assert!(has_separator, "Should have separator between messages");
    }

    #[test]
    fn test_markdown_rendered_in_content_blocks() {
        let messages = vec![assistant_text_message("**bold text** and `code`")];
        let active_tools: &[ActiveToolInfo<'_>] = &[];
        let view = MessageView::new(&messages, None, None, active_tools, 0);
        let text = view.format_messages();
        // Bold markers should not appear in rendered output.
        let raw: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            !raw.contains("**"),
            "Bold markers should be parsed, got: {raw}"
        );
        assert!(raw.contains("bold text"));
    }
}
