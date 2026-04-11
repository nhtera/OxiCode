use std::cell::Cell;
use std::rc::Rc;

use oxicode_common::{ContentBlock, Message, Role};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
    Widget, Wrap,
};

use super::markdown_view;
use super::tool_display;

/// Maximum lines of tool result output shown inline.
const MAX_RESULT_LINES: usize = 30;

/// Maximum lines of streaming thinking text shown inline.
const MAX_THINK_LINES: usize = 5;

/// Per-message render cache — avoids re-parsing markdown for unchanged messages.
///
/// Each entry stores the pre-rendered `Vec<Line<'static>>` for one message (header +
/// content blocks). The cache is indexed by message position. When message count
/// grows, only the new messages are rendered. The cache is invalidated (cleared)
/// on terminal resize or when message count shrinks (e.g. `/compact`).
pub struct MessageRenderCache {
    /// Cached rendered lines per message index.
    entries: Vec<Vec<Line<'static>>>,
    /// Terminal width when cache was built (invalidate on resize).
    cached_width: u16,
    /// Set of message indices with expanded thinking blocks.
    expanded_thinking: std::collections::HashSet<usize>,
}

impl Default for MessageRenderCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageRenderCache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            cached_width: 0,
            expanded_thinking: std::collections::HashSet::new(),
        }
    }

    /// Toggle thinking block expansion for a message index.
    /// Returns true if the message had a thinking block and was toggled.
    pub fn toggle_thinking(&mut self, messages: &[Message], msg_index: usize) -> bool {
        if msg_index >= messages.len() {
            return false;
        }
        let has_thinking = messages[msg_index]
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Thinking { .. }));
        if !has_thinking {
            return false;
        }
        if self.expanded_thinking.contains(&msg_index) {
            self.expanded_thinking.remove(&msg_index);
        } else {
            self.expanded_thinking.insert(msg_index);
        }
        // Invalidate cache for this message — clear and rebuild.
        self.entries.clear();
        true
    }

    /// Ensure cache is valid for the current state. Returns true if cache was usable.
    /// If messages shrunk (e.g. `/compact`) or terminal resized, clears and rebuilds.
    pub fn update(&mut self, messages: &[Message], terminal_width: u16) {
        // Invalidate on resize or message shrink.
        if terminal_width != self.cached_width || messages.len() < self.entries.len() {
            self.entries.clear();
            self.cached_width = terminal_width;
        }

        // Render only new messages (append to cache).
        let start = self.entries.len();
        for (idx, msg) in messages[start..].iter().enumerate() {
            let msg_index = start + idx;
            let mut lines = Vec::new();
            render_message_header_static(msg, &mut lines);
            let expanded = self.expanded_thinking.contains(&msg_index);
            render_content_blocks_static(&msg.content, &mut lines, expanded);
            // Apply user block style: orange left border + dark background.
            if msg.role == Role::User {
                lines = lines
                    .into_iter()
                    .map(|line| apply_user_block_style(line, terminal_width))
                    .collect();
            }
            self.entries.push(lines);
        }
    }

    /// Get cached lines for all messages up to `count`.
    pub fn lines(&self, count: usize) -> &[Vec<Line<'static>>] {
        &self.entries[..count.min(self.entries.len())]
    }

    /// Total cached line count.
    pub fn total_lines(&self) -> usize {
        self.entries.iter().map(Vec::len).sum()
    }

    /// The terminal width this cache was built for (for external scroll estimation).
    pub fn cached_width(&self) -> u16 {
        self.cached_width
    }
}

/// Snapshot of an active tool call for rendering.
pub struct ActiveToolInfo<'a> {
    pub name: &'a str,
    pub input_summary: &'a str,
    /// Raw input JSON for tool-specific rendering.
    pub raw_input: &'a serde_json::Value,
    /// When this tool call started (for elapsed time + spinner).
    pub started_at: std::time::Instant,
    /// `Some((content, is_error))` when the tool completed.
    pub result: Option<(&'a str, bool)>,
}

