//! Configurable keybinding system for the TUI.
//!
//! Loads keybindings from `~/.oxicode/keybindings.toml`, merges with defaults,
//! and dispatches key events to named actions.

use std::collections::HashMap;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

/// A named action that can be triggered by a key binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Quit the application.
    Quit,
    /// Submit current input.
    Submit,
    /// Toggle vim mode.
    ToggleVim,
    /// Toggle right panel.
    TogglePanel,
    /// Scroll up.
    ScrollUp,
    /// Scroll down.
    ScrollDown,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Clear input line.
    ClearLine,
    /// Delete word backward.
    DeleteWordBackward,
    /// Delete to line start.
    DeleteToLineStart,
    /// Open search overlay.
    OpenSearch,
    /// Toggle shortcuts overlay.
    ToggleShortcuts,
    /// Cycle output style.
    CycleOutputStyle,
    /// Move cursor to start of input.
    CursorHome,
    /// Move cursor to end of input.
    CursorEnd,
    /// Move cursor left by word.
    CursorWordLeft,
    /// Move cursor right by word.
    CursorWordRight,
    /// Insert newline (multi-line input).
    InsertNewline,
    /// History: previous entry.
    HistoryPrev,
    /// History: next entry.
    HistoryNext,
    /// History search (Ctrl+R).
    HistorySearch,
}

/// A key combination (modifier flags + key code).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub modifiers: KeyModifiers,
    pub code: KeyCode,
}

impl KeyCombo {
    pub fn new(modifiers: KeyModifiers, code: KeyCode) -> Self {
        Self { modifiers, code }
    }

    /// Check if a `KeyEvent` matches this combo.
    pub fn matches(&self, key: &KeyEvent) -> bool {
        key.modifiers == self.modifiers && key.code == self.code
    }

    /// Human-readable label for display.
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("Alt");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("Shift");
        }
        let key_str = match self.code {
            KeyCode::Char(c) => c.to_uppercase().to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Left => "Left".to_string(),
            KeyCode::Right => "Right".to_string(),
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            KeyCode::F(n) => format!("F{n}"),
            _ => "?".to_string(),
        };
        parts.push(&key_str);
        // Temporary String to own the joined result.
        parts.join("+")
    }
}

/// Keybinding registry: maps key combos to actions.
pub struct KeybindingRegistry {
    bindings: HashMap<KeyCombo, Action>,
}

impl KeybindingRegistry {
    /// Create registry with default keybindings.
    pub fn with_defaults() -> Self {
        let mut bindings = HashMap::new();

        // Quit.
        bindings.insert(
            KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c')),
            Action::Quit,
        );

        // Submit.
        bindings.insert(
            KeyCombo::new(KeyModifiers::NONE, KeyCode::Enter),
            Action::Submit,
        );

        // Panel toggle.
        bindings.insert(
            KeyCombo::new(KeyModifiers::NONE, KeyCode::Tab),
            Action::TogglePanel,
        );

        // Readline shortcuts.
        bindings.insert(
            KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('k')),
            Action::ClearLine,
        );
        bindings.insert(
            KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('w')),
            Action::DeleteWordBackward,
        );
        bindings.insert(
            KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('u')),
            Action::DeleteToLineStart,
        );

        // Cursor movement.
        bindings.insert(
            KeyCombo::new(KeyModifiers::NONE, KeyCode::Home),
            Action::CursorHome,
        );
        bindings.insert(
            KeyCombo::new(KeyModifiers::NONE, KeyCode::End),
            Action::CursorEnd,
        );
        bindings.insert(
            KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('a')),
            Action::CursorHome,
        );
        bindings.insert(
            KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('e')),
            Action::CursorEnd,
        );

        // Word-level cursor movement.
        bindings.insert(
            KeyCombo::new(KeyModifiers::ALT, KeyCode::Left),
            Action::CursorWordLeft,
        );
        bindings.insert(
            KeyCombo::new(KeyModifiers::ALT, KeyCode::Right),
            Action::CursorWordRight,
        );

        // Multi-line input.
        bindings.insert(
            KeyCombo::new(KeyModifiers::SHIFT, KeyCode::Enter),
            Action::InsertNewline,
        );
        bindings.insert(
            KeyCombo::new(KeyModifiers::ALT, KeyCode::Enter),
            Action::InsertNewline,
        );

        // History.
        bindings.insert(
            KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('r')),
            Action::HistorySearch,
        );

        Self { bindings }
    }

    /// Look up the action for a key event.
    pub fn lookup(&self, key: &KeyEvent) -> Option<&Action> {
        let combo = KeyCombo::new(key.modifiers, key.code);
        self.bindings.get(&combo)
    }

    /// Add or override a binding.
    pub fn bind(&mut self, combo: KeyCombo, action: Action) {
        self.bindings.insert(combo, action);
    }

    /// Remove a binding.
    pub fn unbind(&mut self, combo: &KeyCombo) {
        self.bindings.remove(combo);
    }

    /// List all bindings sorted by action label.
    pub fn list_bindings(&self) -> Vec<(String, &Action)> {
        let mut list: Vec<_> = self
            .bindings
            .iter()
            .map(|(combo, action)| (combo.label(), action))
            .collect();
        list.sort_by(|a, b| a.0.cmp(&b.0));
        list
    }

    /// Load user overrides from a TOML file.
    /// Format: `"Ctrl+K" = "clear_line"`
    pub fn load_from_file(&mut self, path: &Path) {
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };

        let table: HashMap<String, String> = match toml::from_str(&content) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Failed to parse keybindings file {}: {e}", path.display());
                return;
            }
        };

        for (key_str, action_str) in &table {
            if let (Some(combo), Some(action)) = (parse_key_combo(key_str), parse_action(action_str))
            {
                self.bindings.insert(combo, action);
            } else {
                tracing::warn!("Unknown keybinding: {key_str} = {action_str}");
            }
        }
    }
}

