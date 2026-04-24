/// Virtual-scroll list widget with per-item height caching.
///
/// Only renders the visible slice of items — O(visible) instead of O(total).
/// Height cache is invalidated lazily on terminal-width change.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

// ── Traits ────────────────────────────────────────────────────────────────────

/// An item that can measure its own height and render itself into a buffer.
///
/// The associated `Ctx` type carries any context the item needs (theme, syntax
/// state, etc.) without requiring `VirtualList` itself to know about it.
pub trait VirtualItem {
    /// Context type passed to `height` and `render` on each call.
    type Ctx<'a>;

    /// Compute the number of terminal rows this item occupies at the given width.
    ///
    /// The result is cached by `VirtualList`; call [`VirtualList::invalidate`]
    /// to evict a stale entry (e.g. after a thinking-block toggle).
    fn height(&self, width: u16, ctx: Self::Ctx<'_>) -> u16;

    /// Render the item into `buf` clipped to `area`.
    fn render(&self, area: Rect, buf: &mut Buffer, ctx: Self::Ctx<'_>);

    /// Optional section-header text for this item (used for pinned headers).
    /// Returns `None` by default.
    fn section_header(&self) -> Option<&str> {
        None
    }
}

// ── Search stub ───────────────────────────────────────────────────────────────

/// Placeholder for future in-list search indexing (not wired in v1).
pub struct SearchIndex;

// ── VirtualList ───────────────────────────────────────────────────────────────

/// A scrollable list that renders only the items visible in the current viewport.
///
/// # Height caching
/// Each item's height is computed once per terminal width and stored in
/// `height_cache`. When the terminal is resized, the entire cache is cleared
/// and heights are recomputed lazily as items scroll into view.
///
/// # Sticky-bottom
/// When `sticky_bottom` is true, every `push()` auto-scrolls to keep the
/// newest item visible — matching the behaviour of most chat UIs. If the user
/// scrolls up manually (`scroll_by` with a negative delta), sticky-bottom is
/// cleared automatically.
pub struct VirtualList<T: VirtualItem> {
    /// The ordered list of items.
    items: Vec<T>,
    /// Per-item height cache at `cached_width`. `None` means not yet computed.
    height_cache: Vec<Option<u16>>,
    /// Terminal width for which `height_cache` is valid.
    cached_width: u16,
    /// First visible row (in character-row units, 0-based from top of list).
    viewport_top: u32,
    /// When true, push() auto-scrolls to bottom.
    sticky_bottom: bool,
    /// Reserved for future search integration.
    #[allow(dead_code)]
    search_index: Option<SearchIndex>,
}

impl<T: VirtualItem> Default for VirtualList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: VirtualItem> VirtualList<T> {
    /// Create a new empty `VirtualList` with sticky-bottom enabled.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            height_cache: Vec::new(),
            cached_width: 0,
            viewport_top: 0,
            sticky_bottom: true,
            search_index: None,
        }
    }

    /// Append a new item. If `sticky_bottom` is set, auto-scrolls to bottom.
    pub fn push(&mut self, item: T) {
        self.items.push(item);
        self.height_cache.push(None);
        if self.sticky_bottom {
            // Defer actual scroll math to render() — use sentinel max value.
            self.viewport_top = u32::MAX;
        }
    }

    /// Invalidate the cached height for item at `idx`.
    ///
    /// Call this after a toggle (e.g. expand/collapse thinking block) that
    /// changes the item's rendered height.
    pub fn invalidate(&mut self, idx: usize) {
        if let Some(entry) = self.height_cache.get_mut(idx) {
            *entry = None;
        }
    }

    /// Scroll to the absolute bottom of the list.
    pub fn scroll_to_bottom(&mut self) {
        self.viewport_top = u32::MAX;
        self.sticky_bottom = true;
    }

    /// Scroll by `rows` (positive = down, negative = up).
    ///
    /// Scrolling up clears `sticky_bottom`.
    pub fn scroll_by(&mut self, rows: i32) {
        if rows < 0 {
            self.sticky_bottom = false;
        }
        let new_top = self.viewport_top as i64 + i64::from(rows);
        // Clamp to [0, u32::MAX]; actual bottom clamping is done in render().
        self.viewport_top = new_top.clamp(0, i64::from(u32::MAX)) as u32;
    }

