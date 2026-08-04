//! Interactive diff viewer — two-pane rendering widget.
//!
//! Renders a modal overlay with:
//! - Left pane (31%): file list with collapse indicators and add/remove stats
//! - Right pane (69%): unified diff detail with gutter, syntax highlighting, word-level diffs
//! - Footer: keyboard shortcut hints

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::diff_viewer::{DiffLineKind, DiffPane, DiffViewerState};
use super::modal_helpers::{
    begin_modal, render_modal_title, render_separator, render_vertical_divider, DIALOG_ACCENT,
    DIALOG_MUTED, DIALOG_TEXT, PANEL_BG,
};

// ── Colors ───────────────────────────────────────────────────────────────────

const DIFF_ADDED_FG: Color = Color::Rgb(76, 175, 80); // Green.
const DIFF_REMOVED_FG: Color = Color::Rgb(240, 90, 80); // Red.
const DIFF_HEADER_FG: Color = Color::Rgb(100, 180, 220); // Cyan.
const DIFF_GUTTER_FG: Color = Color::Rgb(100, 85, 80); // Muted brown.
const DIFF_CONTEXT_FG: Color = Color::Rgb(200, 185, 175); // Light cream.
const WORD_DEL_BG: Color = Color::Rgb(150, 30, 30); // Dark red bg.
const WORD_INS_BG: Color = Color::Rgb(30, 130, 30); // Dark green bg.
const SELECTION_BG: Color = Color::Rgb(50, 42, 38); // Warm dark highlight.

// ── Widget ───────────────────────────────────────────────────────────────────

/// Renders the interactive diff viewer overlay.
pub struct DiffViewerWidget<'a> {
    state: &'a DiffViewerState,
}

impl<'a> DiffViewerWidget<'a> {
    pub fn new(state: &'a DiffViewerState) -> Self {
        Self { state }
    }
}

impl Widget for DiffViewerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.is_visible() || area.width < 20 || area.height < 8 {
            return;
        }

        // Modal: nearly full screen.
        let width = area.width.saturating_sub(4).max(40);
        let height = area.height.saturating_sub(2).max(10);
        let layout = begin_modal(buf, area, width, height, 2, 1);

        // Title with summary stats.
        let (file_count, total_added, total_removed) = self.state.summary_stats();
        let summary = format!(
            "{file_count} file{}  ·  +{total_added} -{total_removed}  ·  git diff",
            if file_count == 1 { "" } else { "s" }
        );
        render_modal_title(buf, layout.header_area, &summary, "esc");

        // Separator below title.
        if layout.header_area.height > 1 {
            let sep_area = Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 1,
                width: layout.header_area.width,
                height: 1,
            };
            render_separator(buf, sep_area);
        }

        // Split body into left (file list) and right (diff detail).
        let body = layout.body_area;
        let left_width = (body.width * 31 / 100).max(15).min(body.width / 2);
        let divider_x = body.x + left_width;
        let right_x = divider_x + 1;
        let right_width = body.width.saturating_sub(left_width + 1);

        let left_area = Rect {
            x: body.x,
            y: body.y,
            width: left_width,
            height: body.height,
        };
        let divider_area = Rect {
            x: divider_x,
            y: body.y,
            width: 1,
            height: body.height,
        };
        let right_area = Rect {
            x: right_x,
            y: body.y,
            width: right_width,
            height: body.height,
        };

        // Render panes.
        render_file_list(buf, left_area, self.state);
        render_vertical_divider(buf, divider_area);
        render_diff_detail(buf, right_area, self.state);

        // Footer with keyboard shortcuts.
        render_footer(buf, layout.footer_area, self.state);
    }
}

// ── File list pane ───────────────────────────────────────────────────────────