/// Parse a key combo string like "Ctrl+K", "Alt+Enter", "Shift+Tab".
fn parse_key_combo(s: &str) -> Option<KeyCombo> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let mut modifiers = KeyModifiers::NONE;
    let mut key_part = "";

    for part in &parts {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "meta" | "opt" | "option" => modifiers |= KeyModifiers::ALT,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            _ => key_part = part,
        }
    }

    let code = match key_part.to_lowercase().as_str() {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" | "bs" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        s if s.len() == 1 => KeyCode::Char(s.chars().next()?),
        s if s.starts_with('f') => {
            let n: u8 = s[1..].parse().ok()?;
            KeyCode::F(n)
        }
        _ => return None,
    };

    Some(KeyCombo::new(modifiers, code))
}

/// Parse an action name string.
fn parse_action(s: &str) -> Option<Action> {
    match s.to_lowercase().replace('-', "_").as_str() {
        "quit" => Some(Action::Quit),
        "submit" => Some(Action::Submit),
        "toggle_vim" => Some(Action::ToggleVim),
        "toggle_panel" => Some(Action::TogglePanel),
        "scroll_up" => Some(Action::ScrollUp),
        "scroll_down" => Some(Action::ScrollDown),
        "page_up" => Some(Action::PageUp),
        "page_down" => Some(Action::PageDown),
        "clear_line" => Some(Action::ClearLine),
        "delete_word_backward" => Some(Action::DeleteWordBackward),
        "delete_to_line_start" => Some(Action::DeleteToLineStart),
        "open_search" => Some(Action::OpenSearch),
        "toggle_shortcuts" => Some(Action::ToggleShortcuts),
        "cycle_output_style" => Some(Action::CycleOutputStyle),
        "cursor_home" => Some(Action::CursorHome),
        "cursor_end" => Some(Action::CursorEnd),
        "cursor_word_left" => Some(Action::CursorWordLeft),
        "cursor_word_right" => Some(Action::CursorWordRight),
        "insert_newline" => Some(Action::InsertNewline),
        "history_prev" => Some(Action::HistoryPrev),
        "history_next" => Some(Action::HistoryNext),
        "history_search" => Some(Action::HistorySearch),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_bindings_exist() {
        let reg = KeybindingRegistry::with_defaults();
        let quit_key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(reg.lookup(&quit_key), Some(&Action::Quit));
    }

    #[test]
    fn test_custom_binding() {
        let mut reg = KeybindingRegistry::with_defaults();
        reg.bind(
            KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('s')),
            Action::Submit,
        );
        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(reg.lookup(&key), Some(&Action::Submit));
    }

    #[test]
    fn test_parse_key_combo() {
        let combo = parse_key_combo("Ctrl+K").unwrap();
        assert_eq!(combo.modifiers, KeyModifiers::CONTROL);
        assert_eq!(combo.code, KeyCode::Char('k'));
    }

    #[test]
    fn test_parse_key_combo_alt_enter() {
        let combo = parse_key_combo("Alt+Enter").unwrap();
        assert_eq!(combo.modifiers, KeyModifiers::ALT);
        assert_eq!(combo.code, KeyCode::Enter);
    }

    #[test]
    fn test_parse_action() {
        assert_eq!(parse_action("quit"), Some(Action::Quit));
        assert_eq!(parse_action("toggle_vim"), Some(Action::ToggleVim));
        assert_eq!(parse_action("toggle-vim"), Some(Action::ToggleVim));
        assert_eq!(parse_action("unknown_action"), None);
    }

    #[test]
    fn test_key_combo_label() {
        let combo = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('k'));
        assert_eq!(combo.label(), "Ctrl+K");
    }

    #[test]
    fn test_list_bindings() {
        let reg = KeybindingRegistry::with_defaults();
        let list = reg.list_bindings();
        assert!(!list.is_empty());
    }

    #[test]
    fn test_unbind() {
        let mut reg = KeybindingRegistry::with_defaults();
        let combo = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c'));
        reg.unbind(&combo);
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(reg.lookup(&key), None);
    }

    #[test]
    fn test_load_from_file_nonexistent() {
        let mut reg = KeybindingRegistry::with_defaults();
        reg.load_from_file(Path::new("/nonexistent/path/keybindings.toml"));
        // Should not panic, bindings unchanged.
        assert!(!reg.list_bindings().is_empty());
    }
}
