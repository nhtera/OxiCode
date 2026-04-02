//! TUI color themes: define palettes for dark, light, catppuccin, dracula, solarized.
//!
//! Each theme provides a `ThemePalette` with named styles for all UI components.
//! Switching themes at runtime is instant (no re-render artifacts).

use ratatui::style::{Color, Modifier, Style};

/// Named color palette for all TUI components.
#[derive(Debug, Clone)]
pub struct ThemePalette {
    pub name: &'static str,
    /// Background color for the main area.
    pub bg: Color,
    /// Default foreground text.
    pub fg: Color,
    /// Primary accent (borders, highlights).
    pub accent: Color,
    /// Secondary accent.
    pub accent_dim: Color,
    /// Status bar background.
    pub status_bg: Color,
    /// Status bar foreground.
    pub status_fg: Color,
    /// User message text.
    pub user_msg: Color,
    /// Assistant message text.
    pub assistant_msg: Color,
    /// Tool call output.
    pub tool_output: Color,
    /// Error text.
    pub error: Color,
    /// Success/ok text.
    pub success: Color,
    /// Warning text.
    pub warning: Color,
    /// Code block background.
    pub code_bg: Color,
    /// Muted/dim text.
    pub muted: Color,
}

impl ThemePalette {
    /// Style for primary text.
    pub fn text_style(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    /// Style for borders.
    pub fn border_style(&self) -> Style {
        Style::default().fg(self.accent_dim)
    }

    /// Style for focused borders.
    pub fn border_focused(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Style for status bar.
    pub fn status_style(&self) -> Style {
        Style::default().fg(self.status_fg).bg(self.status_bg)
    }

    /// Style for errors.
    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error).add_modifier(Modifier::BOLD)
    }

    /// Style for success messages.
    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }
}

/// Available theme names.
pub const THEME_NAMES: &[&str] = &["dark", "light", "catppuccin", "dracula", "solarized"];

/// Get a theme palette by name.
pub fn get_theme(name: &str) -> ThemePalette {
    match name {
        "light" => light(),
        "catppuccin" => catppuccin(),
        "dracula" => dracula(),
        "solarized" => solarized(),
        _ => dark(), // default
    }
}

/// Dark theme (default).
pub fn dark() -> ThemePalette {
    ThemePalette {
        name: "dark",
        bg: Color::Reset,
        fg: Color::White,
        accent: Color::Cyan,
        accent_dim: Color::DarkGray,
        status_bg: Color::DarkGray,
        status_fg: Color::White,
        user_msg: Color::Green,
        assistant_msg: Color::White,
        tool_output: Color::Yellow,
        error: Color::Red,
        success: Color::Green,
        warning: Color::Yellow,
        code_bg: Color::Rgb(30, 30, 40),
        muted: Color::DarkGray,
    }
}

/// Light theme.
pub fn light() -> ThemePalette {
    ThemePalette {
        name: "light",
        bg: Color::White,
        fg: Color::Black,
        accent: Color::Blue,
        accent_dim: Color::Gray,
        status_bg: Color::Rgb(230, 230, 230),
        status_fg: Color::Black,
        user_msg: Color::Rgb(0, 100, 0),
        assistant_msg: Color::Black,
        tool_output: Color::Rgb(130, 80, 0),
        error: Color::Red,
        success: Color::Green,
        warning: Color::Rgb(180, 120, 0),
        code_bg: Color::Rgb(245, 245, 245),
        muted: Color::Gray,
    }
}

/// Catppuccin Mocha theme.
pub fn catppuccin() -> ThemePalette {
    ThemePalette {
        name: "catppuccin",
        bg: Color::Rgb(30, 30, 46),
        fg: Color::Rgb(205, 214, 244),
        accent: Color::Rgb(137, 180, 250),   // blue
        accent_dim: Color::Rgb(88, 91, 112), // surface2
        status_bg: Color::Rgb(49, 50, 68),   // surface0
        status_fg: Color::Rgb(205, 214, 244),
        user_msg: Color::Rgb(166, 227, 161), // green
        assistant_msg: Color::Rgb(205, 214, 244),
        tool_output: Color::Rgb(249, 226, 175), // yellow
        error: Color::Rgb(243, 139, 168),       // red
        success: Color::Rgb(166, 227, 161),     // green
        warning: Color::Rgb(250, 179, 135),     // peach
        code_bg: Color::Rgb(24, 24, 37),        // crust
        muted: Color::Rgb(108, 112, 134),       // overlay0
    }
}

/// Dracula theme.
pub fn dracula() -> ThemePalette {
    ThemePalette {
        name: "dracula",
        bg: Color::Rgb(40, 42, 54),
        fg: Color::Rgb(248, 248, 242),
        accent: Color::Rgb(189, 147, 249),  // purple
        accent_dim: Color::Rgb(68, 71, 90), // selection
        status_bg: Color::Rgb(68, 71, 90),
        status_fg: Color::Rgb(248, 248, 242),
        user_msg: Color::Rgb(80, 250, 123), // green
        assistant_msg: Color::Rgb(248, 248, 242),
        tool_output: Color::Rgb(241, 250, 140), // yellow
        error: Color::Rgb(255, 85, 85),         // red
        success: Color::Rgb(80, 250, 123),
        warning: Color::Rgb(255, 184, 108), // orange
        code_bg: Color::Rgb(33, 34, 44),
        muted: Color::Rgb(98, 114, 164), // comment
    }
}

/// Solarized Dark theme.
pub fn solarized() -> ThemePalette {
    ThemePalette {
        name: "solarized",
        bg: Color::Rgb(0, 43, 54),
        fg: Color::Rgb(131, 148, 150),
        accent: Color::Rgb(38, 139, 210),  // blue
        accent_dim: Color::Rgb(7, 54, 66), // base02
        status_bg: Color::Rgb(7, 54, 66),
        status_fg: Color::Rgb(147, 161, 161),
        user_msg: Color::Rgb(133, 153, 0), // green
        assistant_msg: Color::Rgb(131, 148, 150),
        tool_output: Color::Rgb(181, 137, 0), // yellow
        error: Color::Rgb(220, 50, 47),       // red
        success: Color::Rgb(133, 153, 0),
        warning: Color::Rgb(203, 75, 22), // orange
        code_bg: Color::Rgb(0, 36, 45),
        muted: Color::Rgb(88, 110, 117), // base01
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_themes_load() {
        for name in THEME_NAMES {
            let theme = get_theme(name);
            assert_eq!(theme.name, *name);
        }
    }

    #[test]
    fn test_default_is_dark() {
        let theme = get_theme("unknown");
        assert_eq!(theme.name, "dark");
    }

    #[test]
    fn test_theme_styles() {
        let theme = dark();
        let _ = theme.text_style();
        let _ = theme.border_style();
        let _ = theme.error_style();
        let _ = theme.success_style();
        let _ = theme.status_style();
    }
}
