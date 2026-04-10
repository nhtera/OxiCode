use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::highlight;

/// Default width for box-drawing code block borders.
const CODE_BLOCK_WIDTH: usize = 60;

/// Width used when rendering horizontal rules.
const HORIZONTAL_RULE_WIDTH: usize = 60;

/// Detect `http://` / `https://` URL byte ranges within `text`.
///
/// Returns a list of `(start, end)` byte index pairs (end is exclusive).
/// Terminates each URL at the first whitespace or any of `)`, `>`, `]`.
fn detect_urls(text: &str) -> Vec<(usize, usize)> {
    let mut urls = Vec::new();
    let mut search_from = 0;
    while search_from < text.len() {
        let slice = &text[search_from..];
        let found = slice
            .find("https://")
            .or_else(|| slice.find("http://"))
            .map(|pos| search_from + pos);
        let Some(abs_start) = found else { break };
        let end = text[abs_start..]
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '>' | ']'))
            .map_or(text.len(), |e| abs_start + e);
        if end > abs_start {
            urls.push((abs_start, end));
        }
        search_from = end.max(abs_start + 1);
    }
    urls
}

/// Style applied to auto-detected bare URLs.
fn url_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::UNDERLINED)
}

/// Build a `Vec<Span<'static>>` from `text`, wrapping detected URLs with
/// [`url_style`] and the remainder with `base_style`.
fn spans_from_text_with_urls(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let url_ranges = detect_urls(text);
    if url_ranges.is_empty() {
        return vec![Span::styled(text.to_string(), base_style)];
    }
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    for (start, end) in url_ranges {
        if cursor < start {
            spans.push(Span::styled(text[cursor..start].to_string(), base_style));
        }
        spans.push(Span::styled(text[start..end].to_string(), url_style()));
        cursor = end;
    }
    if cursor < text.len() {
        spans.push(Span::styled(text[cursor..].to_string(), base_style));
    }
    spans
}

/// Return a dim horizontal-rule `Line`.
fn horizontal_rule_line() -> Line<'static> {
    Line::from(Span::styled(
        "\u{2500}".repeat(HORIZONTAL_RULE_WIDTH),
        Style::default().fg(Color::DarkGray),
    ))
}

/// Normalize common language alias shorthands to their canonical name.
/// e.g. `"rs"` → `"rust"`, `"py"` → `"python"`.
fn normalize_lang(lang: &str) -> &str {
    match lang {
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "sh" | "bash" | "zsh" => "shell",
        other => other,
    }
}

/// Renders markdown text as styled Ratatui lines.
pub struct MarkdownView<'a> {
    source: &'a str,
}