    /// Number of items in the list.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when the list is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Current viewport top row (resolved sentinel after first render).
    #[allow(dead_code)]
    pub fn viewport_top(&self) -> u32 {
        self.viewport_top
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Ensure the width cache is valid; clear all heights on width change.
    fn ensure_width(&mut self, width: u16) {
        if self.cached_width != width {
            for h in &mut self.height_cache {
                *h = None;
            }
            self.cached_width = width;
        }
    }

    /// Compute and cache the height for item `idx` at the current width.
    /// Requires `ctx` in order to compute the height via `VirtualItem::height`.
    fn item_height_with<'a>(&mut self, idx: usize, ctx: &T::Ctx<'a>) -> u16
    where
        T::Ctx<'a>: Copy,
    {
        if let Some(Some(h)) = self.height_cache.get(idx) {
            return *h;
        }
        let h = self.items[idx].height(self.cached_width, *ctx);
        let h = h.max(1); // every item occupies at least one row
        if let Some(entry) = self.height_cache.get_mut(idx) {
            *entry = Some(h);
        }
        h
    }

    /// Compute total height of all items (using cached values where available).
    fn total_height_with<'a>(&mut self, ctx: &T::Ctx<'a>) -> u32
    where
        T::Ctx<'a>: Copy,
    {
        let n = self.items.len();
        let mut total: u32 = 0;
        for i in 0..n {
            total = total.saturating_add(u32::from(self.item_height_with(i, ctx)));
        }
        total
    }

    /// Render all items in the viewport.
    ///
    /// Height cache is built at `area.width`. See [`Self::render_with_measure_width`]
    /// if you need to decouple measurement width from render width.
    ///
    /// # Rendering algorithm
    /// 1. Invalidate height cache on width change.
    /// 2. Resolve sentinel `viewport_top` (u32::MAX → clamp to bottom).
    /// 3. Walk items to find first visible one using cumulative height.
    /// 4. Render items until viewport is full, clipping partial items at both edges.
    pub fn render<'a>(&mut self, area: Rect, buf: &mut Buffer, ctx: T::Ctx<'a>)
    where
        T::Ctx<'a>: Copy,
    {
        self.render_with_measure_width(area, area.width, buf, ctx);
    }

    /// Render with an explicit measurement width for height caching.
    ///
    /// Useful when the container computes `line_count` at a different width than
    /// the actual render area (e.g. legacy path measures at outer_width=80 while
    /// rendering into inner_width=78). Passing `measure_width = outer_width` makes
    /// scroll math match the legacy path exactly.
    pub fn render_with_measure_width<'a>(
        &mut self,
        area: Rect,
        measure_width: u16,
        buf: &mut Buffer,
        ctx: T::Ctx<'a>,
    ) where
        T::Ctx<'a>: Copy,
    {
        if area.height == 0 || area.width == 0 || self.items.is_empty() {
            return;
        }

        // Use measure_width for height caching (may differ from area.width).
        self.ensure_width(measure_width);

        let viewport_height = u32::from(area.height);

        // Resolve sentinel max-bottom value.
        if self.viewport_top == u32::MAX {
            let total = self.total_height_with(&ctx);
            self.viewport_top = total.saturating_sub(viewport_height);
        }

        // Clamp viewport_top to valid range.
        let total = self.total_height_with(&ctx);
        let max_top = total.saturating_sub(viewport_height);
        self.viewport_top = self.viewport_top.min(max_top);

        let vp_top = self.viewport_top;
        let vp_bottom = vp_top + viewport_height;

        // Walk items using index-based loop to avoid simultaneous immutable
        // borrow of `self.items` and mutable borrow of `self.height_cache`.
        let mut row_cursor: u32 = 0; // running cumulative height
        let mut screen_row: u16 = area.y;
        let n = self.items.len();

        for idx in 0..n {
            // height_with: computes & caches, requires &mut self.
            let h = self.item_height_with(idx, &ctx);
            let item_top = row_cursor;
            let item_bottom = row_cursor.saturating_add(u32::from(h));

            // Skip items entirely above the viewport.
            if item_bottom <= vp_top {
                row_cursor = item_bottom;
                continue;
            }
            // Stop once past the viewport.
            if item_top >= vp_bottom {
                break;
            }

            // Calculate the visible slice of this item.
            let clip_top = vp_top.saturating_sub(item_top); // rows hidden at top
            let visible_h = h.saturating_sub(u16::try_from(clip_top).unwrap_or(h));
            let available_h = area.bottom().saturating_sub(screen_row);
            let render_h = visible_h.min(available_h);

            if render_h == 0 {
                row_cursor = item_bottom;
                continue;
            }

            // Render item into a scratch buffer at y=0 (full height).
            // Then blit only the visible slice into the real buffer.
            let full_item_area = Rect {
                x: 0,
                y: 0,
                width: area.width,
                height: h,
            };
            let mut sub_buf = Buffer::empty(full_item_area);
            // Re-borrow item immutably after height_cache write is done.
            self.items[idx].render(full_item_area, &mut sub_buf, ctx);

            // Blit: copy [clip_top .. clip_top+render_h] rows into real buf.
            let skip_rows = u16::try_from(clip_top).unwrap_or(0);
            for row_off in 0..render_h {
                let src_row = skip_rows + row_off;
                let dst_row = screen_row + row_off;
                if src_row >= h || dst_row >= area.bottom() {
                    break;
                }
                for col in 0..area.width {
                    buf[(area.x + col, dst_row)] = sub_buf[(col, src_row)].clone();
                }
            }

            screen_row += render_h;
            row_cursor = item_bottom;
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    /// A minimal VirtualItem for testing: fixed height, writes its index as text.
    #[derive(Clone)]
    struct FixedItem {
        label: String,
        height: u16,
    }

    impl FixedItem {
        fn new(label: &str, height: u16) -> Self {
            Self {
                label: label.to_string(),
                height,
            }
        }
    }

    /// Context for FixedItem — no data needed.
    #[derive(Copy, Clone)]
    struct NoCtx;

    impl VirtualItem for FixedItem {
        type Ctx<'a> = NoCtx;

        fn height(&self, _width: u16, _ctx: NoCtx) -> u16 {
            self.height
        }

        fn render(&self, area: Rect, buf: &mut Buffer, _ctx: NoCtx) {
            // Write the label into the first row of the area.
            let label = &self.label;
            for (i, ch) in label.chars().enumerate() {
                let col = area.x + i as u16;
                if col >= area.x + area.width {
                    break;
                }
                buf[(col, area.y)].set_char(ch);
            }
        }
    }

    fn make_list(items: &[(&str, u16)]) -> VirtualList<FixedItem> {
        let mut list = VirtualList::new();
        for (label, h) in items {
            list.push(FixedItem::new(label, *h));
        }
        list
    }

    // ── Test 1: push and height cache basic ───────────────────────────────────

    #[test]
    fn push_populates_height_cache_lazily() {
        let mut list: VirtualList<FixedItem> = VirtualList::new();
        list.push(FixedItem::new("a", 2));
        list.push(FixedItem::new("b", 3));
        assert_eq!(list.items.len(), 2);
        // Cache entries exist but are None until render triggers computation.
        assert!(list.height_cache.iter().all(|h| h.is_none()));
    }

    // ── Test 2: width change invalidates cache ────────────────────────────────

    #[test]
    fn width_change_clears_height_cache() {
        let mut list = make_list(&[("x", 1), ("y", 2)]);
        let area80 = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area80);
        list.render(area80, &mut buf, NoCtx);
        // After render at width 80, cache should be populated.
        assert!(list.height_cache.iter().all(|h| h.is_some()));

        // Render at different width → cache cleared and repopulated.
        let area100 = Rect::new(0, 0, 100, 10);
        let mut buf2 = Buffer::empty(area100);
        list.render(area100, &mut buf2, NoCtx);
        assert_eq!(list.cached_width, 100);
        assert!(list.height_cache.iter().all(|h| h.is_some()));
    }

    // ── Test 3: scroll_by clamps to 0 ────────────────────────────────────────

    #[test]
    fn scroll_by_clamps_to_zero_at_top() {
        let mut list = make_list(&[("a", 3)]);
        // Render first to resolve total height.
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        list.viewport_top = 0;
        list.sticky_bottom = false;
        list.render(area, &mut buf, NoCtx);
        // Scroll up past top.
        list.scroll_by(-100);
        // Re-render to clamp.
        list.render(area, &mut buf, NoCtx);
        assert_eq!(list.viewport_top, 0);
    }

    // ── Test 4: scroll_by clears sticky_bottom when scrolling up ─────────────

    #[test]
    fn scroll_up_clears_sticky_bottom() {
        let mut list = make_list(&[("a", 1)]);
        assert!(list.sticky_bottom);
        list.scroll_by(-1);
        assert!(!list.sticky_bottom);
    }

    // ── Test 5: sticky_bottom auto-scrolls on push ───────────────────────────

    #[test]
    fn sticky_bottom_auto_scrolls_on_push() {
        let mut list: VirtualList<FixedItem> = VirtualList::new();
        list.sticky_bottom = true;
        for i in 0..10 {
            list.push(FixedItem::new(&i.to_string(), 3));
        }
        // viewport_top should be u32::MAX (sentinel) meaning "go to bottom".
        assert_eq!(list.viewport_top, u32::MAX);
    }

    // ── Test 6: sticky_bottom stays put when user scrolled up ─────────────────

    #[test]
    fn no_auto_scroll_when_sticky_cleared() {
        let mut list: VirtualList<FixedItem> = VirtualList::new();
        list.sticky_bottom = true;
        list.push(FixedItem::new("first", 3));
        // User scrolls up → sticky off.
        list.scroll_by(-1);
        assert!(!list.sticky_bottom);
        // Resolve sentinel if any.
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        list.render(area, &mut buf, NoCtx);
        let vp_before = list.viewport_top;
        // Push new item — should NOT auto-scroll.
        list.push(FixedItem::new("second", 3));
        list.render(area, &mut buf, NoCtx);
        assert_eq!(list.viewport_top, vp_before);
    }

    // ── Test 7: invalidate clears single-item cache ───────────────────────────

    #[test]
    fn invalidate_clears_single_entry() {
        let mut list = make_list(&[("a", 2), ("b", 4), ("c", 1)]);
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        list.render(area, &mut buf, NoCtx);
        // All cached.
        assert!(list.height_cache.iter().all(|h| h.is_some()));
        // Invalidate middle item.
        list.invalidate(1);
        assert!(list.height_cache[0].is_some());
        assert!(list.height_cache[1].is_none());
        assert!(list.height_cache[2].is_some());
    }

    // ── Test 8: scroll_to_bottom sets sentinel and sticky ─────────────────────

    #[test]
    fn scroll_to_bottom_sets_sentinel_and_sticky() {
        let mut list = make_list(&[("a", 5)]);
        list.sticky_bottom = false;
        list.viewport_top = 0;
        list.scroll_to_bottom();
        assert!(list.sticky_bottom);
        assert_eq!(list.viewport_top, u32::MAX);
    }

    // ── Test 9: render does not panic for empty list ──────────────────────────

    #[test]
    fn render_empty_list_no_panic() {
        let mut list: VirtualList<FixedItem> = VirtualList::new();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        list.render(area, &mut buf, NoCtx); // must not panic
    }

    // ── Test 10: visible items appear in buffer ────────────────────────────────

    #[test]
    fn visible_items_written_to_buffer() {
        let mut list = make_list(&[("ITEM_A", 1), ("ITEM_B", 1), ("ITEM_C", 1)]);
        list.viewport_top = 0;
        list.sticky_bottom = false;
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        list.render(area, &mut buf, NoCtx);
        // Each item should appear in the buffer.
        let row0: String = (0..6).map(|c| buf[(c, 0)].symbol().to_string()).collect();
        let row1: String = (0..6).map(|c| buf[(c, 1)].symbol().to_string()).collect();
        let row2: String = (0..6).map(|c| buf[(c, 2)].symbol().to_string()).collect();
        assert_eq!(row0, "ITEM_A", "row 0 should contain ITEM_A");
        assert_eq!(row1, "ITEM_B", "row 1 should contain ITEM_B");
        assert_eq!(row2, "ITEM_C", "row 2 should contain ITEM_C");
    }
}
