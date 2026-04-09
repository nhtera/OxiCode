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
const MAX_RESULT_LINES: usize = 5;

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
}

impl MessageRenderCache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            cached_width: 0,
        }
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
        for msg in &messages[start..] {
            let mut lines = Vec::new();
            render_message_header_static(msg, &mut lines);
            render_content_blocks_static(&msg.content, &mut lines);
            self.entries.push(lines);
        }
    }

    /// Get cached lines for all messages up to `count`.
    pub fn lines(&self, count: usize) -> &[Vec<Line<'static>>] {
        &self.entries[..count.min(self.entries.len())]
    }

    /// Total cached line count.
    pub fn total_lines(&self) -> usize {
        self.entries.iter().map(|e| e.len()).sum()
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

    fn format_messages(&self) -> Text<'a> {
        let mut lines: Vec<Line<'a>> = Vec::new();

        // Welcome screen when no messages and not streaming.
        let has_streaming =
            self.streaming_lines.map_or(false, |l| !l.is_empty()) || self.streaming_tail.is_some();
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
                render_separator(&mut lines);
            }
            for line in entry {
                lines.push(line.clone());
            }
        }

        // Streaming section.
        if has_streaming || has_thinking {
            if self.message_count > 0 {
                render_separator(&mut lines);
            }
            // Show assistant header if last message isn't already assistant.
            if self
                .last_message_role
                .map_or(true, |r| r != Role::Assistant)
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

    fn render_streaming(&self, lines: &mut Vec<Line<'a>>) {
        // Check if we have any content to show.
        let has_committed = self.streaming_lines.map_or(false, |l| !l.is_empty());
        let has_tail = self.streaming_tail.map_or(false, |t| !t.is_empty());

        // No content yet — show thinking indicator with spinner.
        if !has_committed && !has_tail {
            if let Some(started) = self.turn_started_at {
                let frame = tool_display::spinner_frame(started);
                let elapsed = tool_display::format_elapsed(started);
                lines.push(Line::from(vec![
                    Span::styled(format!("  {frame} "), Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("Thinking... ({elapsed})"),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
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
        // Blinking cursor with elapsed time.
        if let Some(started) = self.turn_started_at {
            let elapsed = tool_display::format_elapsed(started);
            lines.push(Line::from(vec![
                Span::styled("  \u{258d} ", Style::default().fg(Color::Cyan)),
                Span::styled(elapsed, Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "  \u{258d}",
                Style::default().fg(Color::Cyan),
            )));
        }
    }

    fn render_active_tools(&self, lines: &mut Vec<Line<'a>>) {
        for tool in self.active_tools {
            match tool.result {
                None => {
                    lines.push(tool_display::running_tool_line(
                        tool.name,
                        tool.input_summary,
                        tool.started_at,
                    ));
                }
                Some((content, is_error)) => {
                    let completed_lines = tool_display::completed_tool_lines(
                        tool.name,
                        tool.input_summary,
                        content,
                        is_error,
                        Some(tool.started_at),
                        MAX_RESULT_LINES,
                    );
                    lines.extend(completed_lines);
                }
            }
        }
    }

    /// Render the welcome screen shown when no messages exist.
    fn render_welcome_screen(&self, lines: &mut Vec<Line<'a>>) {
        let cyan = Style::default().fg(Color::Cyan);
        let bold_cyan = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let dim = Style::default().fg(Color::DarkGray);
        let green = Style::default().fg(Color::Green);
        let yellow = Style::default().fg(Color::Yellow);

        // Blank line for spacing.
        lines.push(Line::from(""));

        // Title with decorative box.
        lines.push(Line::from(vec![Span::styled(
            "  \u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}",
            dim,
        )]));
        lines.push(Line::from(vec![
            Span::styled("  \u{2502} ", dim),
            Span::styled("OxiCode v0.1.0", bold_cyan),
            Span::styled(" \u{2502}", dim),
        ]));
        lines.push(Line::from(vec![Span::styled(
            "  \u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}",
            dim,
        )]));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Rust-powered AI coding agent",
            dim,
        )));
        lines.push(Line::from(""));

        // Model and project info.
        if let Some(model) = self.model_name {
            lines.push(Line::from(vec![
                Span::styled("  Model: ", dim),
                Span::styled(model.to_string(), green),
            ]));
        }
        if let Some(cwd) = self.cwd {
            lines.push(Line::from(vec![
                Span::styled("  Project: ", dim),
                Span::styled(cwd.to_string(), cyan),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  Quick Start", yellow)));
        lines.push(Line::from(Span::styled(
            "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            dim,
        )));
        lines.push(Line::from(vec![
            Span::styled("  Enter       ", green),
            Span::styled("Send message", dim),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Alt+Enter   ", green),
            Span::styled("New line", dim),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  /help       ", green),
            Span::styled("Show commands", dim),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Ctrl+C      ", green),
            Span::styled("Interrupt / Quit", dim),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  ?           ", green),
            Span::styled("Keyboard shortcuts", dim),
        ]));
        lines.push(Line::from(""));
    }
}

/// Render a horizontal separator line between messages.
fn render_separator(lines: &mut Vec<Line<'_>>) {
    lines.push(Line::from(""));
    // Use dim thin line separator — 60 chars wide to look clean.
    lines.push(Line::from(Span::styled(
        " \u{2500}".to_string() + &"\u{2500}".repeat(59),
        Style::default().fg(Color::Rgb(50, 50, 50)),
    )));
    lines.push(Line::from(""));
}

/// Render a message header into owned lines (for caching).
fn render_message_header_static(msg: &Message, lines: &mut Vec<Line<'static>>) {
    match msg.role {
        Role::User => {
            lines.push(Line::from(Span::styled(
                "\u{25b6} You".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        Role::Assistant => {
            let mut spans = vec![Span::styled(
                "\u{25c0} OxiCode".to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )];
            // Model badge (e.g. " claude-sonnet-4-20250514").
            if let Some(ref model) = msg.model {
                spans.push(Span::styled(
                    format!(" ({model})"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(spans));
        }
        Role::System => {
            lines.push(Line::from(Span::styled(
                "\u{2699} System".to_string(),
                Style::default().fg(Color::Yellow),
            )));
        }
    }
}

/// Render content blocks into owned lines (for caching). All strings are owned.
fn render_content_blocks_static(blocks: &[ContentBlock], lines: &mut Vec<Line<'static>>) {
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
                let line_count = thinking.lines().count();
                // Collapsed header with line count.
                lines.push(Line::from(vec![
                    Span::styled(
                        "  💭 Thinking ".to_string(),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    ),
                    Span::styled(
                        format!("({line_count} lines) ▶"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
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
            .border_style(Style::default().fg(Color::DarkGray))
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
                .track_style(Style::default().fg(Color::DarkGray))
                .thumb_style(Style::default().fg(Color::Gray));
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
        // Should contain separator character.
        let has_separator = text
            .lines
            .iter()
            .any(|line| line.spans.iter().any(|s| s.content.contains('\u{2500}')));
        assert!(has_separator, "Should have separator between messages");
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
