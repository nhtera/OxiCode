//! Enhanced tool call display helpers.
//!
//! Provides animated Braille spinner frames, elapsed time formatting, and
//! tool-specific styled line builders for the TUI message view.

use std::time::Instant;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Braille spinner frames (10-frame cycle at 100ms = 1s period).
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Get the current spinner frame based on time.
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
        let mins = (secs / 60.0) as u64;
        let rem = secs as u64 % 60;
        format!("{mins}m {rem}s")
    } else {
        let hours = (secs / 3600.0) as u64;
        let mins = (secs as u64 % 3600) / 60;
        format!("{hours}h {mins}m")
    }
}

/// Build a styled line for a running tool call.
pub fn running_tool_line(
    name: &str,
    input_summary: &str,
    started_at: Instant,
) -> Line<'static> {
    let frame = spinner_frame(started_at);
    let elapsed = format_elapsed(started_at);
    Line::from(vec![
        Span::styled(
            format!("  {frame} "),
            Style::default().fg(Color::Cyan),
        ),
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

/// Build styled lines for a completed tool call.
pub fn completed_tool_lines(
    name: &str,
    input_summary: &str,
    content: &str,
    is_error: bool,
    started_at: Option<Instant>,
    max_result_lines: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (icon, header_color) = if is_error {
        ("\u{2717}", Color::Red) // ✗ red
    } else {
        ("\u{2713}", Color::Green) // ✓ green
    };

    let elapsed_str = started_at.map_or(String::new(), |t| {
        format!(" ({})", format_elapsed(t))
    });

    // Header line.
    lines.push(Line::from(vec![
        Span::styled(format!("  {icon} "), Style::default().fg(header_color)),
        Span::styled(
            name.to_string(),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" \u{2014} {input_summary}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(elapsed_str, Style::default().fg(Color::DarkGray)),
    ]));

    // Result lines with │ prefix (truncated).
    let result_fg = if is_error { Color::Red } else { Color::DarkGray };
    let result_style = Style::default().fg(result_fg);
    let pipe_style = Style::default().fg(Color::DarkGray);
    let total_lines = content.lines().count();
    for (i, line) in content.lines().enumerate() {
        if i >= max_result_lines {
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

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_cycles_through_frames() {
        // All frames should be valid Unicode.
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
        let raw: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(raw.contains("✓"), "Should have success icon");
        assert!(raw.contains("file_read"));
    }

    #[test]
    fn completed_tool_lines_error() {
        let lines = completed_tool_lines(
            "bash",
            "rm -rf /",
            "Permission denied",
            true,
            None,
            5,
        );
        let raw: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(raw.contains("✗"), "Should have error icon");
    }

    #[test]
    fn completed_tool_truncates_long_output() {
        let long_output = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = completed_tool_lines("bash", "cmd", &long_output, false, None, 3);
        let raw: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(raw.contains("more lines"), "Should show truncation indicator");
    }
}