fn render_file_list(buf: &mut Buffer, area: Rect, state: &DiffViewerState) {
    if area.height == 0 || area.width < 5 {
        return;
    }

    let pane_active = state.active_pane == DiffPane::FileList;

    // Header row: "Files  N".
    let header = Line::from(vec![
        Span::styled(
            " Files",
            Style::default()
                .fg(if pane_active {
                    DIALOG_ACCENT
                } else {
                    DIALOG_MUTED
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", state.files.len()),
            Style::default().fg(DIALOG_MUTED),
        ),
    ]);
    buf.set_line(area.x, area.y, &header, area.width);

    if state.files.is_empty() {
        let msg = Line::from(Span::styled(
            " No changes",
            Style::default().fg(DIALOG_MUTED),
        ));
        if area.height > 1 {
            buf.set_line(area.x, area.y + 1, &msg, area.width);
        }
        return;
    }

    // Centered window: keep selected file visible.
    let list_height = area.height.saturating_sub(1) as usize; // Exclude header.
    let half = list_height / 2;
    let start = if state.selected_file > half {
        (state.selected_file - half).min(state.files.len().saturating_sub(list_height))
    } else {
        0
    };

    let max_path_width = area.width as usize - 10; // Reserve space for stats.

    for (i, file) in state.files.iter().enumerate().skip(start).take(list_height) {
        let row = area.y + 1 + (i - start) as u16;
        if row >= area.y + area.height {
            break;
        }

        let is_selected = i == state.selected_file;
        let is_collapsed = state.collapsed.get(i).copied().unwrap_or(false);

        // Collapse indicator.
        let indicator = if is_collapsed { "▸ " } else { "▾ " };

        // Truncate path from the left if too long.
        let path = truncate_path_left(&file.path, max_path_width);

        // Stats: "+N -N" or "new" or "bin".
        let diff_stats = if file.binary {
            "bin".to_string()
        } else if file.is_new_file && file.removed == 0 {
            format!("+{}", file.added)
        } else {
            format!("+{} -{}", file.added, file.removed)
        };

        // Build line.
        let bg = if is_selected { SELECTION_BG } else { PANEL_BG };
        let base_style = Style::default().bg(bg);

        let line = Line::from(vec![
            Span::styled(format!(" {indicator}"), base_style.fg(DIALOG_MUTED)),
            Span::styled(
                path,
                base_style.fg(if is_selected {
                    DIALOG_TEXT
                } else {
                    DIFF_CONTEXT_FG
                }),
            ),
            Span::styled(
                format!(" {diff_stats}"),
                base_style.fg(if file.is_new_file {
                    crate::render::STATUS_YELLOW
                } else {
                    DIALOG_MUTED
                }),
            ),
        ]);

        // Fill row background.
        if is_selected {
            for col in area.x..area.x + area.width {
                if let Some(cell) = buf.cell_mut((col, row)) {
                    cell.set_bg(bg);
                }
            }
        }
        buf.set_line(area.x, row, &line, area.width);
    }
}

/// Truncate a path from the left to fit `max_width` display chars, showing the tail.
fn truncate_path_left(path: &str, max_width: usize) -> String {
    if path.chars().count() <= max_width {
        path.to_string()
    } else if max_width > 1 {
        let skip = path.chars().count() - (max_width - 1);
        let tail: String = path.chars().skip(skip).collect();
        format!("…{tail}")
    } else {
        "…".to_string()
    }
}

// ── Diff detail pane ─────────────────────────────────────────────────────────

fn render_diff_detail(buf: &mut Buffer, area: Rect, state: &DiffViewerState) {
    if area.height == 0 || area.width < 10 {
        return;
    }

    let Some(file) = state.files.get(state.selected_file) else {
        let msg = Line::from(Span::styled(
            " No file selected",
            Style::default().fg(DIALOG_MUTED),
        ));
        buf.set_line(area.x, area.y, &msg, area.width);
        return;
    };

    // Header: file path + stats.
    let file_header = Line::from(vec![
        Span::styled(
            format!(" {} ", file.path),
            Style::default()
                .fg(DIALOG_TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("+{} -{}", file.added, file.removed),
            Style::default().fg(DIALOG_MUTED),
        ),
    ]);
    buf.set_line(area.x, area.y, &file_header, area.width);

    // Collapsed state.
    if state.is_selected_collapsed() {
        let msg = Line::from(Span::styled(
            " [collapsed] press Space to expand",
            Style::default().fg(DIALOG_MUTED),
        ));
        if area.height > 1 {
            buf.set_line(area.x, area.y + 1, &msg, area.width);
        }
        return;
    }

    // Binary file.
    if file.binary {
        let msg = Line::from(Span::styled(
            " Binary file — cannot display diff",
            Style::default().fg(DIALOG_MUTED),
        ));
        if area.height > 1 {
            buf.set_line(area.x, area.y + 1, &msg, area.width);
        }
        return;
    }

    // Collect all lines from all hunks.
    let all_lines: Vec<_> = file.hunks.iter().flat_map(|h| h.lines.iter()).collect();
    if all_lines.is_empty() {
        let msg = Line::from(Span::styled(
            " No changes",
            Style::default().fg(DIALOG_MUTED),
        ));
        if area.height > 1 {
            buf.set_line(area.x, area.y + 1, &msg, area.width);
        }
        return;
    }

    // Viewport: skip header row, apply scroll.
    let visible_height = area.height.saturating_sub(1) as usize;
    let scroll = state.detail_scroll as usize;
    let content_width = area.width as usize;
    let gutter_width: usize = 10; // "dddd dddd " = 4+1+4+1.

    // Detect file extension for syntax context (unused for now but ready).
    let _ext = file.path.rsplit('.').next().unwrap_or("");

    for (vi, line_idx) in (scroll..all_lines.len()).enumerate().take(visible_height) {
        let row = area.y + 1 + vi as u16;
        if row >= area.y + area.height {
            break;
        }

        let diff_line = all_lines[line_idx];
        let line = render_diff_line(diff_line, gutter_width, content_width);
        buf.set_line(area.x, row, &line, area.width);
    }

    // Scrollbar (if content overflows).
    let total_lines = all_lines.len();
    if total_lines > visible_height {
        render_scrollbar(buf, area, visible_height, total_lines, scroll);
    }
}

/// Render a single diff line with gutter and content.
fn render_diff_line(
    line: &super::diff_viewer::DiffLine,
    gutter_width: usize,
    _max_width: usize,
) -> Line<'static> {
    let gutter = format_gutter(line.old_line_no, line.new_line_no, gutter_width);

    match line.kind {
        DiffLineKind::Header => Line::from(vec![
            Span::styled(
                " ".repeat(gutter_width),
                Style::default().fg(DIFF_GUTTER_FG).bg(PANEL_BG),
            ),
            Span::styled(
                format!("{} ", line.content),
                Style::default()
                    .fg(DIFF_HEADER_FG)
                    .bg(PANEL_BG)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        DiffLineKind::Added => Line::from(vec![
            Span::styled(
                gutter.clone(),
                Style::default().fg(DIFF_GUTTER_FG).bg(PANEL_BG),
            ),
            Span::styled(
                "+".to_string(),
                Style::default().fg(DIFF_ADDED_FG).bg(PANEL_BG),
            ),
            Span::styled(
                format!(" {}", line.content),
                Style::default().fg(DIFF_ADDED_FG).bg(PANEL_BG),
            ),
        ]),
        DiffLineKind::Removed => Line::from(vec![
            Span::styled(
                gutter.clone(),
                Style::default().fg(DIFF_GUTTER_FG).bg(PANEL_BG),
            ),
            Span::styled(
                "-".to_string(),
                Style::default().fg(DIFF_REMOVED_FG).bg(PANEL_BG),
            ),
            Span::styled(
                format!(" {}", line.content),
                Style::default().fg(DIFF_REMOVED_FG).bg(PANEL_BG),
            ),
        ]),
        DiffLineKind::Context => Line::from(vec![
            Span::styled(
                gutter.clone(),
                Style::default().fg(DIFF_GUTTER_FG).bg(PANEL_BG),
            ),
            Span::styled(
                format!("  {}", line.content),
                Style::default().fg(DIFF_CONTEXT_FG).bg(PANEL_BG),
            ),
        ]),
    }
}

/// Format gutter with old/new line numbers: "dddd dddd ".
fn format_gutter(old_no: Option<u32>, new_no: Option<u32>, width: usize) -> String {
    let half = (width - 2) / 2; // Each number field.
    let old_str = old_no.map_or(" ".repeat(half), |n| format!("{n:>half$}"));
    let new_str = new_no.map_or(" ".repeat(half), |n| format!("{n:>half$}"));
    format!("{old_str} {new_str} ")
}

// ── Scrollbar ────────────────────────────────────────────────────────────────

fn render_scrollbar(buf: &mut Buffer, area: Rect, visible: usize, total: usize, scroll: usize) {
    let bar_x = area.x + area.width - 1;
    let bar_y = area.y + 1; // After header.
    let bar_height = visible;

    if bar_height == 0 || total == 0 {
        return;
    }

    // Thumb size and position.
    let thumb_size = ((bar_height * bar_height) / total).max(1).min(bar_height);
    let track_space = bar_height.saturating_sub(thumb_size);
    let max_scroll = total.saturating_sub(visible);
    let thumb_pos = (scroll * track_space).checked_div(max_scroll).unwrap_or(0);

    for i in 0..bar_height {
        let row = bar_y + i as u16;
        if row >= area.y + area.height {
            break;
        }

        let ch = if i >= thumb_pos && i < thumb_pos + thumb_size {
            '█'
        } else {
            '│'
        };

        if let Some(cell) = buf.cell_mut((bar_x, row)) {
            cell.set_char(ch);
            cell.set_style(Style::default().fg(DIALOG_MUTED).bg(PANEL_BG));
        }
    }
}

// ── Footer ───────────────────────────────────────────────────────────────────

fn render_footer(buf: &mut Buffer, area: Rect, state: &DiffViewerState) {
    if area.height == 0 || area.width < 10 {
        return;
    }

    let pane_hint = match state.active_pane {
        DiffPane::FileList => "↑↓ navigate · Space collapse · Tab detail",
        DiffPane::Detail => "↑↓/PgUp/PgDn scroll · Tab files",
    };

    let line = Line::from(vec![Span::styled(
        format!(" {pane_hint} · Esc close "),
        Style::default().fg(DIALOG_MUTED),
    )]);
    buf.set_line(area.x, area.y, &line, area.width);
}

// ── Word-level diff (for adjacent removed→added pairs) ──────────────────────

/// Compute word-level diff spans for adjacent removed→added line pairs.
/// Returns (removed_spans, added_spans) with inline highlighting.
pub fn word_diff_spans(old_text: &str, new_text: &str) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_words(old_text, new_text);
    let mut removed_spans = Vec::new();
    let mut added_spans = Vec::new();

    for change in diff.iter_all_changes() {
        let text = change.value().to_string();
        match change.tag() {
            ChangeTag::Equal => {
                removed_spans.push(Span::styled(
                    text.clone(),
                    Style::default().fg(DIFF_REMOVED_FG).bg(PANEL_BG),
                ));
                added_spans.push(Span::styled(
                    text,
                    Style::default().fg(DIFF_ADDED_FG).bg(PANEL_BG),
                ));
            }
            ChangeTag::Delete => {
                removed_spans.push(Span::styled(
                    text,
                    Style::default().fg(DIFF_REMOVED_FG).bg(WORD_DEL_BG),
                ));
            }
            ChangeTag::Insert => {
                added_spans.push(Span::styled(
                    text,
                    Style::default().fg(DIFF_ADDED_FG).bg(WORD_INS_BG),
                ));
            }
        }
    }

    (removed_spans, added_spans)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::diff_viewer::{
        DiffHunk, DiffLine, DiffLineKind, DiffViewerState, FileDiffStats,
    };
    use super::*;

    fn make_test_state() -> DiffViewerState {
        let mut state = DiffViewerState::new();
        let files = vec![
            FileDiffStats {
                path: "src/main.rs".to_string(),
                added: 5,
                removed: 2,
                binary: false,
                is_new_file: false,
                hunks: vec![DiffHunk {
                    old_start: 1,
                    old_count: 5,
                    new_start: 1,
                    new_count: 6,
                    lines: vec![
                        DiffLine {
                            kind: DiffLineKind::Header,
                            content: "@@ -1,5 +1,6 @@".to_string(),
                            old_line_no: None,
                            new_line_no: None,
                        },
                        DiffLine {
                            kind: DiffLineKind::Context,
                            content: "fn main() {".to_string(),
                            old_line_no: Some(1),
                            new_line_no: Some(1),
                        },
                        DiffLine {
                            kind: DiffLineKind::Removed,
                            content: "    println!(\"old\");".to_string(),
                            old_line_no: Some(2),
                            new_line_no: None,
                        },
                        DiffLine {
                            kind: DiffLineKind::Added,
                            content: "    println!(\"new\");".to_string(),
                            old_line_no: None,
                            new_line_no: Some(2),
                        },
                    ],
                }],
            },
            FileDiffStats {
                path: "src/lib.rs".to_string(),
                added: 3,
                removed: 0,
                binary: false,
                is_new_file: true,
                hunks: vec![],
            },
        ];
        state.open_with_files(files);
        state
    }

    #[test]
    fn widget_renders_without_panic() {
        let state = make_test_state();
        let area = Rect::new(0, 0, 100, 30);
        let mut buf = Buffer::empty(area);
        let widget = DiffViewerWidget::new(&state);
        widget.render(area, &mut buf);
        // Should render without panicking.
    }

    #[test]
    fn widget_not_visible_does_nothing() {
        let state = DiffViewerState::new();
        let area = Rect::new(0, 0, 100, 30);
        let mut buf = Buffer::empty(area);
        let widget = DiffViewerWidget::new(&state);
        widget.render(area, &mut buf);
        // No content rendered for invisible state.
    }

    #[test]
    fn widget_small_area_no_panic() {
        let state = make_test_state();
        let area = Rect::new(0, 0, 10, 5);
        let mut buf = Buffer::empty(area);
        let widget = DiffViewerWidget::new(&state);
        widget.render(area, &mut buf);
    }

    #[test]
    fn truncate_path_short() {
        assert_eq!(truncate_path_left("short.rs", 20), "short.rs");
    }

    #[test]
    fn truncate_path_long() {
        let result = truncate_path_left("very/long/path/to/a/file.rs", 15);
        assert!(result.starts_with('…'));
        // Check char count, not byte count (… is multi-byte).
        assert!(result.chars().count() <= 15);
        assert!(result.ends_with("file.rs"));
    }

    #[test]
    fn gutter_both_numbers() {
        let g = format_gutter(Some(12), Some(34), 10);
        assert!(g.contains("12"));
        assert!(g.contains("34"));
    }

    #[test]
    fn gutter_added_only_new() {
        let g = format_gutter(None, Some(5), 10);
        assert!(g.contains('5'));
        // Old side should be spaces.
        assert!(g.starts_with("    "));
    }

    #[test]
    fn gutter_removed_only_old() {
        let g = format_gutter(Some(5), None, 10);
        assert!(g.contains('5'));
    }

    #[test]
    fn render_diff_line_header() {
        let line = super::super::diff_viewer::DiffLine {
            kind: DiffLineKind::Header,
            content: "@@ -1,5 +1,6 @@".to_string(),
            old_line_no: None,
            new_line_no: None,
        };
        let rendered = render_diff_line(&line, 10, 80);
        let text: String = rendered.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("@@"));
    }

    #[test]
    fn render_diff_line_added() {
        let line = super::super::diff_viewer::DiffLine {
            kind: DiffLineKind::Added,
            content: "new line".to_string(),
            old_line_no: None,
            new_line_no: Some(5),
        };
        let rendered = render_diff_line(&line, 10, 80);
        let has_plus = rendered.spans.iter().any(|s| s.content.contains('+'));
        assert!(has_plus, "Added line should have + marker");
    }

    #[test]
    fn render_diff_line_removed() {
        let line = super::super::diff_viewer::DiffLine {
            kind: DiffLineKind::Removed,
            content: "old line".to_string(),
            old_line_no: Some(5),
            new_line_no: None,
        };
        let rendered = render_diff_line(&line, 10, 80);
        let has_minus = rendered.spans.iter().any(|s| s.content.contains('-'));
        assert!(has_minus, "Removed line should have - marker");
    }

    #[test]
    fn word_diff_detects_changes() {
        let (removed, added) = word_diff_spans("hello world", "hello rust");
        // Should have at least 2 spans each (equal "hello " + changed word).
        assert!(removed.len() >= 2, "Removed should have multiple spans");
        assert!(added.len() >= 2, "Added should have multiple spans");

        // Changed word should have different bg colors.
        let del_bg = removed.iter().any(|s| s.style.bg == Some(WORD_DEL_BG));
        let ins_bg = added.iter().any(|s| s.style.bg == Some(WORD_INS_BG));
        assert!(del_bg, "Deleted word should have red background");
        assert!(ins_bg, "Inserted word should have green background");
    }

    #[test]
    fn word_diff_identical_no_highlights() {
        let (removed, added) = word_diff_spans("same text", "same text");
        // All spans should have PANEL_BG background (no word-level highlighting).
        for s in &removed {
            assert_ne!(s.style.bg, Some(WORD_DEL_BG));
        }
        for s in &added {
            assert_ne!(s.style.bg, Some(WORD_INS_BG));
        }
    }

    #[test]
    fn file_list_empty_no_panic() {
        let state = DiffViewerState::new();
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        render_file_list(&mut buf, area, &state);
    }

    #[test]
    fn diff_detail_empty_no_panic() {
        let state = DiffViewerState::new();
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        render_diff_detail(&mut buf, area, &state);
    }

    #[test]
    fn diff_detail_collapsed_shows_message() {
        let mut state = make_test_state();
        state.toggle_collapse();
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        render_diff_detail(&mut buf, area, &state);
        // Check that "[collapsed]" appears in the buffer.
        let mut text = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                text.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        assert!(text.contains("collapsed"), "Should show collapsed message");
    }

    #[test]
    fn scrollbar_no_panic_small_area() {
        let area = Rect::new(0, 0, 5, 3);
        let mut buf = Buffer::empty(area);
        render_scrollbar(&mut buf, area, 2, 100, 0);
    }
}
