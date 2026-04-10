//! Render helpers — spinner animation, stall detection, color constants.
//!
//! Used by widgets and the main App draw loop.

use ratatui::style::Color;

// ── Spinner ─────────────────────────────────────────────────────────

/// 12-frame Unicode spinner with forward + reverse mirrored pulse effect.
#[cfg(not(target_os = "windows"))]
pub const SPINNER_FRAMES: &[char] = &[
    '·', '✢', '✳', '✶', '✻', '✽', '✽', '✻', '✶', '✳', '✢', '·',
];
#[cfg(target_os = "windows")]
pub const SPINNER_FRAMES: &[char] = &[
    '·', '✢', '*', '✶', '✻', '✽', '✽', '✻', '✶', '*', '✢', '·',
];

/// Claude brand color.
pub const CLAUDE_ORANGE: Color = Color::Rgb(233, 30, 99);

// ── Transcript colors ──────────────────────────────────────────────

/// Dark background for user message blocks.
pub const TRANSCRIPT_USER_BG: Color = Color::Rgb(23, 23, 31);

/// Primary text color for transcript content.
pub const TRANSCRIPT_TEXT: Color = Color::Rgb(236, 236, 241);

/// Muted text color for metadata, timestamps, and secondary info.
pub const TRANSCRIPT_MUTED: Color = Color::Rgb(139, 139, 153);

/// Stall detection threshold — spinner turns red after this duration.
pub const STALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(3);

/// Get the current spinner character based on frame count.
pub fn spinner_char(frame_count: u64) -> char {
    SPINNER_FRAMES[(frame_count as usize) % SPINNER_FRAMES.len()]
}

/// Spinner color: green (active) → yellow (1–3s pause) → red (>3s stall).
pub fn spinner_color(stall_start: Option<std::time::Instant>) -> Color {
    if let Some(start) = stall_start {
        let elapsed = start.elapsed();
        if elapsed > STALL_THRESHOLD {
            return Color::Red;
        }
        if elapsed > std::time::Duration::from_secs(1) {
            return Color::Yellow;
        }
    }
    Color::Green // Active streaming
}

/// Check if any modal overlay is blocking normal input routing.
///
/// Used by input handlers to decide whether to process keys or defer
/// to the active modal.
#[allow(clippy::fn_params_excessive_bools)]
pub fn is_modal_open(
    has_pending_permission: bool,
    has_pending_paste: bool,
    search_active: bool,
    shortcuts_visible: bool,
) -> bool {
    has_pending_permission || has_pending_paste || search_active || shortcuts_visible
}
