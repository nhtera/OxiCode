//! Vim mode state machine for the TUI input box.
//!
//! Supports Normal, Insert, Visual, and Command modes with standard vim motions.
//! Activated via `/vim` command or `vim_mode` feature flag in settings.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Vim editing modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Normal mode: navigation, operators, motions.
    Normal,
    /// Insert mode: direct text input.
    Insert,
    /// Visual mode: character-level selection.
    Visual,
    /// Visual Line mode: line-level selection.
    VisualLine,
    /// Command-line mode (`:` prefix).
    Command,
}

impl Mode {
    /// Short label for status bar / input box badge.
    pub fn badge(&self) -> &'static str {
        match self {
            Mode::Normal => "N",
            Mode::Insert => "I",
            Mode::Visual => "V",
            Mode::VisualLine => "VL",
            Mode::Command => "C",
        }
    }
}

/// Result of processing a key event in vim mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimAction {
    /// No-op (key consumed, nothing to do).
    Noop,
    /// Insert a character at cursor.
    InsertChar(char),
    /// Delete character(s) at cursor.
    DeleteChar,
    /// Delete character before cursor (backspace).
    DeleteCharBefore,
    /// Move cursor to absolute position (char index).
    MoveCursor(usize),
    /// Move cursor by relative offset (can be negative).
    MoveCursorBy(isize),
    /// Delete the entire current line (dd).
    DeleteLine,
    /// Yank (copy) the current line.
    YankLine,
    /// Paste yanked text at cursor.
    Paste,
    /// Undo last change.
    Undo,
    /// Enter command-line mode.
    EnterCommandMode,
    /// Execute a command-line string (e.g. ":q", ":w").
    ExecuteCommand(String),
    /// Enter search mode (/ key).
    EnterSearch,
    /// Switch to insert mode at position.
    SwitchToInsert,
    /// Submit input (Enter in insert mode).
    Submit,
    /// Quit request (:q).
    Quit,
    /// Pass key through to default handler (not consumed by vim).
    Passthrough(KeyEvent),
    /// Move cursor to start of line.
    MoveToLineStart,
    /// Move cursor to end of line.
    MoveToLineEnd,
    /// Move cursor to start of text (gg).
    MoveToStart,
    /// Move cursor to end of text (G).
    MoveToEnd,
    /// Move forward one word (with count).
    MoveWordForward(usize),
    /// Move backward one word (with count).
    MoveWordBackward(usize),
    /// Move to end of word (with count).
    MoveWordEnd(usize),
    /// Delete from cursor to end of line (D).
    DeleteToEnd,
    /// Delete word forward (dw).
    DeleteWordForward,
    /// Delete word backward (db).
    DeleteWordBackward,
    /// Change to end of line (C): delete to end + enter insert.
    ChangeToEnd,
    /// Append after cursor (a).
    AppendAfterCursor,
    /// Append at end of line (A).
    AppendAtEnd,
    /// Insert at beginning of line (I as in capital-i).
    InsertAtLineStart,
    /// Open line below (o).
    OpenLineBelow,
    /// Delete a range defined by char indices (start, end exclusive).
    DeleteRange(usize, usize),
    /// Change a range (delete + enter insert).
    ChangeRange(usize, usize),
    /// Yank a range to register.
    YankRange(usize, usize),
    /// Enter Visual Line mode (V).
    EnterVisualLine,
    /// Delete text object: (modifier 'i'/'a', target char like 'w'/'"'/'(' etc).
    DeleteTextObject(char, char),
    /// Change text object: delete + enter insert.
    ChangeTextObject(char, char),
    /// Yank text object to register.
    YankTextObject(char, char),
}

/// Vim mode state machine.
pub struct VimState {
    /// Current editing mode.
    pub mode: Mode,
    /// Pending operator (e.g. 'd' waiting for motion).
    pending_op: Option<char>,
    /// Pending text object modifier ('i' for inner, 'a' for around).
    pending_text_obj: Option<char>,
    /// Pending count prefix (e.g. "3" in "3dd").
    count_buf: String,
    /// Command-line buffer (after ':').
    command_buf: String,
    /// Yank register (single line for now).
    yank_register: String,
    /// Visual mode anchor position.
    pub visual_anchor: usize,
    /// Whether vim mode is enabled.
    pub enabled: bool,
}

