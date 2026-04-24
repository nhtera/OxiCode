/// Virtual-scroll rendering path for the message list.
///
/// Implements [`VirtualItem`] for pre-rendered message line bundles so that
/// [`VirtualList<MessageItem>`] can drive the conversation panel without
/// walking every message on every frame.
///
/// # Design
/// The existing [`MessageRenderCache`] already owns `Vec<Vec<Line<'static>>>` —
/// one `Vec<Line>` per message. Rather than re-implementing rendering, we wrap
/// each such slice in a `MessageItem` and delegate height/render to ratatui's
/// `Paragraph` — exactly as `MessageView` does today. This guarantees that the
/// height the virtual list uses for scroll math is identical to what gets
/// painted on screen (zero divergence).
///
/// Enabled only when the `virtual-scroll` Cargo feature is active.
#[cfg(feature = "virtual-scroll")]
use std::collections::HashSet;

#[cfg(feature = "virtual-scroll")]
use ratatui::buffer::Buffer;
#[cfg(feature = "virtual-scroll")]
use ratatui::layout::Rect;
#[cfg(feature = "virtual-scroll")]
use ratatui::text::{Line, Text};
#[cfg(feature = "virtual-scroll")]
use ratatui::widgets::{Paragraph, Widget, Wrap};

#[cfg(feature = "virtual-scroll")]
use super::virtual_list::VirtualItem;

// ── Context ───────────────────────────────────────────────────────────────────

/// Context required to render a single message entry in the virtual list.
///
/// Passed as `VirtualItem::Ctx<'_>` on every `height()` / `render()` call.
#[cfg(feature = "virtual-scroll")]
#[derive(Copy, Clone)]
pub struct MessageRenderCtx<'a> {
    /// Set of message indices that are tool-only (no "You" separator drawn).
    pub tool_only_indices: &'a HashSet<usize>,
}

// ── Item ──────────────────────────────────────────────────────────────────────

/// One message worth of pre-rendered lines, ready for virtual-list rendering.
///
/// Height is derived directly from `Paragraph::line_count` so scroll math
/// matches the actual painted output — same approach as the legacy path.
#[cfg(feature = "virtual-scroll")]
pub struct MessageItem {
    /// Pre-rendered ratatui lines for this message (owned, 'static).
    pub lines: Vec<Line<'static>>,
    /// Whether a turn separator should be drawn before this item.
    pub has_separator: bool,
    /// Whether this is a turn-boundary (thicker separator).
    pub is_turn_boundary: bool,
}

#[cfg(feature = "virtual-scroll")]
impl VirtualItem for MessageItem {
    type Ctx<'a> = MessageRenderCtx<'a>;

    /// Compute the height this item occupies at `width` terminal columns.
    ///
    /// Delegates to `Paragraph::line_count` — the same logic used by the
    /// legacy path — so there is no divergence between height estimation and
    /// actual render.
    fn height(&self, width: u16, _ctx: MessageRenderCtx<'_>) -> u16 {
        let sep_lines = self.separator_line_count();
        let text = Text::from(self.lines.clone());
        let para = Paragraph::new(text).wrap(Wrap { trim: false });
        let content_lines = para.line_count(width);
        let total = sep_lines + content_lines;
        u16::try_from(total).unwrap_or(u16::MAX)
    }

    /// Render this item into `buf` at `area`.
    ///
    /// Writes separator lines first (if any), then the message content via
    /// `Paragraph` with word-wrap — matching the legacy `format_messages` path.
    fn render(&self, area: Rect, buf: &mut Buffer, _ctx: MessageRenderCtx<'_>) {
        if area.height == 0 {
            return;
        }
        let mut y = area.y;

        // Draw separator lines above message content.
        if self.has_separator {
            if self.is_turn_boundary {
                // Turn separator: blank + "─"×60 + blank
                if y < area.bottom() {
                    // blank line
                    y += 1;
                }
                if y < area.bottom() {
                    let sep_line = Line::from(ratatui::text::Span::styled(
                        "─".repeat(60),
                        ratatui::style::Style::default()
                            .fg(ratatui::style::Color::DarkGray),
                    ));
                    let sep_area = Rect { y, height: 1, ..area };
                    Paragraph::new(sep_line).render(sep_area, buf);
                    y += 1;
                }
                if y < area.bottom() {
                    // blank line
                    y += 1;
                }
            } else {
                // Compact separator: single blank line.
                y += 1;
            }
        }

        // Render message content.
        let remaining_h = area.bottom().saturating_sub(y);
        if remaining_h == 0 {
            return;
        }
        let content_area = Rect {
            y,
            height: remaining_h,
            ..area
        };
        let text = Text::from(self.lines.clone());
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .render(content_area, buf);
    }
}