impl<'a> MarkdownView<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source }
    }

    /// Convert markdown source to styled Ratatui Lines.
    pub fn to_lines(&self) -> Vec<Line<'a>> {
        let parser = Parser::new(self.source);
        let mut lines: Vec<Line> = Vec::new();
        let mut current_spans: Vec<Span> = Vec::new();
        let mut style_stack: Vec<Style> = vec![Style::default()];
        let mut in_code_block = false;
        let mut code_block_lang = String::new();
        let mut code_block_lines: Vec<String> = Vec::new();

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => {
                        let style = heading_style(level);
                        style_stack.push(style);
                    }
                    Tag::Strong => {
                        let base = *style_stack.last().unwrap_or(&Style::default());
                        style_stack.push(base.add_modifier(Modifier::BOLD));
                    }
                    Tag::Emphasis => {
                        let base = *style_stack.last().unwrap_or(&Style::default());
                        style_stack.push(base.add_modifier(Modifier::ITALIC));
                    }
                    Tag::CodeBlock(kind) => {
                        in_code_block = true;
                        code_block_lang = match kind {
                            CodeBlockKind::Fenced(lang) => lang.to_string(),
                            CodeBlockKind::Indented => String::new(),
                        };
                        code_block_lines.clear();
                    }
                    Tag::Link { .. } => {
                        let base = *style_stack.last().unwrap_or(&Style::default());
                        style_stack.push(base.fg(Color::Blue).add_modifier(Modifier::UNDERLINED));
                    }
                    Tag::Item => {
                        current_spans
                            .push(Span::styled("• ", Style::default().fg(Color::DarkGray)));
                    }
                    _ => {}
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::Heading(_) => {
                        style_stack.pop();
                        flush_spans(&mut current_spans, &mut lines);
                        lines.push(Line::from(""));
                    }
                    TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link => {
                        style_stack.pop();
                    }
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        render_code_block_boxed(&code_block_lines, &code_block_lang, &mut lines);
                        lines.push(Line::from(""));
                        code_block_lines.clear();
                    }
                    TagEnd::Paragraph => {
                        flush_spans(&mut current_spans, &mut lines);
                        lines.push(Line::from(""));
                    }
                    TagEnd::Item => {
                        flush_spans(&mut current_spans, &mut lines);
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if in_code_block {
                        for line in text.lines() {
                            code_block_lines.push(line.to_string());
                        }
                    } else {
                        let style = *style_stack.last().unwrap_or(&Style::default());
                        current_spans
                            .extend(spans_from_text_with_urls(&text, style));
                    }
                }
                Event::Code(code) => {
                    let code_style = Style::default()
                        .fg(Color::Yellow)
                        .bg(Color::Rgb(40, 40, 40));
                    current_spans.push(Span::styled(format!("`{code}`"), code_style));
                }
                Event::SoftBreak | Event::HardBreak => {
                    flush_spans(&mut current_spans, &mut lines);
                }
                Event::Rule => {
                    flush_spans(&mut current_spans, &mut lines);
                    lines.push(horizontal_rule_line());
                }
                _ => {}
            }
        }

        // Flush remaining spans.
        flush_spans(&mut current_spans, &mut lines);
        lines
    }

    /// Convert markdown to styled lines, each prefixed with `indent` spaces.
    pub fn to_lines_indented(&self, indent: usize) -> Vec<Line<'a>> {
        let prefix: String = " ".repeat(indent);
        self.to_lines()
            .into_iter()
            .map(|line| {
                let mut spans = vec![Span::raw(prefix.clone())];
                spans.extend(line.spans);
                Line::from(spans)
            })
            .collect()
    }
}

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H2 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H3 => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().add_modifier(Modifier::BOLD),
    }
}

fn flush_spans<'a>(spans: &mut Vec<Span<'a>>, lines: &mut Vec<Line<'a>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

/// Parse markdown source into owned `Line<'static>` values.
///
/// Used by `MarkdownStreamCollector` so rendered lines can outlive the source
/// string. This is a free function (no `MarkdownView` instance needed).
pub fn parse_to_owned_lines(source: &str) -> Vec<Line<'static>> {
    let parser = Parser::new(source);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_block_lines: Vec<String> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    style_stack.push(heading_style(level));
                }
                Tag::Strong => {
                    let base = *style_stack.last().unwrap_or(&Style::default());
                    style_stack.push(base.add_modifier(Modifier::BOLD));
                }
                Tag::Emphasis => {
                    let base = *style_stack.last().unwrap_or(&Style::default());
                    style_stack.push(base.add_modifier(Modifier::ITALIC));
                }
                Tag::CodeBlock(kind) => {
                    in_code_block = true;
                    code_block_lang = match kind {
                        CodeBlockKind::Fenced(lang) => lang.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    code_block_lines.clear();
                }
                Tag::Link { .. } => {
                    let base = *style_stack.last().unwrap_or(&Style::default());
                    style_stack.push(base.fg(Color::Blue).add_modifier(Modifier::UNDERLINED));
                }
                Tag::Item => {
                    current_spans.push(Span::styled(
                        "• ".to_string(),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    style_stack.pop();
                    flush_owned_spans(&mut current_spans, &mut lines);
                    lines.push(Line::from(""));
                }
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link => {
                    style_stack.pop();
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    render_code_block_boxed(&code_block_lines, &code_block_lang, &mut lines);
                    lines.push(Line::from(""));
                    code_block_lines.clear();
                }
                TagEnd::Paragraph => {
                    flush_owned_spans(&mut current_spans, &mut lines);
                    lines.push(Line::from(""));
                }
                TagEnd::Item => {
                    flush_owned_spans(&mut current_spans, &mut lines);
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    for line in text.lines() {
                        code_block_lines.push(line.to_string());
                    }
                } else {
                    let style = *style_stack.last().unwrap_or(&Style::default());
                    current_spans.extend(spans_from_text_with_urls(&text, style));
                }
            }
            Event::Code(code) => {
                let code_style = Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Rgb(40, 40, 40));
                current_spans.push(Span::styled(format!("`{code}`"), code_style));
            }
            Event::SoftBreak | Event::HardBreak => {
                flush_owned_spans(&mut current_spans, &mut lines);
            }
            Event::Rule => {
                flush_owned_spans(&mut current_spans, &mut lines);
                lines.push(horizontal_rule_line());
            }
            _ => {}
        }
    }

    flush_owned_spans(&mut current_spans, &mut lines);
    // Remove trailing empty line if present.
    if lines.last().is_some_and(|l| l.spans.is_empty()) {
        lines.pop();
    }
    lines
}

