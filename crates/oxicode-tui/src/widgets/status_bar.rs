use oxicode_common::Usage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

/// Status bar showing model info, provider, token count, cost, and streaming state.
pub struct StatusBar<'a> {
    model: &'a str,
    provider: &'a str,
    usage: &'a Usage,
    is_streaming: bool,
    mcp_server_count: usize,
    session_name: &'a str,
}

impl<'a> StatusBar<'a> {
    pub fn new(model: &'a str, usage: &'a Usage, is_streaming: bool) -> Self {
        Self {
            model,
            provider: "",
            usage,
            is_streaming,
            mcp_server_count: 0,
            session_name: "",
        }
    }

    /// Set the provider name for display.
    pub fn with_provider(mut self, provider: &'a str) -> Self {
        self.provider = provider;
        self
    }

    /// Set the MCP server count indicator.
    pub fn with_mcp_count(mut self, count: usize) -> Self {
        self.mcp_server_count = count;
        self
    }

    /// Set session name for display.
    pub fn with_session(mut self, name: &'a str) -> Self {
        self.session_name = name;
        self
    }
}

/// Map provider name to a color for visual distinction.
fn provider_color(provider: &str) -> Color {
    match provider {
        "anthropic" => Color::Rgb(217, 119, 62), // Orange (Claude brand)
        "openai" => Color::Rgb(116, 170, 156),   // Teal
        "ollama" => Color::Rgb(150, 150, 150),   // Gray (local)
        "deepseek" => Color::Rgb(100, 149, 237), // Cornflower blue
        "azure" => Color::Rgb(0, 120, 212),      // Azure blue
        "openrouter" => Color::Rgb(168, 85, 247), // Purple
        _ => Color::White,
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Streaming indicator.
        let status = if self.is_streaming {
            Span::styled(
                " \u{25cf} Streaming ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(" \u{25cb} Ready ", Style::default().fg(Color::DarkGray))
        };

        // Provider badge (color-coded).
        let provider_label = if self.provider.is_empty() {
            String::new()
        } else {
            format!(" {} ", self.provider)
        };
        let provider_span = Span::styled(
            &provider_label,
            Style::default()
                .fg(Color::Black)
                .bg(provider_color(self.provider)),
        );

        // Model name.
        let model = Span::styled(
            format!(" {} ", self.model),
            Style::default().fg(Color::White).bg(Color::DarkGray),
        );

        // Token count.
        let tokens = Span::styled(
            format!(
                " \u{2191}{} \u{2193}{} ",
                self.usage.input_tokens, self.usage.output_tokens
            ),
            Style::default().fg(Color::Yellow),
        );

        // Cost estimate (rough, based on Claude Sonnet pricing).
        let cost = f64::from(self.usage.input_tokens) * 3.0 / 1_000_000.0
            + f64::from(self.usage.output_tokens) * 15.0 / 1_000_000.0;
        let cost_span = Span::styled(format!(" ${cost:.4} "), Style::default().fg(Color::Cyan));

        // MCP indicator.
        let mcp_span = if self.mcp_server_count > 0 {
            Span::styled(
                format!(" MCP:{} ", self.mcp_server_count),
                Style::default().fg(Color::Magenta),
            )
        } else {
            Span::raw("")
        };

        // Session name (truncated).
        let session_span = if self.session_name.is_empty() {
            Span::raw("")
        } else {
            let short: String = self.session_name.chars().take(8).collect();
            Span::styled(format!(" [{short}] "), Style::default().fg(Color::DarkGray))
        };

        let line = Line::from(vec![
            status,
            provider_span,
            model,
            tokens,
            cost_span,
            mcp_span,
            session_span,
        ]);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
