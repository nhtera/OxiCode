//! Permissions settings tab: mode radio + allow/deny text areas.
//!
//! Four radio buttons for PermissionMode, then two comma-separated editable
//! text areas for always-allow and always-deny tool lists.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::widgets::modal_helpers::{DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT};

/// The four permission modes shown in the radio group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayPermMode {
    Default,
    AcceptEdits,
    Bypass,
    Plan,
}

impl DisplayPermMode {
    pub const ALL: [Self; 4] = [Self::Default, Self::AcceptEdits, Self::Bypass, Self::Plan];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "default       – ask for dangerous operations",
            Self::AcceptEdits => "accept-edits  – auto-approve file edits, ask for shell",
            Self::Bypass => "bypass        – trust everything (no prompts)",
            Self::Plan => "plan          – read-only planning mode",
        }
    }

    /// Serialize to the string stored in settings.toml.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "accept_edits",
            Self::Bypass => "bypass",
            Self::Plan => "plan",
        }
    }

    pub fn from_setting(s: &str) -> Self {
        match s {
            "accept_edits" | "accept-edits" => Self::AcceptEdits,
            "bypass" => Self::Bypass,
            "plan" => Self::Plan,
            _ => Self::Default,
        }
    }
}

/// Which UI element has focus in the Permissions tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermFocus {
    /// Radio button group (index 0–3).
    Radio(usize),
    /// "Always allow" text area.
    AllowList,
    /// "Always deny" text area.
    DenyList,
}

/// Permissions tab state.
#[derive(Debug, Clone)]
pub struct PermissionsTab {
    pub mode: DisplayPermMode,
    /// Comma-separated tool names that are always allowed.
    pub allow_list: String,
    /// Comma-separated tool names that are always denied.
    pub deny_list: String,
    /// Current focus element.
    pub focus: PermFocus,
}

impl PermissionsTab {
    pub fn new(mode: DisplayPermMode, allow_list: String, deny_list: String) -> Self {
        Self {
            mode,
            allow_list,
            deny_list,
            focus: PermFocus::Radio(0),
        }
    }

    pub fn select_next(&mut self) {
        self.focus = match self.focus {
            PermFocus::Radio(i) => {
                if i + 1 < DisplayPermMode::ALL.len() {
                    PermFocus::Radio(i + 1)
                } else {
                    PermFocus::AllowList
                }
            }
            PermFocus::AllowList => PermFocus::DenyList,
            PermFocus::DenyList => PermFocus::Radio(0),
        };
    }

    pub fn select_prev(&mut self) {
        self.focus = match self.focus {
            PermFocus::Radio(0) => PermFocus::DenyList,
            PermFocus::Radio(i) => PermFocus::Radio(i - 1),
            PermFocus::AllowList => PermFocus::Radio(DisplayPermMode::ALL.len() - 1),
            PermFocus::DenyList => PermFocus::AllowList,
        };
    }

    /// Activate the focused element. Returns true when state changed.
    pub fn activate(&mut self) -> bool {
        if let PermFocus::Radio(i) = self.focus {
            let new_mode = DisplayPermMode::ALL[i];
            if self.mode != new_mode {
                self.mode = new_mode;
                return true;
            }
        }
        false
    }

    pub fn push_char(&mut self, c: char) {
        match self.focus {
            PermFocus::AllowList => self.allow_list.push(c),
            PermFocus::DenyList => self.deny_list.push(c),
            PermFocus::Radio(_) => {}
        }
    }

    pub fn pop_char(&mut self) {
        match self.focus {
            PermFocus::AllowList => {
                self.allow_list.pop();
            }
            PermFocus::DenyList => {
                self.deny_list.pop();
            }
            PermFocus::Radio(_) => {}
        }
    }

    /// Render the tab body into `area`.
    #[allow(clippy::too_many_lines)]
    pub fn render(&self, buf: &mut Buffer, area: Rect) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let mut y = area.y;

        // Section: Permission Mode
        if y < area.y + area.height {
            let hdr = Line::from(Span::styled(
                " Permission Mode",
                Style::default()
                    .fg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD),
            ));
            buf.set_line(area.x, y, &hdr, area.width);
            y += 1;
        }

        // Radio buttons.
        for (i, mode) in DisplayPermMode::ALL.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let is_focused = matches!(self.focus, PermFocus::Radio(fi) if fi == i);
            let is_selected = self.mode == *mode;
            let radio = if is_selected { "◉ " } else { "○ " };
            let cursor = if is_focused { "▶" } else { " " };
            let style = if is_focused {
                Style::default()
                    .fg(DIALOG_TEXT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_TEXT)
            };
            let radio_style = if is_selected {
                Style::default()
                    .fg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_MUTED)
            };
            let line = Line::from(vec![
                Span::styled(cursor, Style::default().fg(DIALOG_ACCENT)),
                Span::styled(" ", Style::default()),
                Span::styled(radio, radio_style),
                Span::styled(mode.label(), style),
            ]);
            buf.set_line(area.x, y, &line, area.width);
            y += 1;
        }

        // Hint line for radio.
        if y < area.y + area.height {
            let hint = Line::from(Span::styled(
                "  [Enter] select mode   CLI flag equiv: --permission-mode <mode>",
                Style::default().fg(DIALOG_MUTED),
            ));
            buf.set_line(area.x, y, &hint, area.width);
            y += 2; // blank line gap
        }

        // Section: Always Allow.
        if y < area.y + area.height {
            let allow_focused = self.focus == PermFocus::AllowList;
            let hdr_style = if allow_focused {
                Style::default()
                    .fg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_ACCENT)
            };
            let cursor = if allow_focused { "▶" } else { " " };
            let hdr = Line::from(vec![
                Span::styled(cursor, Style::default().fg(DIALOG_ACCENT)),
                Span::styled(" Always allow tools (comma-separated):", hdr_style),
            ]);
            buf.set_line(area.x, y, &hdr, area.width);
            y += 1;
        }

        if y < area.y + area.height {
            let allow_focused = self.focus == PermFocus::AllowList;
            let value = if allow_focused {
                format!("  {}▊", self.allow_list)
            } else {
                format!("  {}", self.allow_list)
            };
            let val_style = if allow_focused {
                Style::default()
                    .fg(DIALOG_TEXT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_TEXT)
            };
            let line = Line::from(Span::styled(value, val_style));
            buf.set_line(area.x, y, &line, area.width);
            y += 2; // blank line gap
        }

        // Section: Always Deny.
        if y < area.y + area.height {
            let deny_focused = self.focus == PermFocus::DenyList;
            let hdr_style = if deny_focused {
                Style::default()
                    .fg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_ACCENT)
            };
            let cursor = if deny_focused { "▶" } else { " " };
            let hdr = Line::from(vec![
                Span::styled(cursor, Style::default().fg(DIALOG_ACCENT)),
                Span::styled(" Always deny tools (comma-separated):", hdr_style),
            ]);
            buf.set_line(area.x, y, &hdr, area.width);
            y += 1;
        }

        if y < area.y + area.height {
            let deny_focused = self.focus == PermFocus::DenyList;
            let value = if deny_focused {
                format!("  {}▊", self.deny_list)
            } else {
                format!("  {}", self.deny_list)
            };
            let val_style = if deny_focused {
                Style::default()
                    .fg(DIALOG_TEXT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_TEXT)
            };
            let line = Line::from(Span::styled(value, val_style));
            buf.set_line(area.x, y, &line, area.width);
        }
    }
}
