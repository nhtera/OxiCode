//! Enhanced tool call display helpers.
//!
//! Provides animated spinner frames, elapsed time formatting, and
//! tool-specific styled line builders for the TUI message view.
//!
//! Tool-specific renderers dispatch by tool name to show contextual info:
//! - Bash: `$ command` header with stdout/stderr distinction
//! - File tools: file path in cyan with content preview
//! - Edit: diff-style old/new display
//! - Search tools: pattern + match/file counts
//! - Generic fallback for unknown tools

use std::time::Instant;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

/// Braille spinner frames (10-frame cycle at 100ms = 1s period).
/// Used as a fallback for time-based spinner when frame_count is unavailable.
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Default max lines of tool output shown before truncation.
const DEFAULT_MAX_LINES: usize = 10;

/// Get the current spinner frame based on time (fallback, for backward compat).
pub fn spinner_frame(started_at: Instant) -> &'static str {
    let elapsed_ms = started_at.elapsed().as_millis() as usize;
    let idx = (elapsed_ms / 100) % SPINNER_FRAMES.len();
    SPINNER_FRAMES[idx]
}

/// Format elapsed duration as a human-readable string.
///
/// - `< 1s` → `"0.Ns"`
/// - `< 60s` → `"N.Ns"`
/// - `< 1h` → `"Nm Ns"`
/// - `≥ 1h` → `"Nh Nm"`
pub fn format_elapsed(started_at: Instant) -> String {
    let secs = started_at.elapsed().as_secs_f64();
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else if secs < 3600.0 {
        #[allow(clippy::cast_sign_loss)]
        let total = secs as u64;
        let mins = total / 60;
        let rem = total % 60;
        format!("{mins}m {rem}s")
    } else {
        #[allow(clippy::cast_sign_loss)]
        let total = secs as u64;
        let hours = total / 3600;
        let mins = (total % 3600) / 60;
        format!("{hours}h {mins}m")
    }
}