impl VimState {
    pub fn new(enabled: bool) -> Self {
        Self {
            mode: Mode::Normal,
            pending_op: None,
            pending_text_obj: None,
            count_buf: String::new(),
            command_buf: String::new(),
            yank_register: String::new(),
            visual_anchor: 0,
            enabled,
        }
    }

    /// Enable/disable vim mode. Resets to Normal mode.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            self.mode = Mode::Normal;
        }
        self.pending_op = None;
        self.pending_text_obj = None;
        self.count_buf.clear();
        self.command_buf.clear();
    }

    /// Store text in yank register.
    pub fn yank(&mut self, text: &str) {
        self.yank_register = text.to_string();
    }

    /// Get yanked text.
    pub fn yanked(&self) -> &str {
        &self.yank_register
    }

    /// Set visual anchor position (called when entering visual mode).
    pub fn set_visual_anchor(&mut self, pos: usize) {
        self.visual_anchor = pos;
    }

    /// Get visual anchor position.
    pub fn visual_anchor(&self) -> usize {
        self.visual_anchor
    }

    /// Get current command buffer (for command mode display).
    pub fn command_buffer(&self) -> &str {
        &self.command_buf
    }

    /// Get the repeat count from count buffer (default 1).
    fn consume_count(&mut self) -> usize {
        let count = self.count_buf.parse::<usize>().unwrap_or(1);
        self.count_buf.clear();
        count
    }

    /// Process a key event and return the action to take.
    /// `text_len` is the current text length in chars (for bounds).
    pub fn handle_key(&mut self, key: KeyEvent, text_len: usize) -> VimAction {
        if !self.enabled {
            return VimAction::Passthrough(key);
        }

        match self.mode {
            Mode::Normal => self.handle_normal(key, text_len),
            Mode::Insert => self.handle_insert(key),
            Mode::Visual | Mode::VisualLine => self.handle_visual(key, text_len),
            Mode::Command => self.handle_command(key),
        }
    }

    /// Handle key in Normal mode.
    #[allow(clippy::too_many_lines)]
    fn handle_normal(&mut self, key: KeyEvent, text_len: usize) -> VimAction {
        // Accumulate count prefix digits (1-9 first, 0-9 after).
        if let KeyCode::Char(c) = key.code {
            if c.is_ascii_digit() && (!self.count_buf.is_empty() || c != '0') {
                self.count_buf.push(c);
                return VimAction::Noop;
            }
        }

        // Check for pending text object modifier (i/a after operator).
        if let Some(modifier) = self.pending_text_obj {
            if let Some(op) = self.pending_op {
                let result = self.resolve_text_object(op, modifier, key);
                self.pending_op = None;
                self.pending_text_obj = None;
                return result;
            }
            self.pending_text_obj = None;
        }

        // Check for pending operator + motion.
        if let Some(op) = self.pending_op {
            return self.handle_operator_motion(op, key, text_len);
        }

        match (key.modifiers, key.code) {
            // Mode switches.
            (_, KeyCode::Char('i')) => {
                self.mode = Mode::Insert;
                VimAction::SwitchToInsert
            }
            (_, KeyCode::Char('a')) => {
                self.mode = Mode::Insert;
                VimAction::AppendAfterCursor
            }
            (_, KeyCode::Char('A')) => {
                self.mode = Mode::Insert;
                VimAction::AppendAtEnd
            }
            (_, KeyCode::Char('I')) => {
                self.mode = Mode::Insert;
                VimAction::InsertAtLineStart
            }
            (_, KeyCode::Char('o')) => {
                self.mode = Mode::Insert;
                VimAction::OpenLineBelow
            }
            (_, KeyCode::Char('v')) => {
                self.mode = Mode::Visual;
                self.visual_anchor = 0; // Will be set by caller to current cursor
                VimAction::Noop
            }
            (_, KeyCode::Char('V')) => {
                self.mode = Mode::VisualLine;
                self.visual_anchor = 0;
                VimAction::EnterVisualLine
            }

            // Basic motions.
            (_, KeyCode::Char('h') | KeyCode::Left) => {
                let n = self.consume_count();
                #[allow(clippy::cast_possible_wrap)]
                VimAction::MoveCursorBy(-(n as isize))
            }
            (_, KeyCode::Char('l') | KeyCode::Right) => {
                let n = self.consume_count();
                #[allow(clippy::cast_possible_wrap)]
                VimAction::MoveCursorBy(n as isize)
            }
            (_, KeyCode::Char('j' | 'k') | KeyCode::Down | KeyCode::Up) => {
                // In single-line input, j/k and arrows do nothing meaningful.
                VimAction::Noop
            }

            // Word motions.
            (_, KeyCode::Char('w')) => {
                let n = self.consume_count();
                VimAction::MoveWordForward(n)
            }
            (_, KeyCode::Char('b')) => {
                let n = self.consume_count();
                VimAction::MoveWordBackward(n)
            }
            (_, KeyCode::Char('e')) => {
                let n = self.consume_count();
                VimAction::MoveWordEnd(n)
            }

            // Line motions.
            (_, KeyCode::Char('0' | '^')) => VimAction::MoveToLineStart,
            (_, KeyCode::Char('$')) => VimAction::MoveToLineEnd,

            // Global motions.
            (_, KeyCode::Char('g')) => {
                // Wait for second 'g' → gg.
                self.pending_op = Some('g');
                VimAction::Noop
            }
            (_, KeyCode::Char('G')) => VimAction::MoveToEnd,

            // Operators that need a motion.
            (_, KeyCode::Char('d')) => {
                self.pending_op = Some('d');
                VimAction::Noop
            }
            (_, KeyCode::Char('y')) => {
                self.pending_op = Some('y');
                VimAction::Noop
            }

            // Ctrl+C: pass through for quit (must be before 'c' operator).
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => VimAction::Passthrough(key),

            (_, KeyCode::Char('c')) => {
                self.pending_op = Some('c');
                VimAction::Noop
            }

            // Single-key operators.
            (_, KeyCode::Char('x')) => {
                let _n = self.consume_count();
                VimAction::DeleteChar
            }
            (_, KeyCode::Char('X')) => VimAction::DeleteCharBefore,
            (_, KeyCode::Char('p')) => VimAction::Paste,
            (_, KeyCode::Char('u')) => VimAction::Undo,

            // Delete to end of line (D).
            (_, KeyCode::Char('D')) => VimAction::DeleteToEnd,
            // Change to end of line (C).
            (_, KeyCode::Char('C')) => {
                self.mode = Mode::Insert;
                VimAction::ChangeToEnd
            }

            // Command/search.
            (_, KeyCode::Char(':')) => {
                self.mode = Mode::Command;
                self.command_buf.clear();
                VimAction::EnterCommandMode
            }
            (_, KeyCode::Char('/')) => VimAction::EnterSearch,

            // Esc clears pending state.
            (_, KeyCode::Esc) => {
                self.pending_op = None;
                self.pending_text_obj = None;
                self.count_buf.clear();
                VimAction::Noop
            }

            _ => VimAction::Noop,
        }
    }

    /// Handle pending operator + motion (e.g. dd, dw, yy, cw).
    fn handle_operator_motion(&mut self, op: char, key: KeyEvent, _text_len: usize) -> VimAction {
        let _count = self.consume_count();

        match (op, key.code) {
            // Text object modifier: di_, ci_, yi_, da_, ca_, ya_
            (_, KeyCode::Char('i' | 'a')) => {
                if let KeyCode::Char(modifier) = key.code {
                    self.pending_text_obj = Some(modifier);
                    return VimAction::Noop;
                }
                self.pending_op = None;
                VimAction::Noop
            }

            // dd = delete line, yy = yank line, cc = change line.
            ('d', KeyCode::Char('d')) => {
                self.pending_op = None;
                VimAction::DeleteLine
            }
            ('y', KeyCode::Char('y')) => {
                self.pending_op = None;
                VimAction::YankLine
            }
            ('c', KeyCode::Char('c')) => {
                self.pending_op = None;
                self.mode = Mode::Insert;
                VimAction::DeleteLine
            }

            // dw = delete word, cw = change word.
            ('d', KeyCode::Char('w')) => {
                self.pending_op = None;
                VimAction::DeleteWordForward
            }
            ('c', KeyCode::Char('w')) => {
                self.pending_op = None;
                self.mode = Mode::Insert;
                VimAction::DeleteWordForward
            }

            // db = delete word backward.
            ('d', KeyCode::Char('b')) => {
                self.pending_op = None;
                VimAction::DeleteWordBackward
            }

            // d$ = delete to end.
            ('d', KeyCode::Char('$')) => {
                self.pending_op = None;
                VimAction::DeleteToEnd
            }
            ('c', KeyCode::Char('$')) => {
                self.pending_op = None;
                self.mode = Mode::Insert;
                VimAction::ChangeToEnd
            }

            // gg = go to start.
            ('g', KeyCode::Char('g')) => {
                self.pending_op = None;
                VimAction::MoveToStart
            }

            _ => {
                self.pending_op = None;
                VimAction::Noop
            }
        }
    }

    /// Resolve a text object: operator + modifier (i/a) + target char.
    /// `text` is provided by the caller — but we only have key here.
    /// Returns a VimAction with the range for the caller to resolve.
    fn resolve_text_object(&mut self, op: char, modifier: char, key: KeyEvent) -> VimAction {
        // The key determines the text object target. We emit a specialized action
        // that app.rs will handle using vim_text_objects functions with actual text.
        let KeyCode::Char(target) = key.code else {
            return VimAction::Noop;
        };

        // Validate target is a recognized text object.
        let valid = matches!(
            target,
            'w' | '"' | '\'' | '(' | ')' | '{' | '}' | '[' | ']' | '<' | '>' | '`'
        );
        if !valid {
            return VimAction::Noop;
        }

        match op {
            'd' => VimAction::DeleteTextObject(modifier, target),
            'c' => {
                self.mode = Mode::Insert;
                VimAction::ChangeTextObject(modifier, target)
            }
            'y' => VimAction::YankTextObject(modifier, target),
            _ => VimAction::Noop,
        }
    }

    /// Handle key in Insert mode.
    fn handle_insert(&mut self, key: KeyEvent) -> VimAction {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.mode = Mode::Normal;
                VimAction::Noop
            }
            (_, KeyCode::Enter) => VimAction::Submit,
            (_, KeyCode::Backspace) => VimAction::DeleteCharBefore,
            (_, KeyCode::Delete) => VimAction::DeleteChar,
            (_, KeyCode::Left) => VimAction::MoveCursorBy(-1),
            (_, KeyCode::Right) => VimAction::MoveCursorBy(1),
            (_, KeyCode::Home) => VimAction::MoveToLineStart,
            (_, KeyCode::End) => VimAction::MoveToLineEnd,
            // Ctrl combos must be before the wildcard Char(c) arm.
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => VimAction::Passthrough(key),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => VimAction::DeleteWordBackward,
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => VimAction::DeleteLine,
            (_, KeyCode::Char(c)) => VimAction::InsertChar(c),
            _ => VimAction::Noop,
        }
    }

    /// Handle key in Visual mode.
    fn handle_visual(&mut self, key: KeyEvent, _text_len: usize) -> VimAction {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.mode = Mode::Normal;
                VimAction::Noop
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => VimAction::Passthrough(key),
            (_, KeyCode::Char('h') | KeyCode::Left) => VimAction::MoveCursorBy(-1),
            (_, KeyCode::Char('l') | KeyCode::Right) => VimAction::MoveCursorBy(1),
            (_, KeyCode::Char('w')) => VimAction::MoveWordForward(1),
            (_, KeyCode::Char('b')) => VimAction::MoveWordBackward(1),
            (_, KeyCode::Char('0')) => VimAction::MoveToLineStart,
            (_, KeyCode::Char('$')) => VimAction::MoveToLineEnd,
            // d/x in visual = delete selection range.
            (_, KeyCode::Char('d' | 'x')) => {
                self.mode = Mode::Normal;
                VimAction::DeleteRange(0, 0) // Placeholder — app.rs resolves with anchor+cursor
            }
            // c in visual = change selection range.
            (_, KeyCode::Char('c')) => {
                self.mode = Mode::Insert;
                VimAction::ChangeRange(0, 0) // Placeholder — app.rs resolves with anchor+cursor
            }
            // y in visual = yank selection range.
            (_, KeyCode::Char('y')) => {
                self.mode = Mode::Normal;
                VimAction::YankRange(0, 0) // Placeholder — app.rs resolves with anchor+cursor
            }
            _ => VimAction::Noop,
        }
    }

    /// Handle key in Command mode (: prefix).
    fn handle_command(&mut self, key: KeyEvent) -> VimAction {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                self.mode = Mode::Normal;
                self.command_buf.clear();
                VimAction::Noop
            }
            (_, KeyCode::Enter) => {
                let cmd = self.command_buf.trim().to_string();
                self.command_buf.clear();
                self.mode = Mode::Normal;
                match cmd.as_str() {
                    "q" | "quit" | "wq" => VimAction::Quit,
                    "w" | "write" => VimAction::Noop, // No file to save in input
                    _ => VimAction::ExecuteCommand(cmd),
                }
            }
            (_, KeyCode::Backspace) => {
                if self.command_buf.is_empty() {
                    self.mode = Mode::Normal;
                } else {
                    self.command_buf.pop();
                }
                VimAction::Noop
            }
            (_, KeyCode::Char(c)) => {
                self.command_buf.push(c);
                VimAction::Noop
            }
            _ => VimAction::Noop,
        }
    }
}

