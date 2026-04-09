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
    /// Context window usage percentage (0.0–100.0), None if unknown.
    context_pct: Option<f32>,
    /// Permission mode label (e.g. "ask", "auto", "bypass").
    permission_mode: &'a str,
    /// Current working directory (last 2 components shown).
    cwd: &'a str,
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
            context_pct: None,
            permission_mode: "",
            cwd: "",
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

    /// Set context window usage percentage.
    pub fn with_context_pct(mut self, pct: Option<f32>) -> Self {
        self.context_pct = pct;
        self
    }

    /// Set permission mode label.
    pub fn with_permission_mode(mut self, mode: &'a str) -> Self {
        self.permission_mode = mode;
        self
    }

    /// Set current working directory.
    pub fn with_cwd(mut self, cwd: &'a str) -> Self {
        self.cwd = cwd;
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
    #[allow(clippy::too_many_lines)] // render method with sequential layout sections
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

        // Token count (compact format: 1500 → "1.5K").
        let tokens = Span::styled(
            format!(
                " \u{2191}{} \u{2193}{} ",
                format_tokens(self.usage.input_tokens),
                format_tokens(self.usage.output_tokens),
            ),
            Style::default().fg(Color::Yellow),
        );

        // Cache token display (only when cache tokens are present).
        let cache_read = self.usage.cache_read_input_tokens.unwrap_or(0);
        let cache_write = self.usage.cache_creation_input_tokens.unwrap_or(0);
        let cache_span = if cache_read > 0 || cache_write > 0 {
            let mut parts = String::from(" \u{26a1}");
            if cache_read > 0 {
                parts.push_str(&format_tokens(cache_read));
            }
            if cache_write > 0 {
                use std::fmt::Write;
                let _ = write!(parts, " \u{2191}{}", format_tokens(cache_write));
            }
            parts.push(' ');
            Span::styled(parts, Style::default().fg(Color::Cyan))
        } else {
            Span::raw("")
        };

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

        // Context window usage — visual progress bar + percentage.
        let ctx_span = match self.context_pct {
            Some(pct) if pct > 0.0 => {
                let color = if pct >= 85.0 {
                    Color::Red
                } else if pct >= 60.0 {
                    Color::Yellow
                } else {
                    Color::Green
                };
                // 10-char bar: filled = █, empty = ░
                #[allow(clippy::cast_sign_loss)] // pct is always > 0.0 in this branch
                let filled = ((pct / 100.0) * 10.0).round() as usize;
                let filled = filled.min(10);
                let empty = 10 - filled;
                let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(empty);
                Span::styled(
                    format!(" {bar} {pct:.0}%"),
                    Style::default().fg(color),
                )
            }
            _ => Span::raw(""),
        };

        // Permission mode.
        let perm_span = if self.permission_mode.is_empty() {
            Span::raw("")
        } else {
            let perm_color = match self.permission_mode {
                "auto" => Color::Green,
                "bypass" => Color::Red,
                _ => Color::Yellow, // "ask" and others
            };
            Span::styled(
                format!(" [{}]", self.permission_mode),
                Style::default().fg(perm_color),
            )
        };

        // CWD (last 2 path components).
        let cwd_span = if self.cwd.is_empty() {
            Span::raw("")
        } else {
            let short = shorten_cwd(self.cwd);
            Span::styled(format!(" {short}"), Style::default().fg(Color::DarkGray))
        };

        let line = Line::from(vec![
            status,
            provider_span,
            model,
            tokens,
            cache_span,
            cost_span,
            ctx_span,
            perm_span,
            mcp_span,
            auth_span,
            voice_span,
            vim_span,
            cwd_span,
            session_span,
        ]);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

/// Shorten a path to at most the last 2 components.
fn shorten_cwd(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    match parts.len() {
        0 => "/".to_string(),
        1 => format!("/{}", parts[0]),
        _ => {
            let last_two = &parts[parts.len() - 2..];
            format!("…/{}/{}", last_two[0], last_two[1])
        }
    }
}

/// Format a token count in compact human-readable form.
///
/// - `0` → `"0"`
/// - `999` → `"999"`
/// - `1500` → `"1.5K"`
/// - `1500000` → `"1.5M"`
fn format_tokens(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tokens_zero() {
        assert_eq!(format_tokens(0), "0");
    }

    #[test]
    fn format_tokens_small() {
        assert_eq!(format_tokens(999), "999");
    }

    #[test]
    fn format_tokens_thousands() {
        assert_eq!(format_tokens(1_500), "1.5K");
        assert_eq!(format_tokens(10_000), "10.0K");
        assert_eq!(format_tokens(999_999), "1000.0K");
    }

    #[test]
    fn format_tokens_millions() {
        assert_eq!(format_tokens(1_500_000), "1.5M");
        assert_eq!(format_tokens(2_000_000), "2.0M");
    }
}
