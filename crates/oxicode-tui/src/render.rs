//! Render helpers — spinner animation, stall detection, color constants.
//!
//! Color palette using a warm Rust-inspired dark theme:
//!   - Deep charcoal backgrounds with warm undertones
//!   - Burnt orange accent as brand color
//!   - Warm cream text for readability
//!   - Muted rose-gray for secondary content
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

// ── Brand & accent ─────────────────────────────────────────────────

/// Brand accent color (burnt orange).
pub const CLAUDE_ORANGE: Color = Color::Rgb(200, 90, 23);

// ── UI chrome colors ───────────────────────────────────────────────

/// Status bar background — warm dark charcoal.
pub const STATUS_BAR_BG: Color = Color::Rgb(30, 26, 24);

/// Border / separator color — warm gray.
pub const BORDER_COLOR: Color = Color::Rgb(70, 60, 55);

/// Primary foreground text on chrome (status bar, input title).
pub const CHROME_TEXT: Color = Color::Rgb(213, 196, 192);

/// Muted chrome text (secondary info, hints).
pub const CHROME_MUTED: Color = Color::Rgb(130, 115, 110);

/// Focused border color (warm orange glow).
pub const FOCUS_BORDER: Color = Color::Rgb(200, 120, 60);

/// Success green for status indicators.
pub const STATUS_GREEN: Color = Color::Rgb(76, 175, 80);

/// Warning yellow — warm amber tone.
pub const STATUS_YELLOW: Color = Color::Rgb(232, 160, 64);

/// Error/danger red — bright for readability.
pub const STATUS_RED: Color = Color::Rgb(240, 90, 80);

/// Info highlight — warm cream-orange for costs, cache.
pub const STATUS_CYAN: Color = Color::Rgb(210, 160, 100);

// ── Transcript colors ──────────────────────────────────────────────

/// Warm background for user message blocks — noticeable contrast.
pub const TRANSCRIPT_USER_BG: Color = Color::Rgb(50, 42, 38);

/// Primary text color — warm off-white / cream.
pub const TRANSCRIPT_TEXT: Color = Color::Rgb(240, 232, 224);

/// Secondary text for tool output, code lines — readable but dimmer.
pub const TRANSCRIPT_SECONDARY: Color = Color::Rgb(190, 178, 170);

/// Muted text for metadata, timestamps, pipe lines — subtle.
pub const TRANSCRIPT_MUTED: Color = Color::Rgb(150, 132, 125);

/// Separator line color — warm subtle.
pub const SEPARATOR_COLOR: Color = Color::Rgb(55, 48, 44);

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
            return STATUS_RED;
        }
        if elapsed > std::time::Duration::from_secs(1) {
            return STATUS_YELLOW;
        }
    }
    STATUS_GREEN
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