/// Widget to display the conversation message list.
///
/// Supports pre-rendered markdown streaming lines from `MarkdownStreamCollector`
/// and renders completed messages with full markdown formatting.
///
/// Uses `MessageRenderCache` for O(1) rendering of unchanged messages.
pub struct MessageView<'a> {
    /// Cached rendered lines for finalized messages (borrowed from App).
    cached_lines: &'a [Vec<Line<'static>>],
    /// Total message count (for separator logic).
    message_count: usize,
    /// Role of the last message (for streaming header logic).
    last_message_role: Option<Role>,
    /// Pre-rendered streaming lines from `MarkdownStreamCollector`.
    streaming_lines: Option<&'a [Line<'static>]>,
    /// Trailing incomplete line fragment (text after last `\n` in stream).
    streaming_tail: Option<&'a str>,
    /// Active tool calls during streaming.
    active_tools: &'a [ActiveToolInfo<'a>],
    scroll_offset: u16,
    /// Viewport height for message limiting (inner height, excluding borders).
    viewport_height: u16,
    /// When the current turn started (for thinking indicator spinner).
    turn_started_at: Option<std::time::Instant>,
    /// Shared cell: render() writes the actual max scroll offset here so
    /// the owning App can read it after draw() returns.
    reported_max_scroll: Option<Rc<Cell<u16>>>,
    /// Model name for welcome screen display.
    model_name: Option<&'a str>,
    /// Current working directory for welcome screen display.
    cwd: Option<&'a str>,
    /// Frame counter for animated spinner (from App).
    frame_count: u64,
    /// Stall detection start time (spinner turns red when stalled).
    stall_start: Option<std::time::Instant>,
    /// Thinking text accumulated during streaming (shown dim italic above content).
    streaming_thinking: Option<&'a str>,
    /// Duration of the last completed turn (for metadata footer).
    last_turn_duration: Option<std::time::Duration>,
    /// Message roles for turn boundary detection.
    message_roles: Vec<Role>,
}

