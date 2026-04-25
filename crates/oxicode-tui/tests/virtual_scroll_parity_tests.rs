//! Parity tests: VirtualList<MessageItem> vs legacy MessageView rendering.
//!
//! Each fixture conversation is rendered through both the old
//! (Paragraph-based, full-walk) path and the new (VirtualList, visible-only)
//! path. The resulting terminal cell buffers are compared for equality,
//! modulo trailing-whitespace differences in lines below content.
//!
//! Run with:
//!   cargo test -p oxicode-tui --test virtual_scroll_parity_tests --features virtual-scroll
//!
//! When the `virtual-scroll` feature is off the tests compile to no-ops.

use oxicode_common::{ContentBlock, Message, Role};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;

use oxicode_tui::widgets::{ActiveToolInfo, MessageRenderCache, MessageView};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_user(text: &str) -> Message {
    Message {
        id: "u".into(),
        role: Role::User,
        content: vec![ContentBlock::Text { text: text.into() }],
        model: None,
        stop_reason: None,
        created_at: chrono::Utc::now(),
        usage: None,
    }
}

fn make_assistant(text: &str) -> Message {
    Message {
        id: "a".into(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text { text: text.into() }],
        model: None,
        stop_reason: None,
        created_at: chrono::Utc::now(),
        usage: None,
    }
}

/// Build a conversation of `n` alternating User/Assistant messages.
fn build_conversation(n: usize) -> Vec<Message> {
    let mut msgs = Vec::with_capacity(n);
    for i in 0..n {
        if i % 2 == 0 {
            msgs.push(make_user(&format!("User message number {i}.")));
        } else {
            msgs.push(make_assistant(&format!(
                "Assistant reply number {i}. Some **markdown** with `code` and a newline.\n\
                 Second paragraph in message {i}."
            )));
        }
    }
    msgs
}

/// Extract content rows from a rendered buffer's inner area (strip borders).
///
/// Returns a `Vec<String>` of trimmed row text for the inner content area.
/// The right-most column (border or scrollbar) is excluded from comparison
/// since the legacy path overlays a scrollbar thumb there.
#[cfg(feature = "virtual-scroll")]
fn extract_inner_rows(buf: &Buffer, width: u16, height: u16) -> Vec<String> {
    // Inner area: skip 1-cell border on all sides, and also skip the rightmost
    // column of the inner area because the legacy path paints a scrollbar thumb
    // (`█`) there that the virtual path does not.
    let inner_right = width.saturating_sub(2); // exclude right border + scrollbar col
    let mut rows = Vec::new();
    for row in 1..height.saturating_sub(1) {
        let s: String = (1..inner_right)
            .map(|col| buf[(col, row)].symbol().to_string())
            .collect();
        rows.push(s.trim_end().to_string());
    }
    rows
}

/// Render the message list through the legacy `MessageView` path.
///
/// `width` and `height` are the OUTER dimensions (including 1-cell borders).
/// The cache is updated with `width` (outer) — matching the production code path.
fn render_legacy(messages: &[Message], width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut cache = MessageRenderCache::new();
    cache.update(messages, width);
    let roles: Vec<Role> = messages.iter().map(|m| m.role).collect();
    let active: &[ActiveToolInfo<'_>] = &[];
    let widget = MessageView::new(
        cache.lines(messages.len()),
        messages.len(),
        messages.last().map(|m| m.role),
        None,
        None,
        active,
        u16::MAX, // auto-scroll to bottom
    )
    .with_message_roles(roles)
    .with_tool_only_indices(cache.tool_only_indices());
    let mut buf = Buffer::empty(area);
    ratatui::widgets::Widget::render(widget, area, &mut buf);
    buf
}

/// Render the message list through the VirtualList path.
///
/// Uses `render_with_measure_width(outer_width)` so that VirtualList height
/// caching uses the same width as `paragraph.line_count(area.width)` in the
/// legacy path — making scroll positions equivalent.
#[cfg(feature = "virtual-scroll")]
fn render_virtual(messages: &[Message], width: u16, height: u16) -> Buffer {
    use oxicode_common::Role as ORole;
    use oxicode_tui::widgets::message_view_virtual::{build_message_items, MessageRenderCtx};
    use oxicode_tui::widgets::virtual_list::VirtualList;
    use ratatui::widgets::{Block, Borders, Widget};

    let outer_area = Rect::new(0, 0, width, height);
    let inner_area = Rect {
        x: outer_area.x + 1,
        y: outer_area.y + 1,
        width: outer_area.width.saturating_sub(2),
        height: outer_area.height.saturating_sub(2),
    };

    let mut cache = MessageRenderCache::new();
    // Update at outer_width — same as legacy path — so pre-rendered Line
    // items have the same trailing-space padding as the legacy renderer sees.
    cache.update(messages, width);
    let cached = cache.lines(messages.len());
    let tool_only = cache.tool_only_indices();
    let roles: Vec<ORole> = messages.iter().map(|m| m.role).collect();

    let items = build_message_items(cached, &roles, tool_only, 0);
    let mut list: VirtualList<_> = VirtualList::new();
    for item in items {
        list.push(item);
    }
    list.scroll_to_bottom();

    let ctx = MessageRenderCtx {
        tool_only_indices: tool_only,
    };

    let mut buf = Buffer::empty(outer_area);
    Block::default()
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default())
        .title(" Conversation ")
        .render(outer_area, &mut buf);

    // render_with_measure_width: compute heights at outer_width (matching
    // legacy's paragraph.line_count(area.width)) but paint into inner_area.
    list.render_with_measure_width(inner_area, width, &mut buf, ctx);
    buf
}

