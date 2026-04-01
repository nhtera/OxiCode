use oxicode_common::Usage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

/// Status bar showing model info, token count, and streaming state.
pub struct StatusBar<'a> {
    model: &'a str,
    usage: &'a Usage,
    is_streaming: bool,
}

impl<'a> StatusBar<'a> {
    pub fn new(model: &'a str, usage: &'a Usage, is_streaming: bool) -> Self {
        Self {
            model,
            usage,
            is_streaming,
        }
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let status = if self.is_streaming {
            Span::styled(
                " ● Streaming ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(" ○ Ready ", Style::default().fg(Color::DarkGray))
        };

        let model = Span::styled(
            format!(" {} ", self.model),
            Style::default().fg(Color::White).bg(Color::DarkGray),
        );

        let tokens = Span::styled(
            format!(
                " ↑{} ↓{} tokens ",
                self.usage.input_tokens, self.usage.output_tokens
            ),
            Style::default().fg(Color::Yellow),
        );

        let line = Line::from(vec![status, model, tokens]);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
