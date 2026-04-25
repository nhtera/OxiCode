use std::time::Instant;

use oxicode_common::Usage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::render;

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
    /// Session start time for elapsed display.
    session_start: Option<Instant>,
    /// Accurate total cost from CostTracker (replaces hardcoded pricing).
    cost_usd: f64,
    /// Retry/rate-limit state label shown in status bar (e.g. "⟳ Retrying 2/5").
    /// Empty string when not retrying.
    retry_status: &'a str,
    /// Number of registered slash commands (0 = hide chip).
    slash_command_count: usize,
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
            session_start: None,
            cost_usd: 0.0,
            retry_status: "",
            slash_command_count: 0,
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

    /// Set session start time for elapsed display.
    pub fn with_session_start(mut self, start: Option<Instant>) -> Self {
        self.session_start = start;
        self
    }

    /// Set total session cost from CostTracker (accurate per-model pricing).
    pub fn with_cost(mut self, cost_usd: f64) -> Self {
        self.cost_usd = cost_usd;
        self
    }

    /// Set retry/rate-limit status label shown in the status bar.
    ///
    /// Pass a non-empty string (e.g. `"⟳ Retrying 2/5"`) while a retry is in
    /// flight; pass `""` to hide the indicator.
    pub fn with_retry_status(mut self, status: &'a str) -> Self {
        self.retry_status = status;
        self
    }

    /// Set slash command count for display (0 hides the chip).
    pub fn with_cmd_count(mut self, count: usize) -> Self {
        self.slash_command_count = count;
        self
    }
}

/// Map provider name to a color for visual distinction.
fn provider_color(provider: &str) -> Color {
    match provider {
        "anthropic" => Color::Rgb(200, 90, 23), // Burnt orange (brand)
        "openai" => Color::Rgb(116, 170, 156),  // Teal
        "ollama" => Color::Rgb(150, 140, 135),  // Warm gray (local)
        "deepseek" => Color::Rgb(100, 149, 237), // Cornflower blue
        "azure" => Color::Rgb(0, 120, 212),     // Azure blue
        "openrouter" => Color::Rgb(168, 85, 247), // Purple
        _ => render::CHROME_TEXT,
    }
}