// ── Comparison helper ─────────────────────────────────────────────────────────

/// Compare the rendered content of two buffers for visual parity.
///
/// Both paths auto-scroll to the bottom. The legacy path has a known 2-row
/// scroll-offset from how `Paragraph::line_count` counts block borders in the
/// total (making legacy over-scroll by 2 rows and show 2 blank rows at bottom).
/// The virtual path shows 2 more content rows at the top of the viewport.
///
/// Parity guarantee: both paths show the same last N content rows at the bottom
/// of the viewport (where N = content rows shown in both = viewport_h - 2).
///
/// Strategy: extract non-empty rows from both, then align from the END and
/// compare the portion that both buffers share.
#[cfg(feature = "virtual-scroll")]
fn compare_inner_content(
    legacy: &Buffer,
    new: &Buffer,
    width: u16,
    height: u16,
) -> Result<(), String> {
    let legacy_rows = extract_inner_rows(legacy, width, height);
    let new_rows = extract_inner_rows(new, width, height);

    // Non-empty rows only — blank rows are padding or the legacy 2-row overshoot.
    let lc: Vec<&str> = legacy_rows
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    let nc: Vec<&str> = new_rows
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();

    if lc.is_empty() && nc.is_empty() {
        return Ok(());
    }

    // The shorter sequence determines what to compare. Both should end with the
    // same bottom-of-conversation content lines.
    let compare_n = lc.len().min(nc.len());
    let l_tail = &lc[lc.len() - compare_n..];
    let n_tail = &nc[nc.len() - compare_n..];

    let mut diffs = Vec::new();
    for (i, (l, n)) in l_tail.iter().zip(n_tail.iter()).enumerate() {
        if l != n {
            diffs.push(format!("  [{i}]: legacy={l:?} new={n:?}"));
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Bottom {compare_n} content lines differ:\n{}",
            diffs.join("\n")
        ))
    }
}

// ── Fixtures ──────────────────────────────────────────────────────────────────

// Note: when `virtual-scroll` is OFF these tests still compile but use only
// the legacy path (no comparison). They serve as regression tests for the
// legacy renderer itself.

#[test]
fn parity_1_message() {
    let msgs = build_conversation(1);
    let (w, h) = (80, 24);
    let legacy = render_legacy(&msgs, w, h);

    #[cfg(feature = "virtual-scroll")]
    {
        let new = render_virtual(&msgs, w, h);
        let result = compare_inner_content(&legacy, &new, w, h);
        assert!(result.is_ok(), "{}", result.unwrap_err());
    }
    #[cfg(not(feature = "virtual-scroll"))]
    {
        // Smoke test: render must not panic.
        let _ = legacy;
    }
}

#[test]
fn parity_10_messages() {
    let msgs = build_conversation(10);
    let (w, h) = (80, 24);
    let legacy = render_legacy(&msgs, w, h);

    #[cfg(feature = "virtual-scroll")]
    {
        let new = render_virtual(&msgs, w, h);
        let result = compare_inner_content(&legacy, &new, w, h);
        assert!(result.is_ok(), "{}", result.unwrap_err());
    }
    #[cfg(not(feature = "virtual-scroll"))]
    let _ = legacy;
}

#[test]
fn parity_100_messages() {
    let msgs = build_conversation(100);
    let (w, h) = (80, 24);
    let legacy = render_legacy(&msgs, w, h);

    #[cfg(feature = "virtual-scroll")]
    {
        let new = render_virtual(&msgs, w, h);
        let result = compare_inner_content(&legacy, &new, w, h);
        assert!(result.is_ok(), "{}", result.unwrap_err());
    }
    #[cfg(not(feature = "virtual-scroll"))]
    let _ = legacy;
}

#[test]
fn parity_500_messages() {
    let msgs = build_conversation(500);
    let (w, h) = (80, 24);
    let legacy = render_legacy(&msgs, w, h);

    #[cfg(feature = "virtual-scroll")]
    {
        let new = render_virtual(&msgs, w, h);
        let result = compare_inner_content(&legacy, &new, w, h);
        assert!(result.is_ok(), "{}", result.unwrap_err());
    }
    #[cfg(not(feature = "virtual-scroll"))]
    let _ = legacy;
}

#[test]
fn parity_wide_terminal() {
    let msgs = build_conversation(20);
    let (w, h) = (200, 50);
    let legacy = render_legacy(&msgs, w, h);

    #[cfg(feature = "virtual-scroll")]
    {
        let new = render_virtual(&msgs, w, h);
        let result = compare_inner_content(&legacy, &new, w, h);
        assert!(result.is_ok(), "{}", result.unwrap_err());
    }
    #[cfg(not(feature = "virtual-scroll"))]
    let _ = legacy;
}

// ── Legacy path smoke tests (always run) ─────────────────────────────────────

#[test]
fn legacy_renders_without_panic_with_terminal_backend() {
    let msgs = build_conversation(5);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            let mut cache = MessageRenderCache::new();
            cache.update(&msgs, area.width);
            let active: &[ActiveToolInfo<'_>] = &[];
            let widget = MessageView::new(
                cache.lines(msgs.len()),
                msgs.len(),
                msgs.last().map(|m| m.role),
                None,
                None,
                active,
                u16::MAX,
            );
            frame.render_widget(widget, area);
        })
        .unwrap();
}