/// Flush owned spans into a Line<'static>.
fn flush_owned_spans(spans: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

/// Render a code block with box-drawing borders and optional language label.
///
/// Output format:
/// ```text
///   ┌─ rust ──────────────────────┐
///   │ fn main() {}                │
///   └─────────────────────────────┘
/// ```
fn render_code_block_boxed(
    code_lines: &[String],
    lang: &str,
    output: &mut Vec<Line<'static>>,
) {
    let border_style = Style::default().fg(Color::DarkGray);
    let label_style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
    let code_bg = Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 40));

    // Top border: ┌─ lang ──...──┐
    let label = if lang.is_empty() { String::new() } else { format!(" {lang} ") };
    let used = 4 + label.len(); // "  ┌─" + label
    let fill = CODE_BLOCK_WIDTH.saturating_sub(used);
    output.push(Line::from(vec![
        Span::styled("  \u{250c}\u{2500}".to_string(), border_style), // ┌─
        Span::styled(label, label_style),
        Span::styled("\u{2500}".repeat(fill) + "\u{2510}", border_style), // ─...─┐
    ]));

    // Highlighted or plain code lines with │ prefix and │ suffix.
    let code = code_lines.join("\n");
    if let Some(highlighted) = highlight::highlight_code_inline(&code, lang) {
        for hl_line in highlighted {
            let mut spans = Vec::with_capacity(hl_line.spans.len() + 2);
            spans.push(Span::styled("  \u{2502} ".to_string(), border_style)); // │
            spans.extend(hl_line.spans);
            output.push(Line::from(spans));
        }
    } else {
        for cl in code_lines {
            output.push(Line::from(vec![
                Span::styled("  \u{2502} ".to_string(), border_style),
                Span::styled(cl.clone(), code_bg),
            ]));
        }
    }

    // Bottom border: └──...──┘
    let bottom_fill = CODE_BLOCK_WIDTH.saturating_sub(3); // "  └" prefix
    output.push(Line::from(Span::styled(
        format!("  \u{2514}{}\u{2518}", "\u{2500}".repeat(bottom_fill)),
        border_style,
    )));
}

/// Persistent state for incremental streaming markdown parsing.
///
/// Tracks whether we are inside a fenced code block so that partial code
/// blocks spanning multiple delta batches render correctly.
#[derive(Default)]
pub struct StreamParserState {
    /// True when a code fence has been opened but not yet closed.
    pub in_code_block: bool,
    /// Language tag from the opening fence.
    pub code_lang: String,
    /// Accumulated code lines inside the open fence.
    pub code_lines: Vec<String>,
}

/// Parse a slice of markdown incrementally, carrying `state` across calls.
///
/// This handles the cross-line code fence construct — all other markdown
/// elements (bold, italic, headings, lists) are line-local in LLM output and
/// parse correctly per-slice.
pub fn parse_incremental(source: &str, state: &mut StreamParserState) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for raw_line in source.lines() {
        let trimmed = raw_line.trim();

        // Check for code fence toggle (``` with optional language).
        if trimmed.starts_with("```") {
            if state.in_code_block {
                // Closing fence — render accumulated code with box-drawing borders.
                render_code_block_boxed(&state.code_lines, &state.code_lang, &mut lines);
                lines.push(Line::from(""));
                state.in_code_block = false;
                state.code_lang.clear();
                state.code_lines.clear();
            } else {
                // Opening fence — extract and normalise language tag.
                state.in_code_block = true;
                let raw_lang = trimmed.trim_start_matches('`');
                state.code_lang = normalize_lang(raw_lang).to_string();
                state.code_lines.clear();
            }
            continue;
        }

        if state.in_code_block {
            state.code_lines.push(raw_line.to_string());
            continue;
        }

        // Detect standalone horizontal rule lines: ---, ***, ___ (3+ same chars).
        if trimmed.len() >= 3
            && (trimmed.chars().all(|c| c == '-')
                || trimmed.chars().all(|c| c == '*')
                || trimmed.chars().all(|c| c == '_'))
        {
            lines.push(horizontal_rule_line());
            continue;
        }

        // Non-code content: parse as single-line markdown for styling.
        let parsed = parse_to_owned_lines(raw_line);
        if parsed.is_empty() && !raw_line.is_empty() {
            // If pulldown_cmark produced nothing, emit as plain text.
            lines.push(Line::from(Span::raw(raw_line.to_string())));
        } else {
            lines.extend(parsed);
        }
    }

    lines
}