/// Find the next word boundary position (forward) from `pos` in `text`.
pub fn next_word_pos(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if pos >= len {
        return len;
    }
    let mut i = pos;
    // Skip current word characters.
    while i < len && !chars[i].is_whitespace() {
        i += 1;
    }
    // Skip whitespace.
    while i < len && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

/// Find the previous word boundary position (backward) from `pos` in `text`.
pub fn prev_word_pos(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    if pos == 0 {
        return 0;
    }
    let mut i = pos;
    // Skip whitespace before cursor.
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    // Skip word characters.
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

/// Find end of current word from `pos` in `text`.
pub fn word_end_pos(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if pos >= len.saturating_sub(1) {
        return len.saturating_sub(1);
    }
    let mut i = pos + 1;
    // Skip whitespace.
    while i < len && chars[i].is_whitespace() {
        i += 1;
    }
    // Skip to end of word.
    while i < len && !chars[i].is_whitespace() {
        i += 1;
    }
    i.saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn test_initial_state() {
        let vim = VimState::new(true);
        assert_eq!(vim.mode, Mode::Normal);
        assert!(vim.enabled);
    }

    #[test]
    fn test_disabled_passthrough() {
        let mut vim = VimState::new(false);
        let action = vim.handle_key(key(KeyCode::Char('h')), 10);
        assert!(matches!(action, VimAction::Passthrough(_)));
    }

    #[test]
    fn test_normal_to_insert() {
        let mut vim = VimState::new(true);
        let action = vim.handle_key(key(KeyCode::Char('i')), 5);
        assert_eq!(action, VimAction::SwitchToInsert);
        assert_eq!(vim.mode, Mode::Insert);
    }

    #[test]
    fn test_insert_to_normal() {
        let mut vim = VimState::new(true);
        vim.mode = Mode::Insert;
        let action = vim.handle_key(key(KeyCode::Esc), 5);
        assert_eq!(action, VimAction::Noop);
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn test_hjkl_motions() {
        let mut vim = VimState::new(true);
        assert_eq!(
            vim.handle_key(key(KeyCode::Char('h')), 10),
            VimAction::MoveCursorBy(-1)
        );
        assert_eq!(
            vim.handle_key(key(KeyCode::Char('l')), 10),
            VimAction::MoveCursorBy(1)
        );
    }

    #[test]
    fn test_dd_delete_line() {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('d')), 10);
        let action = vim.handle_key(key(KeyCode::Char('d')), 10);
        assert_eq!(action, VimAction::DeleteLine);
    }

    #[test]
    fn test_yy_yank_line() {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('y')), 10);
        let action = vim.handle_key(key(KeyCode::Char('y')), 10);
        assert_eq!(action, VimAction::YankLine);
    }

    #[test]
    fn test_gg_go_to_start() {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('g')), 10);
        let action = vim.handle_key(key(KeyCode::Char('g')), 10);
        assert_eq!(action, VimAction::MoveToStart);
    }

    #[test]
    fn test_command_mode_quit() {
        let mut vim = VimState::new(true);
        vim.handle_key(shift_key(':'), 10);
        assert_eq!(vim.mode, Mode::Command);
        vim.handle_key(key(KeyCode::Char('q')), 10);
        let action = vim.handle_key(key(KeyCode::Enter), 10);
        assert_eq!(action, VimAction::Quit);
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn test_insert_mode_typing() {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('i')), 0);
        assert_eq!(vim.mode, Mode::Insert);
        assert_eq!(
            vim.handle_key(key(KeyCode::Char('x')), 0),
            VimAction::InsertChar('x')
        );
        assert_eq!(
            vim.handle_key(key(KeyCode::Backspace), 1),
            VimAction::DeleteCharBefore
        );
    }

    #[test]
    fn test_mode_badge() {
        assert_eq!(Mode::Normal.badge(), "N");
        assert_eq!(Mode::Insert.badge(), "I");
        assert_eq!(Mode::Visual.badge(), "V");
        assert_eq!(Mode::Command.badge(), "C");
    }

    #[test]
    fn test_word_navigation() {
        let text = "hello world foo";
        assert_eq!(next_word_pos(text, 0), 6);
        assert_eq!(next_word_pos(text, 6), 12);
        assert_eq!(prev_word_pos(text, 6), 0);
        assert_eq!(prev_word_pos(text, 12), 6);
        assert_eq!(word_end_pos(text, 0), 4);
    }

    #[test]
    fn test_search_mode() {
        let mut vim = VimState::new(true);
        let action = vim.handle_key(key(KeyCode::Char('/')), 10);
        assert_eq!(action, VimAction::EnterSearch);
    }

    #[test]
    fn test_visual_mode() {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('v')), 10);
        assert_eq!(vim.mode, Mode::Visual);
        vim.handle_key(key(KeyCode::Esc), 10);
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn test_count_prefix() {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('3')), 20);
        let action = vim.handle_key(key(KeyCode::Char('l')), 20);
        assert_eq!(action, VimAction::MoveCursorBy(3));
    }

    #[test]
    fn test_yank_register() {
        let mut vim = VimState::new(true);
        vim.yank("hello");
        assert_eq!(vim.yanked(), "hello");
    }

    #[test]
    fn test_ctrl_c_passthrough() {
        let mut vim = VimState::new(true);
        let action = vim.handle_key(ctrl_key('c'), 10);
        assert!(matches!(action, VimAction::Passthrough(_)));
    }

    #[test]
    fn test_dw_delete_word() {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('d')), 10);
        let action = vim.handle_key(key(KeyCode::Char('w')), 10);
        assert_eq!(action, VimAction::DeleteWordForward);
    }

    #[test]
    fn test_x_delete_char() {
        let mut vim = VimState::new(true);
        let action = vim.handle_key(key(KeyCode::Char('x')), 10);
        assert_eq!(action, VimAction::DeleteChar);
    }

    #[test]
    fn test_disabled_default_state_is_normal() {
        // When disabled, the mode field still holds Normal — but all keys are passed through.
        let vim = VimState::new(false);
        assert_eq!(vim.mode, Mode::Normal, "disabled vim still starts in Normal mode internally");
        assert!(!vim.enabled);
    }

    #[test]
    fn test_enabled_default_state_is_normal() {
        let vim = VimState::new(true);
        assert_eq!(vim.mode, Mode::Normal);
        assert!(vim.enabled);
    }

    #[test]
    fn test_p_paste_in_normal_mode() {
        let mut vim = VimState::new(true);
        let action = vim.handle_key(key(KeyCode::Char('p')), 10);
        assert_eq!(action, VimAction::Paste);
    }

    #[test]
    fn test_w_move_word_forward() {
        let mut vim = VimState::new(true);
        let action = vim.handle_key(key(KeyCode::Char('w')), 10);
        assert_eq!(action, VimAction::MoveWordForward(1));
    }

    #[test]
    fn test_b_move_word_backward() {
        let mut vim = VimState::new(true);
        let action = vim.handle_key(key(KeyCode::Char('b')), 10);
        assert_eq!(action, VimAction::MoveWordBackward(1));
    }

    #[test]
    fn test_set_enabled_resets_state() {
        let mut vim = VimState::new(true);
        vim.mode = Mode::Insert;
        vim.set_enabled(false);
        // After disabling, mode is not forced to Normal (no enum change when disabling).
        assert!(!vim.enabled);
        // Re-enabling sets to Normal.
        vim.set_enabled(true);
        assert_eq!(vim.mode, Mode::Normal);
        assert!(vim.enabled);
    }

    #[test]
    fn test_v_enters_visual_mode() {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('v')), 10);
        assert_eq!(vim.mode, Mode::Visual);
    }

    #[test]
    fn test_esc_in_insert_returns_to_normal() {
        let mut vim = VimState::new(true);
        // Start in Insert.
        vim.handle_key(key(KeyCode::Char('i')), 5);
        assert_eq!(vim.mode, Mode::Insert);
        // Esc returns to Normal.
        vim.handle_key(key(KeyCode::Esc), 5);
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn test_mode_badges_all_variants() {
        assert_eq!(Mode::Normal.badge(), "N");
        assert_eq!(Mode::Insert.badge(), "I");
        assert_eq!(Mode::Visual.badge(), "V");
        assert_eq!(Mode::VisualLine.badge(), "VL");
        assert_eq!(Mode::Command.badge(), "C");
    }
}