/// Build a styled line for a running tool call (uses time-based Braille spinner).
pub fn running_tool_line(name: &str, input_summary: &str, started_at: Instant) -> Line<'static> {
    let frame = spinner_frame(started_at);
    let elapsed = format_elapsed(started_at);
    Line::from(vec![
        Span::styled(format!("  {frame} "), Style::default().fg(Color::Cyan)),
        Span::styled(
            name.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" \u{2014} {input_summary}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!(" ({elapsed})"),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

/// Build a styled line for a running tool call with animated Unicode spinner.
///
/// Uses the 12-frame spinner from `render.rs` and stall detection coloring.
pub fn running_tool_line_animated(
    name: &str,
    input_summary: &str,
    started_at: Instant,
    frame_count: u64,
    stall_start: Option<Instant>,
) -> Line<'static> {
    let spinner = crate::render::spinner_char(frame_count);
    let color = crate::render::spinner_color(stall_start);
    let elapsed = format_elapsed(started_at);
    Line::from(vec![
        Span::styled(format!("  {spinner} "), Style::default().fg(color)),
        Span::styled(
            name.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" \u{2014} {input_summary}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!(" ({elapsed})"),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

/// Build styled lines for a completed tool call with tool-specific rendering.
///
/// Dispatches to specialized renderers for known tools (Bash, Read, Write,
/// Edit, Grep, Glob) and falls back to generic rendering for unknown tools.
pub fn completed_tool_lines(
    name: &str,
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    max_result_lines: usize,
) -> Vec<Line<'static>> {
    completed_tool_lines_with_input(name, input_summary, content, is_error, started_at, max_result_lines, None)
}

/// Build styled lines for a completed tool call with raw input JSON for
/// tool-specific rendering.
pub fn completed_tool_lines_with_input(
    name: &str,
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    max_result_lines: usize,
    raw_input: Option<&Value>,
) -> Vec<Line<'static>> {
    let max = if max_result_lines == 0 { DEFAULT_MAX_LINES } else { max_result_lines };
    match name {
        "Bash" | "bash" => render_bash(input_summary, content, is_error, started_at, max, raw_input),
        "Read" | "file_read" => render_file_read(input_summary, content, is_error, started_at, max, raw_input),
        "Write" | "file_write" => render_file_write(input_summary, content, is_error, started_at, raw_input),
        "Edit" | "file_edit" => render_edit(input_summary, content, is_error, started_at, raw_input),
        "Grep" | "grep" => render_search("Grep", input_summary, content, is_error, started_at, max, raw_input),
        "Glob" | "glob" => render_search("Glob", input_summary, content, is_error, started_at, max, raw_input),
        _ => render_generic(name, input_summary, content, is_error, started_at, max),
    }
}

// ─── Header helper ──────────────────────────────────────────────────────────

/// Build the standard tool header line with icon, name, summary, and elapsed.
fn tool_header(icon: &str, color: Color, name: &str, summary: &str, started_at: Option<Instant>) -> Line<'static> {
    let elapsed_str = started_at.map_or(String::new(), |t| format!(" ({})", format_elapsed(t)));
    Line::from(vec![
        Span::styled(format!("  {icon} "), Style::default().fg(color)),
        Span::styled(
            name.to_string(),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" \u{2014} {summary}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(elapsed_str, Style::default().fg(Color::DarkGray)),
    ])
}

/// Append truncated output lines with `│` prefix.
fn append_output_lines(
    lines: &mut Vec<Line<'static>>,
    content: &str,
    max: usize,
    style: Style,
) {
    let pipe_style = Style::default().fg(Color::DarkGray);
    let total = content.lines().count();
    for (i, line) in content.lines().enumerate() {
        if i >= max {
            lines.push(Line::from(Span::styled(
                format!("  \u{2502} ... ({} more lines)", total - i),
                pipe_style,
            )));
            break;
        }
        lines.push(Line::from(vec![
            Span::styled("  \u{2502} ".to_string(), pipe_style),
            Span::styled(line.to_string(), style),
        ]));
    }
}

fn status_icon(is_error: bool) -> (&'static str, Color) {
    if is_error {
        ("\u{2717}", Color::Red)
    } else {
        ("\u{2713}", Color::Green)
    }
}

// ─── Bash renderer ──────────────────────────────────────────────────────────

fn render_bash(
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    max: usize,
    raw_input: Option<&Value>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (icon, color) = status_icon(is_error);

    // Extract command from raw input or fall back to input_summary.
    let command = raw_input
        .and_then(|v| v.get("command"))
        .and_then(Value::as_str)
        .unwrap_or(input_summary);

    // Header: ✓ Bash — $ command
    let cmd_display = if command.len() > 60 {
        format!("$ {}...", &command[..57])
    } else {
        format!("$ {command}")
    };
    lines.push(tool_header(icon, color, "Bash", &cmd_display, started_at));

    // Output with error coloring.
    if !content.is_empty() {
        let result_style = if is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        append_output_lines(&mut lines, content, max, result_style);
    }
    lines
}

// ─── File read renderer ─────────────────────────────────────────────────────

fn render_file_read(
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    max: usize,
    raw_input: Option<&Value>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (icon, color) = status_icon(is_error);

    let file_path = raw_input
        .and_then(|v| v.get("file_path"))
        .and_then(Value::as_str)
        .unwrap_or(input_summary);

    let line_count = content.lines().count();
    let summary = format!("{file_path} ({line_count} lines)");
    lines.push(tool_header(icon, color, "Read", &summary, started_at));

    if !content.is_empty() && !is_error {
        let preview_max = max.min(5); // Show fewer preview lines for file reads.
        append_output_lines(
            &mut lines,
            content,
            preview_max,
            Style::default().fg(Color::DarkGray),
        );
    } else if is_error {
        append_output_lines(
            &mut lines,
            content,
            max,
            Style::default().fg(Color::Red),
        );
    }
    lines
}

// ─── File write renderer ────────────────────────────────────────────────────

fn render_file_write(
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    raw_input: Option<&Value>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (icon, color) = status_icon(is_error);

    let file_path = raw_input
        .and_then(|v| v.get("file_path"))
        .and_then(Value::as_str)
        .unwrap_or(input_summary);

    let summary = if is_error {
        format!("Failed: {file_path}")
    } else {
        format!("Created {file_path}")
    };
    lines.push(tool_header(icon, color, "Write", &summary, started_at));

    if is_error && !content.is_empty() {
        append_output_lines(&mut lines, content, 3, Style::default().fg(Color::Red));
    }
    lines
}

// ─── Edit renderer ──────────────────────────────────────────────────────────

fn render_edit(
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    raw_input: Option<&Value>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (icon, color) = status_icon(is_error);

    let file_path = raw_input
        .and_then(|v| v.get("file_path"))
        .and_then(Value::as_str)
        .unwrap_or(input_summary);

    lines.push(tool_header(icon, color, "Edit", file_path, started_at));

    // Show brief diff from input if available.
    if let Some(input) = raw_input {
        let old = input.get("old_string").and_then(Value::as_str);
        let new = input.get("new_string").and_then(Value::as_str);
        if let (Some(old_str), Some(new_str)) = (old, new) {
            let pipe_style = Style::default().fg(Color::DarkGray);
            let del_style = Style::default().fg(Color::Red);
            let add_style = Style::default().fg(Color::Green);

            // Show up to 3 old lines and 3 new lines.
            for line in old_str.lines().take(3) {
                lines.push(Line::from(vec![
                    Span::styled("  \u{2502} ".to_string(), pipe_style),
                    Span::styled(format!("-{line}"), del_style),
                ]));
            }
            if old_str.lines().count() > 3 {
                lines.push(Line::from(Span::styled(
                    format!("  \u{2502} ... ({} more removed)", old_str.lines().count() - 3),
                    pipe_style,
                )));
            }
            for line in new_str.lines().take(3) {
                lines.push(Line::from(vec![
                    Span::styled("  \u{2502} ".to_string(), pipe_style),
                    Span::styled(format!("+{line}"), add_style),
                ]));
            }
            if new_str.lines().count() > 3 {
                lines.push(Line::from(Span::styled(
                    format!("  \u{2502} ... ({} more added)", new_str.lines().count() - 3),
                    pipe_style,
                )));
            }
        }
    }

    if is_error && !content.is_empty() {
        append_output_lines(&mut lines, content, 3, Style::default().fg(Color::Red));
    }
    lines
}

// ─── Search renderers (Grep, Glob) ─────────────────────────────────────────

fn render_search(
    tool_label: &str,
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    max: usize,
    raw_input: Option<&Value>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (icon, color) = status_icon(is_error);

    // Build summary from raw input when possible.
    let summary = if let Some(input) = raw_input {
        if tool_label == "Grep" {
            let pattern = input.get("pattern").and_then(Value::as_str).unwrap_or("?");
            let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
            let file_count = content.lines().count();
            format!("\"{pattern}\" in {path} \u{2192} {file_count} results")
        } else {
            // Glob
            let pattern = input.get("pattern").and_then(Value::as_str).unwrap_or("?");
            let file_count = content.lines().count();
            format!("\"{pattern}\" \u{2192} {file_count} files")
        }
    } else {
        let file_count = content.lines().count();
        format!("{input_summary} \u{2192} {file_count} results")
    };

    lines.push(tool_header(icon, color, tool_label, &summary, started_at));

    if !content.is_empty() && !is_error {
        append_output_lines(
            &mut lines,
            content,
            max,
            Style::default().fg(Color::DarkGray),
        );
    } else if is_error {
        append_output_lines(&mut lines, content, max, Style::default().fg(Color::Red));
    }
    lines
}

// ─── Generic fallback ───────────────────────────────────────────────────────

fn render_generic(
    name: &str,
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    max: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (icon, color) = status_icon(is_error);
    lines.push(tool_header(icon, color, name, input_summary, started_at));

    if !content.is_empty() {
        let result_style = if is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        append_output_lines(&mut lines, content, max, result_style);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_to_string(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn spinner_cycles_through_frames() {
        for frame in SPINNER_FRAMES {
            assert!(!frame.is_empty());
        }
    }

    #[test]
    fn format_elapsed_subsecond() {
        let now = Instant::now();
        let s = format_elapsed(now);
        assert!(s.ends_with('s'), "Should end with 's', got: {s}");
        assert!(s.contains('.'), "Should have decimal, got: {s}");
    }

    #[test]
    fn running_tool_line_has_spinner_and_elapsed() {
        let started = Instant::now();
        let line = running_tool_line("bash", "echo hello", started);
        let raw: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(raw.contains("bash"), "Should contain tool name");
        assert!(raw.contains("echo hello"), "Should contain input summary");
        assert!(raw.contains('s'), "Should contain elapsed time");
    }

    #[test]
    fn completed_tool_lines_success() {
        let lines = completed_tool_lines(
            "file_read",
            "/tmp/test.txt",
            "file contents here\nline 2",
            false,
            None,
            5,
        );
        assert!(!lines.is_empty());
        let raw = lines_to_string(&lines);
        assert!(raw.contains("✓"), "Should have success icon");
        assert!(raw.contains("Read"));
    }

    #[test]
    fn completed_tool_lines_error() {
        let lines = completed_tool_lines("bash", "rm -rf /", "Permission denied", true, None, 5);
        let raw = lines_to_string(&lines);
        assert!(raw.contains("✗"), "Should have error icon");
    }

    #[test]
    fn completed_tool_truncates_long_output() {
        let long_output = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = completed_tool_lines("bash", "cmd", &long_output, false, None, 3);
        let raw = lines_to_string(&lines);
        assert!(raw.contains("more lines"), "Should show truncation indicator");
    }

    #[test]
    fn bash_shows_dollar_command() {
        let input = serde_json::json!({"command": "echo hello world"});
        let lines = completed_tool_lines_with_input(
            "Bash", "echo hello world", "hello world", false, None, 10, Some(&input),
        );
        let raw = lines_to_string(&lines);
        assert!(raw.contains("$ echo hello world"), "Bash should show $ command");
        assert!(raw.contains("Bash"), "Should show Bash label");
    }

    #[test]
    fn file_read_shows_path_and_line_count() {
        let input = serde_json::json!({"file_path": "/src/main.rs"});
        let content = "fn main() {\n    println!(\"hello\");\n}";
        let lines = completed_tool_lines_with_input(
            "Read", "/src/main.rs", content, false, None, 10, Some(&input),
        );
        let raw = lines_to_string(&lines);
        assert!(raw.contains("/src/main.rs"), "Should show file path");
        assert!(raw.contains("3 lines"), "Should show line count");
    }

    #[test]
    fn file_write_shows_created() {
        let input = serde_json::json!({"file_path": "/tmp/new.txt", "content": "hello"});
        let lines = completed_tool_lines_with_input(
            "Write", "/tmp/new.txt", "", false, None, 10, Some(&input),
        );
        let raw = lines_to_string(&lines);
        assert!(raw.contains("Created"), "Write should show Created");
        assert!(raw.contains("/tmp/new.txt"), "Should show file path");
    }

    #[test]
    fn edit_shows_diff_preview() {
        let input = serde_json::json!({
            "file_path": "/src/lib.rs",
            "old_string": "old line 1\nold line 2",
            "new_string": "new line 1\nnew line 2\nnew line 3",
        });
        let lines = completed_tool_lines_with_input(
            "Edit", "/src/lib.rs", "ok", false, None, 10, Some(&input),
        );
        let raw = lines_to_string(&lines);
        assert!(raw.contains("-old line"), "Should show removed lines");
        assert!(raw.contains("+new line"), "Should show added lines");
        assert!(raw.contains("Edit"), "Should show Edit label");
    }

    #[test]
    fn grep_shows_pattern_and_result_count() {
        let input = serde_json::json!({"pattern": "fn main", "path": "src/"});
        let content = "src/main.rs\nsrc/lib.rs\nsrc/app.rs";
        let lines = completed_tool_lines_with_input(
            "Grep", "fn main in src/", content, false, None, 10, Some(&input),
        );
        let raw = lines_to_string(&lines);
        assert!(raw.contains("\"fn main\""), "Should show pattern");
        assert!(raw.contains("3 results"), "Should show result count");
    }

    #[test]
    fn glob_shows_file_count() {
        let input = serde_json::json!({"pattern": "*.rs"});
        let content = "src/main.rs\nsrc/lib.rs";
        let lines = completed_tool_lines_with_input(
            "Glob", "*.rs", content, false, None, 10, Some(&input),
        );
        let raw = lines_to_string(&lines);
        assert!(raw.contains("2 files"), "Should show file count");
    }

    #[test]
    fn generic_fallback_for_unknown_tool() {
        let lines = completed_tool_lines_with_input(
            "CustomTool", "some input", "output", false, None, 10, None,
        );
        let raw = lines_to_string(&lines);
        assert!(raw.contains("CustomTool"), "Should show tool name");
        assert!(raw.contains("output"), "Should show output");
    }
}
