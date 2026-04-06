use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

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
                    Tag::CodeBlock(_) => {
                        in_code_block = true;
                        code_block_lines.clear();
                    }
                    Tag::Link { .. } => {
                        let base = *style_stack.last().unwrap_or(&Style::default());
                        style_stack.push(base.fg(Color::Blue).add_modifier(Modifier::UNDERLINED));
                    }
                    Tag::Item => {
                        current_spans.push(Span::styled(
                            "• ",
                            Style::default().fg(Color::DarkGray),
                        ));
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
                        let code_style =
                            Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 40));
                        for code_line in &code_block_lines {
                            lines.push(Line::from(Span::styled(
                                format!("  {code_line}"),
                                code_style,
                            )));
                        }
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
                        current_spans.push(Span::styled(text.to_string(), style));
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
                Tag::CodeBlock(_) => {
                    in_code_block = true;
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
                    let code_style =
                        Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 40));
                    for code_line in &code_block_lines {
                        lines.push(Line::from(Span::styled(
                            format!("  {code_line}"),
                            code_style,
                        )));
                    }
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
                    current_spans.push(Span::styled(text.to_string(), style));
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
            _ => {}
        }
    }

    flush_owned_spans(&mut current_spans, &mut lines);
    // Remove trailing empty line if present.
    if lines.last().map_or(false, |l| l.spans.is_empty()) {
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

impl Widget for MarkdownView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = self.to_lines();
        let text = ratatui::text::Text::from(lines);
        let paragraph =
            ratatui::widgets::Paragraph::new(text).wrap(ratatui::widgets::Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