#[cfg(feature = "virtual-scroll")]
impl MessageItem {
    /// Number of lines consumed by the separator (0 or 1 or 3).
    fn separator_line_count(&self) -> usize {
        if !self.has_separator {
            0
        } else if self.is_turn_boundary {
            3 // blank + "─"×60 + blank
        } else {
            1 // compact: one blank line
        }
    }
}

// ── Builder helper ────────────────────────────────────────────────────────────

/// Build a `Vec<MessageItem>` from an already-populated `MessageRenderCache`.
///
/// `start_index` is the global index of `cached_lines[0]` — required when
/// building incrementally (e.g. after `virtual_list.len()` items already
/// exist): `message_roles` and `tool_only_indices` use global indices, so the
/// local `enumerate()` counter must be offset by `start_index` before looking
/// them up. Pass `0` when building from the full cache.
#[cfg(feature = "virtual-scroll")]
pub fn build_message_items(
    cached_lines: &[Vec<Line<'static>>],
    message_roles: &[oxicode_common::Role],
    tool_only_indices: &HashSet<usize>,
    start_index: usize,
) -> Vec<MessageItem> {
    cached_lines
        .iter()
        .enumerate()
        .map(|(local_i, lines)| {
            let i = start_index + local_i;
            let (has_separator, is_turn_boundary) = if i == 0 {
                (false, false)
            } else if tool_only_indices.contains(&i) {
                (false, false)
            } else {
                let is_turn = message_roles
                    .get(i)
                    .is_some_and(|r| *r == oxicode_common::Role::User);
                (true, is_turn)
            };
            MessageItem {
                lines: lines.clone(),
                has_separator,
                is_turn_boundary,
            }
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "virtual-scroll"))]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use std::collections::HashSet;

    fn ctx() -> MessageRenderCtx<'static> {
        static EMPTY: std::sync::OnceLock<HashSet<usize>> = std::sync::OnceLock::new();
        MessageRenderCtx {
            tool_only_indices: EMPTY.get_or_init(HashSet::new),
        }
    }

    #[test]
    fn height_matches_paragraph_line_count() {
        let lines = vec![Line::from("Hello world"), Line::from("Second line")];
        let item = MessageItem {
            lines: lines.clone(),
            has_separator: false,
            is_turn_boundary: false,
        };
        let w = 80;
        let h = item.height(w, ctx());
        // Two lines, no separator → should be 2.
        assert_eq!(h, 2);
    }

    #[test]
    fn separator_adds_one_line() {
        let lines = vec![Line::from("content")];
        let item = MessageItem {
            lines,
            has_separator: true,
            is_turn_boundary: false,
        };
        let h = item.height(80, ctx());
        assert_eq!(h, 2); // 1 sep + 1 content
    }

    #[test]
    fn turn_boundary_adds_three_lines() {
        let lines = vec![Line::from("content")];
        let item = MessageItem {
            lines,
            has_separator: true,
            is_turn_boundary: true,
        };
        let h = item.height(80, ctx());
        assert_eq!(h, 4); // 3 sep + 1 content
    }

    #[test]
    fn render_does_not_panic_in_small_area() {
        let item = MessageItem {
            lines: vec![Line::from("hello")],
            has_separator: true,
            is_turn_boundary: true,
        };
        let area = Rect::new(0, 0, 40, 2);
        let mut buf = Buffer::empty(area);
        item.render(area, &mut buf, ctx()); // must not panic
    }

    #[test]
    fn build_message_items_first_has_no_separator() {
        use oxicode_common::Role;
        let lines = vec![
            vec![Line::from("msg0")],
            vec![Line::from("msg1")],
        ];
        let roles = vec![Role::User, Role::Assistant];
        let tool_only = HashSet::new();
        let items = build_message_items(&lines, &roles, &tool_only, 0);
        assert!(!items[0].has_separator);
        assert!(items[1].has_separator);
    }
}
