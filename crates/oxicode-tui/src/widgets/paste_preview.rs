//! Paste preview overlay for large paste content.
//!
//! When a paste exceeds 5 lines, show a preview modal so the user can
//! confirm (Enter) or cancel (Esc) before inserting.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// Maximum lines shown in the preview before truncating.
const MAX_PREVIEW_LINES: usize = 20;

/// Minimum number of paste lines to trigger the preview modal.
pub const PASTE_PREVIEW_THRESHOLD: usize = 5;

/// Overlay widget for previewing large paste content.
pub struct PastePreview<'a> {
    text: &'a str,
}

impl<'a> PastePreview<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }
}

impl Widget for PastePreview<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Compute centered overlay area (60% width, 50% height, min 10x5).
        let overlay_w = (area.width * 60 / 100).max(10).min(area.width);
        let overlay_h = (area.height * 50 / 100).max(5).min(area.height);
        let x = area.x + (area.width.saturating_sub(overlay_w)) / 2;
        let y = area.y + (area.height.saturating_sub(overlay_h)) / 2;
        let overlay = Rect::new(x, y, overlay_w, overlay_h);

        // Clear the overlay area.
        Clear.render(overlay, buf);

        let total_lines = self.text.lines().count();
        let title = format!(" Paste Preview ({total_lines} lines) ");

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(title)
            .title_alignment(Alignment::Center);

        // Build preview lines with line numbers.
        let mut lines: Vec<Line<'_>> = self
            .text
            .lines()
            .take(MAX_PREVIEW_LINES)
            .enumerate()
            .map(|(i, line)| {
                Line::from(vec![
                    Span::styled(
                        format!("{:>3} ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(line),
                ])
            })
            .collect();

        // Truncation indicator.
        if total_lines > MAX_PREVIEW_LINES {
            lines.push(Line::from(Span::styled(
                format!("    ... {} more lines", total_lines - MAX_PREVIEW_LINES),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        // Footer hint.
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to paste | "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to cancel"),
        ]));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(overlay, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn small_paste_below_threshold() {
        let text = "line1\nline2\nline3";
        assert!(text.lines().count() < PASTE_PREVIEW_THRESHOLD);
    }

    #[test]
    fn large_paste_above_threshold() {
        let text = (0..10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.lines().count() >= PASTE_PREVIEW_THRESHOLD);
    }

    #[test]
    fn preview_renders_without_panic() {
        let text = (0..25)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        PastePreview::new(&text).render(area, &mut buf);
        // Scan all cells for title text (overlay is centered).
        let mut all_text = String::new();
        for y in 0..20 {
            for x in 0..60 {
                all_text.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        assert!(
            all_text.contains("25 lines"),
            "Title should show line count"
        );
    }
}
