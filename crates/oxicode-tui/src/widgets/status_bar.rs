use oxicode_common::Usage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

/// Status bar showing model info, provider, token count, cost, streaming state, and auth.
pub struct StatusBar<'a> {
    model: &'a str,
    provider: &'a str,
    usage: &'a Usage,
    is_streaming: bool,
    mcp_server_count: usize,
    session_name: &'a str,
    /// Vim mode badge (e.g. "N", "I", "V", "C") or empty if disabled.
    vim_badge: &'a str,
    /// Auth status label (e.g. "⚡ user@example.com", "🔑 sk-...XXXX", or empty).
    auth_label: &'a str,
    /// Voice mode indicator (e.g. "listening", "processing", or empty if off).
    voice_status: &'a str,
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
            vim_badge: "",
            auth_label: "",
            voice_status: "",
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

    /// Set vim mode badge for display (e.g. "N", "I", "V", "C").
    pub fn with_vim_badge(mut self, badge: &'a str) -> Self {
        self.vim_badge = badge;
        self
    }

    /// Set auth status label for display (e.g. "⚡ user@example.com").
    pub fn with_auth_label(mut self, label: &'a str) -> Self {
        self.auth_label = label;
        self
    }

    /// Set voice mode status indicator (e.g. "listening", "processing").
    pub fn with_voice_status(mut self, status: &'a str) -> Self {
        self.voice_status = status;
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

        // Vim mode badge.
        let vim_span = if self.vim_badge.is_empty() {
            Span::raw("")
        } else {
            let badge_color = match self.vim_badge {
                "N" => Color::Blue,
                "I" => Color::Green,
                "V" | "VL" => Color::Magenta,
                "C" => Color::Yellow,
                _ => Color::White,
            };
            Span::styled(
                format!(" VIM:{} ", self.vim_badge),
                Style::default()
                    .fg(Color::Black)
                    .bg(badge_color)
                    .add_modifier(Modifier::BOLD),
            )
        };

        // Auth status indicator.
        let auth_span = if self.auth_label.is_empty() {
            Span::raw("")
        } else {
            let auth_color = if self.auth_label.contains('\u{26a1}') {
                Color::Green // OAuth
            } else {
                Color::Yellow // API key
            };
            Span::styled(
                format!(" {} ", self.auth_label),
                Style::default().fg(auth_color),
            )
        };

        // Voice mode indicator.
        let voice_span = if self.voice_status.is_empty() {
            Span::raw("")
        } else {
            let (icon, color) = match self.voice_status {
                "listening" => ("\u{1f3a4}", Color::Red), // 🎤 red when recording
                "processing" => ("\u{1f3a4}", Color::Yellow), // 🎤 yellow when processing
                _ => ("\u{1f3a4}", Color::DarkGray),
            };
            Span::styled(
                format!(" {icon} {}", self.voice_status),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )
        };

        let line = Line::from(vec![
            status,
            provider_span,
            model,
            tokens,
            cost_span,
            mcp_span,
            auth_span,
            voice_span,
            vim_span,
            session_span,
        ]);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
