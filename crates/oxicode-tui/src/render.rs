//! Render helpers — spinner animation, stall detection, color constants.
//!
//! Ported from claurst's render.rs pattern. These pure functions are used by
//! widgets and the main App draw loop. The full render-frame extraction will
//! follow in a later phase once all widgets are stabilized.

use ratatui::style::Color;

// ── Spinner ─────────────────────────────────────────────────────────

/// 12-frame Unicode spinner matching claurst's `SpinnerGlyph` pattern.
/// Forward + reverse mirrored for smooth pulse effect.
#[cfg(not(target_os = "windows"))]
pub const SPINNER_FRAMES: &[char] = &[
    '·', '✢', '✳', '✶', '✻', '✽', '✽', '✻', '✶', '✳', '✢', '·',
];
#[cfg(target_os = "windows")]
pub const SPINNER_FRAMES: &[char] = &[
    '·', '✢', '*', '✶', '✻', '✽', '✽', '✻', '✶', '*', '✢', '·',
];

/// Claude brand color (matches claurst `CLAUDE_ORANGE`).
pub const CLAUDE_ORANGE: Color = Color::Rgb(233, 30, 99);

/// Stall detection threshold — spinner turns red after this duration.
pub const STALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(3);

/// Get the current spinner character based on frame count.
pub fn spinner_char(frame_count: u64) -> char {
    SPINNER_FRAMES[(frame_count as usize) % SPINNER_FRAMES.len()]
}

/// Spinner color: yellow normally, red when stalled (>3s without stream data).
pub fn spinner_color(stall_start: Option<std::time::Instant>) -> Color {
    if let Some(start) = stall_start {
        if start.elapsed() > STALL_THRESHOLD {
            return Color::Red;
        }
    }
    Color::Yellow
}

/// Check if any modal overlay is blocking normal input routing.
///
/// Used by input handlers to decide whether to process keys or defer
/// to the active modal.
pub fn is_modal_open(
    has_pending_permission: bool,
    has_pending_paste: bool,
    search_active: bool,
    shortcuts_visible: bool,
) -> bool {
    has_pending_permission || has_pending_paste || search_active || shortcuts_visible
}