impl Widget for MarkdownView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = self.to_lines();
        let text = ratatui::text::Text::from(lines);
        let paragraph =
            ratatui::widgets::Paragraph::new(text).wrap(ratatui::widgets::Wrap { trim: false });
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("")
    }

    #[allow(dead_code)]
    fn span_texts(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn plain_text_renders() {
        let v = MarkdownView::new("Hello, world!");
        let lines = v.to_lines();
        assert!(!lines.is_empty());
        assert!(text_of(&lines).contains("Hello, world!"));
    }

    #[test]
    fn bold_text_uses_bold_modifier() {
        let v = MarkdownView::new("**bold text**");
        let lines = v.to_lines();
        let raw = text_of(&lines);
        assert!(!raw.contains("**"), "Bold markers should be removed");
        assert!(raw.contains("bold text"));

        // Check BOLD modifier is applied.
        let bold_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.add_modifier.contains(Modifier::BOLD))
            .collect();
        assert!(!bold_spans.is_empty(), "Should have bold-styled spans");
    }

    #[test]
    fn italic_text_uses_italic_modifier() {
        let v = MarkdownView::new("*italic text*");
        let lines = v.to_lines();
        let italic_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.add_modifier.contains(Modifier::ITALIC))
            .collect();
        assert!(!italic_spans.is_empty(), "Should have italic-styled spans");
    }

    #[test]
    fn heading_renders_with_color() {
        let v = MarkdownView::new("# Heading 1");
        let lines = v.to_lines();
        let raw = text_of(&lines);
        assert!(raw.contains("Heading 1"), "Should contain heading text");

        // H1 should use Magenta color.
        let colored: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.fg == Some(Color::Magenta))
            .collect();
        assert!(!colored.is_empty(), "H1 should use Magenta color");
    }

    #[test]
    fn h2_renders_with_cyan() {
        let v = MarkdownView::new("## Sub Heading");
        let lines = v.to_lines();
        let colored: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.fg == Some(Color::Cyan))
            .collect();
        assert!(!colored.is_empty(), "H2 should use Cyan color");
    }

    #[test]
    fn code_block_renders_with_background() {
        let source = "```\nfn main() {}\n```";
        let v = MarkdownView::new(source);
        let lines = v.to_lines();
        let raw = text_of(&lines);
        assert!(
            raw.contains("fn main()"),
            "Code block content should render"
        );

        // Code should have dark background.
        let bg_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| matches!(s.style.bg, Some(Color::Rgb(40, 40, 40))))
            .collect();
        assert!(
            !bg_spans.is_empty(),
            "Code block should have background color"
        );
    }

    #[test]
    fn inline_code_renders_with_backticks() {
        let v = MarkdownView::new("Use `cargo test` to run.");
        let lines = v.to_lines();
        let raw = text_of(&lines);
        assert!(raw.contains("`cargo test`"), "Inline code should render");

        // Inline code should have yellow foreground.
        let code_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.fg == Some(Color::Yellow))
            .collect();
        assert!(!code_spans.is_empty(), "Inline code should be yellow");
    }

    #[test]
    fn list_items_have_bullet() {
        let v = MarkdownView::new("- Item one\n- Item two");
        let lines = v.to_lines();
        let raw = text_of(&lines);
        assert!(raw.contains("•"), "List items should have bullet char");
        assert!(raw.contains("Item one"));
        assert!(raw.contains("Item two"));
    }

    #[test]
    fn link_renders_with_underline() {
        let v = MarkdownView::new("[click here](https://example.com)");
        let lines = v.to_lines();
        let underline_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.add_modifier.contains(Modifier::UNDERLINED))
            .collect();
        assert!(!underline_spans.is_empty(), "Links should be underlined");
    }

    #[test]
    fn parse_to_owned_lines_works() {
        let lines = parse_to_owned_lines("**hello** world\n");
        assert!(!lines.is_empty());
        let raw: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(raw.contains("hello"));
        assert!(raw.contains("world"));
    }

    #[test]
    fn indented_lines_have_prefix() {
        let v = MarkdownView::new("test");
        let lines = v.to_lines_indented(4);
        let first_span = &lines[0].spans[0];
        assert_eq!(first_span.content.as_ref(), "    ");
    }

    // --- URL detection ---

    #[test]
    fn detect_urls_finds_https() {
        let ranges = detect_urls("Visit https://example.com for more.");
        assert_eq!(ranges.len(), 1);
        let (s, e) = ranges[0];
        assert_eq!(&"Visit https://example.com for more."[s..e], "https://example.com");
    }

    #[test]
    fn detect_urls_finds_http() {
        let ranges = detect_urls("see http://example.org ok");
        assert_eq!(ranges.len(), 1);
        let (s, e) = ranges[0];
        assert_eq!(&"see http://example.org ok"[s..e], "http://example.org");
    }

    #[test]
    fn detect_urls_stops_at_closing_paren() {
        let ranges = detect_urls("(https://example.com)");
        assert_eq!(ranges.len(), 1);
        let (s, e) = ranges[0];
        assert_eq!(&"(https://example.com)"[s..e], "https://example.com");
    }

    #[test]
    fn detect_urls_empty_when_none() {
        assert!(detect_urls("no urls here").is_empty());
    }

    #[test]
    fn detect_urls_multiple() {
        let text = "a https://foo.com b https://bar.org c";
        let ranges = detect_urls(text);
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[0].0..ranges[0].1], "https://foo.com");
        assert_eq!(&text[ranges[1].0..ranges[1].1], "https://bar.org");
    }

    #[test]
    fn url_in_plain_text_renders_cyan_underline() {
        let v = MarkdownView::new("Go to https://example.com now");
        let lines = v.to_lines();
        let url_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| {
                s.style.fg == Some(Color::Cyan)
                    && s.style.add_modifier.contains(Modifier::UNDERLINED)
            })
            .collect();
        assert!(!url_spans.is_empty(), "URL should be Cyan + Underlined");
        let url_text: String = url_spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(url_text.contains("https://example.com"));
    }

    #[test]
    fn url_parse_owned_lines_cyan_underline() {
        let lines = parse_to_owned_lines("See https://rust-lang.org for docs.");
        let url_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| {
                s.style.fg == Some(Color::Cyan)
                    && s.style.add_modifier.contains(Modifier::UNDERLINED)
            })
            .collect();
        assert!(!url_spans.is_empty(), "URL should be Cyan + Underlined");
    }

    // --- Horizontal rules ---

    #[test]
    fn horizontal_rule_dashes_renders() {
        // pulldown_cmark parses --- as a rule only when on its own paragraph.
        let v = MarkdownView::new("above\n\n---\n\nbelow");
        let lines = v.to_lines();
        let rule_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.content.contains('\u{2500}') && s.style.fg == Some(Color::DarkGray))
            .collect();
        assert!(!rule_spans.is_empty(), "Horizontal rule should render as ─ chars");
    }

    #[test]
    fn horizontal_rule_incremental_dashes() {
        let mut state = StreamParserState::default();
        let lines = parse_incremental("---", &mut state);
        let rule_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.content.contains('\u{2500}'))
            .collect();
        assert!(!rule_spans.is_empty(), "Incremental --- should render as horizontal rule");
    }

    #[test]
    fn horizontal_rule_incremental_underscores() {
        let mut state = StreamParserState::default();
        let lines = parse_incremental("___", &mut state);
        let rule_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.content.contains('\u{2500}'))
            .collect();
        assert!(!rule_spans.is_empty(), "Incremental ___ should render as horizontal rule");
    }

    // --- Code block language labels ---

    #[test]
    fn code_block_with_lang_shows_label_in_border() {
        let source = "```rust\nfn main() {}\n```";
        let v = MarkdownView::new(source);
        let lines = v.to_lines();
        let raw = text_of(&lines);
        // The top-border line should contain the language name.
        assert!(raw.contains("rust"), "Code block border should show language label");
    }

    #[test]
    fn code_block_incremental_lang_alias_normalised() {
        let mut state = StreamParserState::default();
        // Open fence with alias "rs", should normalise to "rust".
        parse_incremental("```rs", &mut state);
        assert_eq!(state.code_lang, "rust", "rs alias should normalise to rust");
    }

    #[test]
    fn normalize_lang_aliases() {
        assert_eq!(normalize_lang("rs"), "rust");
        assert_eq!(normalize_lang("py"), "python");
        assert_eq!(normalize_lang("js"), "javascript");
        assert_eq!(normalize_lang("ts"), "typescript");
        assert_eq!(normalize_lang("go"), "go");
    }
}
