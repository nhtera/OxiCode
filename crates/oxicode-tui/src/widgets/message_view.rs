use oxicode_common::{Message, Role};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

/// Widget to display the conversation message list.
pub struct MessageView<'a> {
    messages: &'a [Message],
    /// Current streaming text (if any).
    streaming_text: Option<&'a str>,
    scroll_offset: u16,
}

impl<'a> MessageView<'a> {
    pub fn new(
        messages: &'a [Message],
        streaming_text: Option<&'a str>,
        scroll_offset: u16,
    ) -> Self {
        Self {
            messages,
            streaming_text,
            scroll_offset,
        }
    }

    fn format_messages(&self) -> Text<'a> {
        let mut lines = Vec::new();

        for msg in self.messages {
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

            let text = msg.text();
            for line in text.lines() {
                lines.push(Line::from(format!("  {line}")));
            }
            lines.push(Line::from(""));
        }

        // Show streaming text if active
        if let Some(streaming) = self.streaming_text {
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

        Text::from(lines)
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