impl<'a> MessageView<'a> {
    pub fn new(
        cached_lines: &'a [Vec<Line<'static>>],
        message_count: usize,
        last_message_role: Option<Role>,
        streaming_lines: Option<&'a [Line<'static>]>,
        streaming_tail: Option<&'a str>,
        active_tools: &'a [ActiveToolInfo<'a>],
        scroll_offset: u16,
    ) -> Self {
        Self {
            cached_lines,
            message_count,
            last_message_role,
            streaming_lines,
            streaming_tail,
            active_tools,
            scroll_offset,
            viewport_height: 50, // Default, overridden during render
            turn_started_at: None,
            reported_max_scroll: None,
            model_name: None,
            cwd: None,
            frame_count: 0,
            stall_start: None,
            streaming_thinking: None,
            last_turn_duration: None,
            message_roles: Vec::new(),
        }
    }

    /// Set the viewport height for message limiting.
    pub fn with_viewport_height(mut self, height: u16) -> Self {
        self.viewport_height = height;
        self
    }

    /// Set the turn start time for the thinking indicator.
    pub fn with_turn_started_at(mut self, started_at: Option<std::time::Instant>) -> Self {
        self.turn_started_at = started_at;
        self
    }

    /// Attach a shared cell to receive the actual max scroll offset after rendering.
    pub fn with_scroll_report(mut self, cell: Rc<Cell<u16>>) -> Self {
        self.reported_max_scroll = Some(cell);
        self
    }

    /// Set model name for welcome screen display.
    pub fn with_model_name(mut self, name: &'a str) -> Self {
        self.model_name = Some(name);
        self
    }

    /// Set CWD for welcome screen display.
    pub fn with_cwd(mut self, cwd: &'a str) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// Set frame counter for animated spinner.
    pub fn with_frame_count(mut self, count: u64) -> Self {
        self.frame_count = count;
        self
    }

    /// Set stall detection start time (spinner turns red when stalled).
    pub fn with_stall_start(mut self, start: Option<std::time::Instant>) -> Self {
        self.stall_start = start;
        self
    }

    /// Set thinking text accumulated during streaming (shown dim italic above content).
    pub fn with_streaming_thinking(mut self, text: &'a str) -> Self {
        self.streaming_thinking = Some(text);
        self
    }

    /// Set the duration of the last completed turn (for metadata footer).
    pub fn with_last_turn_duration(mut self, duration: Option<std::time::Duration>) -> Self {
        self.last_turn_duration = duration;
        self
    }

    /// Set message roles for turn boundary detection (turn separators).
    pub fn with_message_roles(mut self, roles: Vec<Role>) -> Self {
        self.message_roles = roles;
        self
    }

    fn format_messages(&self) -> Text<'a> {
        let mut lines: Vec<Line<'a>> = Vec::new();

        // Welcome screen when no messages and not streaming.
        let has_streaming =
            self.streaming_lines.is_some_and(|l| !l.is_empty()) || self.streaming_tail.is_some();
        let has_thinking =
            !has_streaming && self.streaming_lines.is_some() && self.turn_started_at.is_some();

        if self.message_count == 0 && !has_streaming && !has_thinking {
            self.render_welcome_screen(&mut lines);
            return Text::from(lines);
        }

        // Render all cached messages. Paragraph::scroll() handles viewport
        // positioning — no manual viewport culling needed. This ensures the
        // total line count is accurate for max_scroll_offset computation.
        for (i, entry) in self.cached_lines.iter().enumerate() {
            if i > 0 {
                // Turn separator: thicker line between turns (User after non-User).
                let is_turn_boundary = self.message_roles.get(i).is_some_and(|r| *r == Role::User);
                if is_turn_boundary {
                    render_turn_separator(&mut lines);
                } else {
                    render_separator(&mut lines);
                }
            }
            for line in entry {
                lines.push(line.clone());
            }
        }

        // Turn metadata footer — show duration after the last completed turn.
        if let Some(duration) = self.last_turn_duration {
            if !self.cached_lines.is_empty() {
                let secs = duration.as_secs_f64();
                let duration_str = if secs < 1.0 {
                    format!("{:.0}ms", secs * 1000.0)
                } else if secs < 60.0 {
                    format!("{secs:.1}s")
                } else {
                    #[allow(clippy::cast_sign_loss)]
                    let mins = secs as u64 / 60;
                    #[allow(clippy::cast_sign_loss)]
                    let rem = secs as u64 % 60;
                    format!("{mins}m{rem}s")
                };
                lines.push(Line::from(Span::styled(
                    format!("  ⏱ {duration_str}"),
                    Style::default().fg(crate::render::CHROME_MUTED),
                )));
            }
        }

        // Streaming section.
        if has_streaming || has_thinking {
            if self.message_count > 0 {
                render_separator(&mut lines);
            }
            // Show assistant header if last message isn't already assistant.
            if self.last_message_role != Some(Role::Assistant) {
                lines.push(Line::from(Span::styled(
                    "\u{25c6} Assistant", // ◆ Assistant
                    Style::default()
                        .fg(crate::render::STATUS_GREEN)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            self.render_streaming(&mut lines);
        }

        self.render_active_tools(&mut lines);

        Text::from(lines)
    }

    fn render_streaming(&self, lines: &mut Vec<Line<'a>>) {
        // Check if we have any content to show.
        let has_committed = self.streaming_lines.is_some_and(|l| !l.is_empty());
        let has_tail = self.streaming_tail.is_some_and(|t| !t.is_empty());

        // No content yet — show thinking indicator with animated spinner.
        if !has_committed && !has_tail {
            if let Some(started) = self.turn_started_at {
                let spinner = crate::render::spinner_char(self.frame_count);
                let color = crate::render::spinner_color(self.stall_start);
                let elapsed = tool_display::format_elapsed(started);

                // Show accumulated thinking text (max 5 lines) if available.
                if let Some(thinking) = self.streaming_thinking.filter(|t| !t.is_empty()) {
                    let dim_italic = Style::default()
                        .fg(crate::render::TRANSCRIPT_MUTED)
                        .add_modifier(Modifier::ITALIC);
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {spinner} "), Style::default().fg(color)),
                        Span::styled(format!("Thinking... ({elapsed})"), dim_italic),
                    ]));
                    // Show up to 5 lines of thinking text.
                    let all_lines: Vec<&str> = thinking.lines().collect();
                    let total = all_lines.len();
                    let shown = all_lines.len().min(MAX_THINK_LINES);
                    for think_line in &all_lines[..shown] {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "  \u{2502} ",
                                Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                            ),
                            Span::styled((*think_line).to_string(), dim_italic),
                        ]));
                    }
                    if total > MAX_THINK_LINES {
                        lines.push(Line::from(Span::styled(
                            format!("  \u{2502} ... ({} more lines)", total - MAX_THINK_LINES),
                            Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                        )));
                    }
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {spinner} "), Style::default().fg(color)),
                        Span::styled(
                            format!("Thinking... ({elapsed})"),
                            Style::default()
                                .fg(crate::render::TRANSCRIPT_MUTED)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                }
            }
            return;
        }

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
        // Animated cursor with spinner and elapsed time.
        let spinner = crate::render::spinner_char(self.frame_count);
        let color = crate::render::spinner_color(self.stall_start);
        if let Some(started) = self.turn_started_at {
            let elapsed = tool_display::format_elapsed(started);
            lines.push(Line::from(vec![
                Span::styled(format!("  {spinner} "), Style::default().fg(color)),
                Span::styled(
                    elapsed,
                    Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                format!("  {spinner}"),
                Style::default().fg(color),
            )));
        }
    }

    fn render_active_tools(&self, lines: &mut Vec<Line<'a>>) {
        for (i, tool) in self.active_tools.iter().enumerate() {
            if i > 0 {
                // Breathing room between consecutive tools.
                lines.push(Line::from(""));
            }
            match tool.result {
                None => {
                    lines.push(tool_display::running_tool_line_animated(
                        tool.name,
                        tool.input_summary,
                        tool.started_at,
                        self.frame_count,
                        self.stall_start,
                    ));
                }
                Some((content, is_error)) => {
                    let completed_lines = tool_display::completed_tool_lines_with_input(
                        tool.name,
                        tool.input_summary,
                        content,
                        is_error,
                        Some(tool.started_at),
                        MAX_RESULT_LINES,
                        Some(tool.raw_input),
                    );
                    lines.extend(completed_lines);
                }
            }
        }
    }

    /// Render the welcome screen shown when no messages exist.
    fn render_welcome_screen(&self, lines: &mut Vec<Line<'a>>) {
        let dim = Style::default().fg(crate::render::TRANSCRIPT_MUTED);
        let accent = Style::default().fg(crate::render::CLAUDE_ORANGE);

        // ASCII art logo — gradient from brand magenta to soft blue.
        let logo_lines = [
            "   \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2557}  \u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}",
            "  \u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{2588}\u{2588}\u{2557}\u{255a}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2554}\u{255d}\u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}",
            "  \u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551} \u{255a}\u{2588}\u{2588}\u{2588}\u{2554}\u{255d} \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2551}     \u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2551}  \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}  ",
            "  \u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551} \u{2588}\u{2588}\u{2554}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2551}     \u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2551}  \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{255d}  ",
            "  \u{255a}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2554}\u{255d}\u{2588}\u{2588}\u{2554}\u{255d} \u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2551}\u{255a}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}\u{255a}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2554}\u{255d}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2554}\u{255d}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}",
            "   \u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d} \u{255a}\u{2550}\u{255d}  \u{255a}\u{2550}\u{255d}\u{255a}\u{2550}\u{255d} \u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d} \u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d} \u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d} \u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}",
        ];
        // Gradient: deep rust red → burnt orange → warm orange for depth.
        let gradient = [
            Color::Rgb(139, 37, 0),   // Deep rust red
            Color::Rgb(180, 70, 15),  // Dark burnt orange
            Color::Rgb(200, 90, 23),  // Brand burnt orange
            Color::Rgb(220, 120, 40), // Warm amber
            Color::Rgb(232, 160, 64), // Golden orange
            Color::Rgb(210, 140, 80), // Muted warm
        ];

        lines.push(Line::from(""));
        for (logo_line, &color) in logo_lines.iter().zip(gradient.iter()) {
            lines.push(Line::from(Span::styled(
                (*logo_line).to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
        }
        lines.push(Line::from(""));

        // Version • model • cwd info line.
        let mut info_spans: Vec<Span<'a>> = vec![Span::styled("  v0.1.0", dim)];
        if let Some(model) = self.model_name {
            info_spans.push(Span::styled(" • ", dim));
            info_spans.push(Span::styled(
                model.to_string(),
                Style::default().fg(crate::render::TRANSCRIPT_MUTED),
            ));
        }
        if let Some(cwd) = self.cwd {
            info_spans.push(Span::styled(" • ", dim));
            info_spans.push(Span::styled(
                cwd.to_string(),
                Style::default().fg(crate::render::TRANSCRIPT_MUTED),
            ));
        }
        lines.push(Line::from(info_spans));
        lines.push(Line::from(""));

        // Quick shortcuts.
        lines.push(Line::from(vec![
            Span::styled("  Enter  ", accent),
            Span::styled("— send message    ", dim),
            Span::styled("Ctrl+C  ", accent),
            Span::styled("— interrupt/quit", dim),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Tab    ", accent),
            Span::styled("— toggle panel    ", dim),
            Span::styled("/help   ", accent),
            Span::styled("— commands", dim),
        ]));
        lines.push(Line::from(""));
    }
}

/// Render a compact separator between messages — single empty line.
fn render_separator(lines: &mut Vec<Line<'_>>) {
    lines.push(Line::from(""));
}

/// Visual separator between conversation turns (dimmed horizontal line).
fn render_turn_separator(lines: &mut Vec<Line<'_>>) {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "─".repeat(60),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));
}

/// Apply user block style: orange `▏` left border + dark background.
///
/// Makes user messages visually distinct from assistant replies.
fn apply_user_block_style(line: Line<'static>, width: u16) -> Line<'static> {
    let bg = crate::render::TRANSCRIPT_USER_BG;
    let mut spans = Vec::with_capacity(line.spans.len() + 2);
    // Orange left border character.
    spans.push(Span::styled(
        "\u{258f} ".to_string(), // ▏ (left 1/8 block)
        Style::default().fg(crate::render::CLAUDE_ORANGE).bg(bg),
    ));
    // Re-style existing spans with dark background.
    for span in line.spans {
        spans.push(Span::styled(span.content.to_string(), span.style.bg(bg)));
    }
    // Fill remaining width with background to create a solid block.
    // Use inner width (accounting for Block borders) to avoid triggering
    // unwanted line wrapping inside the bordered Paragraph.
    let built = Line::from(spans);
    let used = built.width();
    let inner_width = (width as usize).saturating_sub(2); // subtract Block borders
    if used < inner_width {
        let mut spans = built.spans;
        spans.push(Span::styled(
            " ".repeat(inner_width - used),
            Style::default().bg(bg),
        ));
        Line::from(spans)
    } else {
        built
    }
}

/// Render a message header into owned lines (for caching).
fn render_message_header_static(msg: &Message, lines: &mut Vec<Line<'static>>) {
    let muted = Style::default().fg(crate::render::TRANSCRIPT_MUTED);
    match msg.role {
        Role::User => {
            // User header with orange accent.
            lines.push(Line::from(Span::styled(
                "\u{276f} You".to_string(), // ❯ You
                Style::default()
                    .fg(crate::render::CLAUDE_ORANGE)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        Role::Assistant => {
            let mut spans = vec![Span::styled(
                "\u{25c6} Assistant".to_string(), // ◆ Assistant
                Style::default()
                    .fg(crate::render::STATUS_GREEN)
                    .add_modifier(Modifier::BOLD),
            )];
            // Shortened model name (e.g. "claude-sonnet-4-20250514" → "sonnet-4").
            if let Some(ref model) = msg.model {
                spans.push(Span::styled(
                    format!(" ({})", shorten_model_name(model)),
                    muted,
                ));
            }
            // Token usage badge.
            if let Some(ref usage) = msg.usage {
                spans.push(Span::styled(
                    format!(" · {}↓ {}↑", usage.input_tokens, usage.output_tokens),
                    muted,
                ));
            }
            lines.push(Line::from(spans));
        }
        Role::System => {
            lines.push(Line::from(Span::styled(
                "\u{2699} System".to_string(),
                Style::default().fg(crate::render::STATUS_YELLOW),
            )));
        }
    }
}

/// Shorten a model name for display (e.g. "claude-sonnet-4-20250514" → "sonnet-4").
fn shorten_model_name(model: &str) -> String {
    // Strip "claude-" prefix and date suffix.
    let name = model.strip_prefix("claude-").unwrap_or(model);
    // Remove trailing date pattern (-YYYYMMDD).
    if name.len() > 9 {
        let maybe_date = &name[name.len() - 9..];
        if maybe_date.starts_with('-') && maybe_date[1..].chars().all(|c| c.is_ascii_digit()) {
            return name[..name.len() - 9].to_string();
        }
    }
    name.to_string()
}

/// Render content blocks into owned lines (for caching). All strings are owned.
///
/// When `expanded_thinking` is true, thinking blocks show full text instead of collapsed.
#[allow(clippy::too_many_lines)]
fn render_content_blocks_static(
    blocks: &[ContentBlock],
    lines: &mut Vec<Line<'static>>,
    expanded_thinking: bool,
) {
    // Build tool_name map: tool_use_id → tool_name (for result display).
    let tool_names: std::collections::HashMap<&str, &str> = blocks
        .iter()
        .filter_map(|b| {
            if let ContentBlock::ToolUse { id, name, .. } = b {
                Some((id.as_str(), name.as_str()))
            } else {
                None
            }
        })
        .collect();

    let mut image_counter: usize = 0;

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
            ContentBlock::Image { .. } => {
                image_counter += 1;
                lines.push(Line::from(vec![Span::styled(
                    format!("  [Image #{image_counter}]"),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                )]));
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let summary = tool_input_summary(input);
                lines.push(Line::from(vec![
                    Span::styled(
                        "  \u{27f3} ",
                        Style::default().fg(crate::render::STATUS_YELLOW),
                    ),
                    Span::styled(
                        name.clone(),
                        Style::default()
                            .fg(crate::render::STATUS_YELLOW)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" \u{2500} {summary}"),
                        Style::default().fg(crate::render::TRANSCRIPT_SECONDARY),
                    ),
                ]));
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let (icon, color) = if *is_error {
                    ("\u{2717}", crate::render::STATUS_RED) // ✗
                } else {
                    ("\u{2713}", crate::render::STATUS_GREEN) // ✓
                };
                // Show tool name if available, otherwise generic tag.
                let tool_label = tool_names.get(tool_use_id.as_str()).map_or_else(
                    || {
                        if *is_error {
                            "error".to_string()
                        } else {
                            "result".to_string()
                        }
                    },
                    |name| (*name).to_string(),
                );
                lines.push(Line::from(vec![
                    Span::styled(format!("  {icon} "), Style::default().fg(color)),
                    Span::styled(
                        tool_label,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]));
                let result_fg = if *is_error {
                    crate::render::STATUS_RED
                } else {
                    crate::render::TRANSCRIPT_SECONDARY
                };
                let result_style = Style::default().fg(result_fg);
                let pipe_style = Style::default().fg(crate::render::TRANSCRIPT_MUTED);
                let total_lines = content.lines().count();
                for (i, line) in content.lines().enumerate() {
                    if i >= MAX_RESULT_LINES {
                        lines.push(Line::from(Span::styled(
                            format!("  \u{2502} ... ({} more lines)", total_lines - i),
                            pipe_style,
                        )));
                        break;
                    }
                    lines.push(Line::from(vec![
                        Span::styled("  \u{2502} ".to_string(), pipe_style),
                        Span::styled(line.to_string(), result_style),
                    ]));
                }
            }
            ContentBlock::Thinking { thinking } => {
                let line_count = thinking.lines().count();
                if expanded_thinking {
                    // Expanded: show full thinking text.
                    lines.push(Line::from(vec![
                        Span::styled(
                            "  \u{25bc} Thinking ".to_string(), // ▼
                            Style::default()
                                .fg(crate::render::TRANSCRIPT_MUTED)
                                .add_modifier(Modifier::ITALIC),
                        ),
                        Span::styled(
                            format!("({line_count} lines)"),
                            Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                        ),
                    ]));
                    let dim_italic = Style::default()
                        .fg(crate::render::TRANSCRIPT_MUTED)
                        .add_modifier(Modifier::ITALIC);
                    for think_line in thinking.lines() {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "  \u{2502} ".to_string(),
                                Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                            ),
                            Span::styled(think_line.to_string(), dim_italic),
                        ]));
                    }
                } else {
                    // Collapsed: show 72-char preview of thinking text + line count.
                    let preview: String = thinking
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(72)
                        .collect();
                    let truncated = preview.len() < thinking.lines().next().map_or(0, str::len);
                    let preview_display = if truncated {
                        format!("\"{preview}...\"")
                    } else if preview.is_empty() {
                        String::new()
                    } else {
                        format!("\"{preview}\"")
                    };
                    let mut spans = vec![Span::styled(
                        "  \u{25b6} Thinking ".to_string(), // ▶
                        Style::default()
                            .fg(crate::render::TRANSCRIPT_MUTED)
                            .add_modifier(Modifier::ITALIC),
                    )];
                    if preview_display.is_empty() {
                        spans.push(Span::styled(
                            format!("({line_count} lines)"),
                            Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                        ));
                    } else {
                        spans.push(Span::styled(
                            preview_display,
                            Style::default()
                                .fg(crate::render::CHROME_MUTED)
                                .add_modifier(Modifier::ITALIC),
                        ));
                        spans.push(Span::styled(
                            format!(" ({line_count} lines)"),
                            Style::default().fg(crate::render::TRANSCRIPT_MUTED),
                        ));
                    }
                    lines.push(Line::from(spans));
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
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(crate::render::BORDER_COLOR))
            .title(" Conversation ");

        // Set viewport height for message limiting optimization.
        self.viewport_height = area.height.saturating_sub(2);

        let text = self.format_messages();

        // Use Ratatui's own Paragraph::line_count() to get the exact wrapped
        // line count. This accounts for word-wrapping, unicode widths, and all
        // the same logic that Paragraph::render() uses — eliminating any
        // mismatch between our scroll estimation and the actual display.
        let paragraph = Paragraph::new(text)
            .block(block.clone())
            .wrap(Wrap { trim: false });

        let wrapped_line_count = paragraph.line_count(area.width);

        let scroll_y = resolve_scroll_offset(self.scroll_offset, area, wrapped_line_count);

        // Report the actual max scroll offset to the parent App.
        let actual_max = max_content_scroll(area, wrapped_line_count);
        if let Some(ref cell) = self.reported_max_scroll {
            cell.set(actual_max);
        }

        let paragraph = paragraph.scroll((scroll_y, 0));
        paragraph.render(area, buf);

        // Scrollbar on right edge (only when content exceeds viewport).
        if wrapped_line_count > self.viewport_height as usize {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█")
                .track_style(Style::default().fg(crate::render::BORDER_COLOR))
                .thumb_style(Style::default().fg(crate::render::CHROME_MUTED));
            // Use max scrollable range as content_length so the thumb reaches
            // the very bottom when fully scrolled down.
            let mut scrollbar_state =
                ScrollbarState::new(actual_max as usize).position(scroll_y as usize);
            scrollbar.render(area, buf, &mut scrollbar_state);
        }
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
        let mut cache = MessageRenderCache::new();
        cache.update(&messages, 80);
        let active_tools: &[ActiveToolInfo<'_>] = &[];
        let streaming_lines: Vec<Line<'static>> = vec![Line::from("streaming...")];
        let widget = MessageView::new(
            cache.lines(messages.len()),
            messages.len(),
            messages.last().map(|m| m.role),
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
        let messages = vec![Message::user("hello"), assistant_text_message("world")];
        let mut cache = MessageRenderCache::new();
        cache.update(&messages, 80);
        let active_tools: &[ActiveToolInfo<'_>] = &[];
        let view = MessageView::new(
            cache.lines(messages.len()),
            messages.len(),
            messages.last().map(|m| m.role),
            None,
            None,
            active_tools,
            0,
        );
        let text = view.format_messages();
        // Compact separator: single empty line between messages.
        let has_empty = text.lines.iter().any(|line| line.spans.is_empty());
        assert!(
            has_empty,
            "Should have empty-line separator between messages"
        );
    }

    #[test]
    fn test_markdown_rendered_in_content_blocks() {
        let messages = vec![assistant_text_message("**bold text** and `code`")];
        let mut cache = MessageRenderCache::new();
        cache.update(&messages, 80);
        let active_tools: &[ActiveToolInfo<'_>] = &[];
        let view = MessageView::new(
            cache.lines(messages.len()),
            messages.len(),
            messages.last().map(|m| m.role),
            None,
            None,
            active_tools,
            0,
        );
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

    #[test]
    fn test_render_cache_incremental() {
        let msg1 = Message::user("hello");
        let msg2 = assistant_text_message("world");
        let mut cache = MessageRenderCache::new();

        // First update: 1 message
        cache.update(&[msg1.clone()], 80);
        assert_eq!(cache.entries.len(), 1);

        // Second update: 2 messages — only renders the new one
        let messages = vec![msg1, msg2];
        cache.update(&messages, 80);
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn test_render_cache_invalidates_on_resize() {
        let messages = vec![Message::user("hello")];
        let mut cache = MessageRenderCache::new();
        cache.update(&messages, 80);
        assert_eq!(cache.entries.len(), 1);

        // Resize → cache cleared and rebuilt
        cache.update(&messages, 120);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.cached_width, 120);
    }

    #[test]
    fn test_scroll_offset_reported_correctly_for_long_markdown() {
        // Simulate realistic content similar to "what can you do?" response.
        let long_md = r#"## What It Is

A **Rust-powered CLI agent** — a full-parity port of Claude Code — for software engineering tasks.

## Architecture: 17 Cargo Crates

| Layer | Crates |
|---|---|
| Foundation | `oxicode-common` — shared types, errors |
| Core Services | `oxicode-config`, `oxicode-api`, `oxicode-tools` (49 tools) |
| Engine | `oxicode-core` (query engine), `oxicode-permissions`, `oxicode-state` |
| Persistence | `oxicode-session`, `oxicode-hooks`, `oxicode-context` |
| Agents | `oxicode-agents`, `oxicode-skills`, `oxicode-tasks` |
| Plugins | `oxicode-plugins` (subprocess system, hot-reload) |
| Integrations | `oxicode-mcp` (4 transports), `oxicode-tui` (Ratatui UI) |
| Entry Point | `oxicode-cli` (binary, 105+ slash commands) |

## Key Features

• 49 built-in tools — file ops, bash, git, grep, glob, agents, MCP, and more
• 6-layer permission pipeline
• Ratatui TUI — split panes, markdown rendering, syntax highlighting, themes
• MCP client — stdio, SSE, streamable HTTP, WebSocket transports
• Multi-provider API — Anthropic, OpenAI, Google, Amazon Bedrock
• Agent system — background tasks, subprocess agents
• Session persistence — JSONL conversation logs
• Hook system — 26 lifecycle events
• Vim mode — optional vim keybindings"#;

        let messages = vec![
            Message::user("What can you do?"),
            assistant_text_message(long_md),
        ];
        let mut cache = MessageRenderCache::new();
        cache.update(&messages, 80);

        let active_tools: &[ActiveToolInfo<'_>] = &[];
        let scroll_report = Rc::new(Cell::new(0u16));

        let widget = MessageView::new(
            cache.lines(messages.len()),
            messages.len(),
            messages.last().map(|m| m.role),
            None,
            None,
            active_tools,
            u16::MAX, // auto-scroll
        )
        .with_scroll_report(Rc::clone(&scroll_report));

        // Simulate a 80x24 terminal.
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let max_scroll = scroll_report.get();
        // With 80x24 viewport (inner height = 22), content should exceed it.
        assert!(
            max_scroll > 0,
            "max_scroll should be > 0 for long markdown content, got {max_scroll}"
        );
    }

    #[test]
    fn test_scroll_offset_with_emoji_content() {
        // Emoji have display width 2 but char count 1 — test that wrapping accounts for this.
        let emoji_content = "🖥 Files & Filesystem — Read, write, edit files. Search codebases with grep/glob. Navigate project structures. This is a long line with emoji that should wrap.";

        let messages = vec![
            Message::user("list features"),
            assistant_text_message(emoji_content),
        ];
        let mut cache = MessageRenderCache::new();
        cache.update(&messages, 60); // narrow terminal to force wrapping

        let active_tools: &[ActiveToolInfo<'_>] = &[];
        let scroll_report = Rc::new(Cell::new(0u16));

        let widget = MessageView::new(
            cache.lines(messages.len()),
            messages.len(),
            messages.last().map(|m| m.role),
            None,
            None,
            active_tools,
            u16::MAX,
        )
        .with_scroll_report(Rc::clone(&scroll_report));

        let area = Rect::new(0, 0, 60, 10); // Small viewport
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let max_scroll = scroll_report.get();
        // Content should produce enough lines to scroll.
        // The emoji line alone is ~160 chars which wraps to ~3 lines in 58-col inner.
        // Plus headers, separators, etc.
        assert!(
            max_scroll > 0,
            "max_scroll should be > 0 for emoji content, got {max_scroll}"
        );
    }
}
