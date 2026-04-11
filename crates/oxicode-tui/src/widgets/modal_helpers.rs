//! Reusable modal overlay helpers — centering, darkening, dialog background.
//!
//! These primitives are shared by all overlay widgets (help, search, permission, etc.)
//! Pattern: darken screen → clear dialog area → fill background → render content.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Widget};

// ── Color palette ────────────────────────────────────────────────────────────

/// Semi-transparent overlay background (darkened warm charcoal).
pub const OVERLAY_BG: Color = Color::Rgb(12, 10, 8);

/// Dialog panel background — warm dark charcoal.
pub const PANEL_BG: Color = Color::Rgb(25, 22, 20);

/// Primary text color inside dialogs — warm cream.
pub const DIALOG_TEXT: Color = Color::Rgb(240, 232, 224);

/// Muted/secondary text color — rose-gray.
pub const DIALOG_MUTED: Color = Color::Rgb(140, 120, 115);

/// Accent color for headers and highlights — warm amber.
pub const DIALOG_ACCENT: Color = Color::Rgb(210, 140, 70);

// ── Geometry helpers ─────────────────────────────────────────────────────────

/// Compute a centered rectangle within `area`.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// Pre-computed layout areas for a modal dialog with header, body, and footer.
#[derive(Debug, Clone, Copy)]
pub struct ModalLayout {
    /// Outer dialog rect (includes border space).
    pub dialog_area: Rect,
    /// Inner content rect (1-cell padding from dialog edges).
    pub inner_area: Rect,
    /// Header row(s) at top of inner area.
    pub header_area: Rect,
    /// Main body content between header and footer.
    pub body_area: Rect,
    /// Footer row(s) at bottom of inner area.
    pub footer_area: Rect,
}

fn compute_modal_layout(
    area: Rect,
    width: u16,
    height: u16,
    header_height: u16,
    footer_height: u16,
) -> ModalLayout {
    let dialog_width = width.min(area.width.saturating_sub(4)).max(8);
    let dialog_height = height.min(area.height.saturating_sub(2)).max(6);
    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    let inner_area = Rect {
        x: dialog_area.x + 1,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(2),
        height: dialog_area.height.saturating_sub(2),
    };
    let header_h = header_height.min(inner_area.height);
    let footer_h = footer_height.min(inner_area.height.saturating_sub(header_h));
    let body_y = inner_area.y.saturating_add(header_h);
    let body_height = inner_area.height.saturating_sub(header_h + footer_h);
    ModalLayout {
        dialog_area,
        inner_area,
        header_area: Rect {
            x: inner_area.x,
            y: inner_area.y,
            width: inner_area.width,
            height: header_h,
        },
        body_area: Rect {
            x: inner_area.x,
            y: body_y,
            width: inner_area.width,
            height: body_height,
        },
        footer_area: Rect {
            x: inner_area.x,
            y: inner_area.y + inner_area.height.saturating_sub(footer_h),
            width: inner_area.width,
            height: footer_h,
        },
    }
}

// ── Rendering helpers ────────────────────────────────────────────────────────

/// Darken the entire screen area (call before rendering dialog content).
pub fn render_dark_overlay(buf: &mut Buffer, area: Rect) {
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_bg(OVERLAY_BG);
                cell.set_fg(DIALOG_MUTED);
            }
        }
    }
}

/// Fill a rectangle with the dialog panel background.
pub fn render_dialog_bg(buf: &mut Buffer, area: Rect) {
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(' ');
                cell.set_bg(PANEL_BG);
                cell.set_fg(DIALOG_TEXT);
            }
        }
    }
}

/// Set up a modal frame: darken screen, clear dialog area, fill background.
/// Returns the computed layout for placing header, body, and footer content.
pub fn begin_modal(
    buf: &mut Buffer,
    area: Rect,
    width: u16,
    height: u16,
    header_height: u16,
    footer_height: u16,
) -> ModalLayout {
    let layout = compute_modal_layout(area, width, height, header_height, footer_height);
    render_dark_overlay(buf, area);
    Clear.render(layout.dialog_area, buf);
    render_dialog_bg(buf, layout.dialog_area);
    layout
}

/// Render a modal title line: bold title on left, muted hint on right.
pub fn render_modal_title(buf: &mut Buffer, area: Rect, title: &str, right_hint: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let title_len = title.len() + 1; // " title"
    let hint_len = right_hint.len();
    let padding = (area.width as usize).saturating_sub(title_len + hint_len + 2);
    let line = Line::from(vec![
        Span::styled(
            format!(" {title}"),
            Style::default()
                .fg(DIALOG_TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(padding)),
        Span::styled(format!("{right_hint} "), Style::default().fg(DIALOG_MUTED)),
    ]);
    buf.set_line(area.x, area.y, &line, area.width);
}

/// Render a horizontal separator line using dim box-drawing characters.
pub fn render_separator(buf: &mut Buffer, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let sep: String = "\u{2500}".repeat(area.width as usize);
    let line = Line::from(Span::styled(sep, Style::default().fg(DIALOG_MUTED)));
    buf.set_line(area.x, area.y, &line, area.width);
}

/// Build a key→description line for the shortcuts column.
pub fn shortcut_line(key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {key:<14}"),
            Style::default()
                .fg(DIALOG_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc.to_string(), Style::default().fg(DIALOG_TEXT)),
    ])
}

/// Build a category header line.
pub fn category_header(name: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {name}"),
        Style::default()
            .fg(DIALOG_ACCENT)
            .add_modifier(Modifier::BOLD),
    ))
}

/// Render a vertical divider column (box-drawing │ character).
pub fn render_vertical_divider(buf: &mut Buffer, area: Rect) {
    for row in area.y..area.y + area.height {
        if let Some(cell) = buf.cell_mut((area.x, row)) {
            cell.set_char('\u{2502}');
            cell.set_style(Style::default().fg(DIALOG_MUTED).bg(PANEL_BG));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_rect_basic() {
        let area = Rect::new(0, 0, 80, 24);
        let r = centered_rect(40, 10, area);
        assert_eq!(r.x, 20);
        assert_eq!(r.y, 7);
        assert_eq!(r.width, 40);
        assert_eq!(r.height, 10);
    }

    #[test]
    fn centered_rect_clamps_to_area() {
        let area = Rect::new(5, 5, 30, 10);
        let r = centered_rect(100, 50, area);
        assert_eq!(r.width, 30); // clamped
        assert_eq!(r.height, 10); // clamped
    }

    #[test]
    fn modal_layout_has_correct_regions() {
        let area = Rect::new(0, 0, 80, 40);
        let layout = compute_modal_layout(area, 60, 20, 2, 1);
        // Header + body + footer should fit inside inner area
        assert!(layout.header_area.y >= layout.inner_area.y);
        assert!(layout.body_area.y >= layout.header_area.y + layout.header_area.height);
        assert_eq!(layout.footer_area.height, 1);
        assert_eq!(layout.header_area.height, 2);
    }

    #[test]
    fn begin_modal_darkens_and_clears() {
        let area = Rect::new(0, 0, 40, 20);
        let mut buf = Buffer::empty(area);
        let layout = begin_modal(&mut buf, area, 20, 10, 1, 1);
        // Dialog area should have PANEL_BG background.
        let center = buf
            .cell((layout.dialog_area.x + 1, layout.dialog_area.y + 1))
            .unwrap();
        assert_eq!(center.bg, PANEL_BG);
    }

    #[test]
    fn shortcut_line_formatting() {
        let line = shortcut_line("Ctrl+F", "Open search");
        assert_eq!(line.spans.len(), 2);
    }
}