impl Widget for StatusBar<'_> {
    #[allow(clippy::too_many_lines)] // render method with sequential layout sections
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill entire status bar row with dark background.
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, area.y)) {
                cell.set_bg(render::STATUS_BAR_BG);
            }
        }

        // Streaming indicator — shows retry state when active, otherwise streaming/ready.
        let status = if !self.retry_status.is_empty() {
            Span::styled(
                format!(" {} ", self.retry_status),
                Style::default()
                    .fg(render::STATUS_YELLOW)
                    .bg(render::STATUS_BAR_BG)
                    .add_modifier(Modifier::BOLD),
            )
        } else if self.is_streaming {
            Span::styled(
                " ● Streaming ",
                Style::default()
                    .fg(render::STATUS_GREEN)
                    .bg(render::STATUS_BAR_BG)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                " ○ Ready ",
                Style::default()
                    .fg(render::CHROME_MUTED)
                    .bg(render::STATUS_BAR_BG),
            )
        };

        // Provider badge — subtle colored text (no inverted bg).
        let provider_span = if self.provider.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                format!(" {} ", self.provider),
                Style::default()
                    .fg(provider_color(self.provider))
                    .bg(render::STATUS_BAR_BG)
                    .add_modifier(Modifier::BOLD),
            )
        };

        // Model name — muted badge style.
        let model = Span::styled(
            format!(" {} ", self.model),
            Style::default()
                .fg(render::CHROME_TEXT)
                .bg(Color::Rgb(45, 38, 34)),
        );

        // Token count (compact format).
        let tokens = Span::styled(
            format!(
                " ↑{} ↓{} ",
                format_tokens(self.usage.input_tokens),
                format_tokens(self.usage.output_tokens),
            ),
            Style::default()
                .fg(render::STATUS_YELLOW)
                .bg(render::STATUS_BAR_BG),
        );

        // Cache token display.
        let cache_read = self.usage.cache_read_input_tokens.unwrap_or(0);
        let cache_write = self.usage.cache_creation_input_tokens.unwrap_or(0);
        let cache_span = if cache_read > 0 || cache_write > 0 {
            let mut parts = String::from(" ⚡");
            if cache_read > 0 {
                parts.push_str(&format_tokens(cache_read));
            }
            if cache_write > 0 {
                use std::fmt::Write;
                let _ = write!(parts, " ↑{}", format_tokens(cache_write));
            }
            parts.push(' ');
            Span::styled(
                parts,
                Style::default()
                    .fg(render::STATUS_CYAN)
                    .bg(render::STATUS_BAR_BG),
            )
        } else {
            Span::raw("")
        };

        // Cost from CostTracker (accurate per-model pricing).
        // Normalize negative zero to positive zero for display.
        let cost = if self.cost_usd == 0.0 {
            0.0
        } else {
            self.cost_usd
        };
        let cost_span = Span::styled(
            format!(" ${cost:.4} "),
            Style::default()
                .fg(render::STATUS_CYAN)
                .bg(render::STATUS_BAR_BG),
        );

        // MCP indicator.
        let mcp_span = if self.mcp_server_count > 0 {
            Span::styled(
                format!(" MCP:{} ", self.mcp_server_count),
                Style::default()
                    .fg(Color::Rgb(180, 120, 80))
                    .bg(render::STATUS_BAR_BG),
            )
        } else {
            Span::raw("")
        };

        // Slash command count chip (hidden when 0).
        let slash_cmd_count_span = if self.slash_command_count > 0 {
            Span::styled(
                format!(" {} cmds ", self.slash_command_count),
                Style::default()
                    .fg(render::CHROME_MUTED)
                    .bg(render::STATUS_BAR_BG),
            )
        } else {
            Span::raw("")
        };

        // Session name (truncated).
        let session_span = if self.session_name.is_empty() {
            Span::raw("")
        } else {
            let short: String = self.session_name.chars().take(8).collect();
            Span::styled(
                format!(" [{short}] "),
                Style::default()
                    .fg(render::CHROME_MUTED)
                    .bg(render::STATUS_BAR_BG),
            )
        };

        // Vim mode badge — colored text on subtle bg, not inverted.
        let vim_span = if self.vim_badge.is_empty() {
            Span::raw("")
        } else {
            let badge_color = match self.vim_badge {
                "N" => Color::Rgb(180, 130, 80),        // Warm amber for Normal
                "I" => render::STATUS_GREEN,            // Green for Insert
                "V" | "VL" => Color::Rgb(180, 100, 60), // Warm rust for Visual
                "C" => render::STATUS_YELLOW,           // Amber for Command
                _ => render::CHROME_TEXT,
            };
            Span::styled(
                format!(" VIM:{} ", self.vim_badge),
                Style::default()
                    .fg(badge_color)
                    .bg(Color::Rgb(40, 34, 30))
                    .add_modifier(Modifier::BOLD),
            )
        };

        // Auth status indicator.
        let auth_span = if self.auth_label.is_empty() {
            Span::raw("")
        } else {
            let auth_color = if self.auth_label.contains('⚡') {
                render::STATUS_GREEN
            } else {
                render::STATUS_YELLOW
            };
            Span::styled(
                format!(" {} ", self.auth_label),
                Style::default().fg(auth_color).bg(render::STATUS_BAR_BG),
            )
        };

        // Voice mode indicator.
        let voice_span = if self.voice_status.is_empty() {
            Span::raw("")
        } else {
            let (icon, color) = match self.voice_status {
                "listening" => ("🎤", render::STATUS_RED),
                "processing" => ("🎤", render::STATUS_YELLOW),
                _ => ("🎤", render::CHROME_MUTED),
            };
            Span::styled(
                format!(" {icon} {}", self.voice_status),
                Style::default()
                    .fg(color)
                    .bg(render::STATUS_BAR_BG)
                    .add_modifier(Modifier::BOLD),
            )
        };

        // Context window usage — visual progress bar.
        let ctx_span = match self.context_pct {
            Some(pct) if pct > 0.0 => {
                let color = if pct >= 85.0 {
                    render::STATUS_RED
                } else if pct >= 60.0 {
                    render::STATUS_YELLOW
                } else {
                    render::STATUS_GREEN
                };
                #[allow(clippy::cast_sign_loss)]
                let filled = ((pct / 100.0) * 10.0).round() as usize;
                let filled = filled.min(10);
                let empty = 10 - filled;
                let bar: String = "█".repeat(filled) + &"░".repeat(empty);
                Span::styled(
                    format!(" {bar} {pct:.0}%"),
                    Style::default().fg(color).bg(render::STATUS_BAR_BG),
                )
            }
            _ => Span::raw(""),
        };

        // Permission mode.
        let perm_span = if self.permission_mode.is_empty() {
            Span::raw("")
        } else {
            let perm_color = match self.permission_mode {
                "auto" => render::STATUS_GREEN,
                "bypass" => render::STATUS_RED,
                _ => render::STATUS_YELLOW,
            };
            Span::styled(
                format!(" [{}]", self.permission_mode),
                Style::default().fg(perm_color).bg(render::STATUS_BAR_BG),
            )
        };

        // Elapsed session time.
        let elapsed_span = match self.session_start {
            Some(start) => {
                let secs = start.elapsed().as_secs();
                let display = if secs < 60 {
                    format!(" {secs}s")
                } else if secs < 3600 {
                    format!(" {}m{}s", secs / 60, secs % 60)
                } else {
                    format!(" {}h{}m", secs / 3600, (secs % 3600) / 60)
                };
                Span::styled(
                    display,
                    Style::default()
                        .fg(render::CHROME_MUTED)
                        .bg(render::STATUS_BAR_BG),
                )
            }
            None => Span::raw(""),
        };

        // CWD (last 2 path components).
        let cwd_span = if self.cwd.is_empty() {
            Span::raw("")
        } else {
            let short = shorten_cwd(self.cwd);
            Span::styled(
                format!(" {short}"),
                Style::default()
                    .fg(render::CHROME_MUTED)
                    .bg(render::STATUS_BAR_BG),
            )
        };

        let line = Line::from(vec![
            status,
            provider_span,
            model,
            tokens,
            cache_span,
            cost_span,
            elapsed_span,
            ctx_span,
            perm_span,
            mcp_span,
            slash_cmd_count_span,
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
        format!("{:.1}M", f64::from(n) / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", f64::from(n) / 1_000.0)
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

    // ── slash command count chip ──────────────────────────────────────────

    #[test]
    fn status_bar_cmd_count_chip_hidden_when_zero() {
        // with_cmd_count(0) → slash_command_count field defaults to 0 → chip hidden.
        let usage = Usage::default();
        let bar = StatusBar::new("model", &usage, false);
        assert_eq!(bar.slash_command_count, 0);
    }

    #[test]
    fn status_bar_cmd_count_chip_set() {
        let usage = Usage::default();
        let bar = StatusBar::new("model", &usage, false).with_cmd_count(140);
        assert_eq!(bar.slash_command_count, 140);
    }
}
