/// Key dispatch: handle_key, handle_vim_key, search, history, scroll.
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::vim_mode::{self, VimAction};
use crate::vim_text_objects;
use crate::widgets::Notification;

use super::utils::char_to_byte_index;

impl super::App {
    #[allow(clippy::too_many_lines)]
    pub(super) async fn handle_key(&mut self, key: KeyEvent) {
        // Clear Ctrl+C hint on any non-Ctrl+C keypress.
        if !(key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c')) {
            self.ctrl_c_hint_visible = false;
        }

        // Paste preview modal takes highest priority.
        if self.pending_paste.is_some() {
            self.handle_paste_preview_key(key);
            return;
        }

        // History search overlay captures keys when active.
        if self.history_search.is_some() {
            self.handle_history_search_key(key);
            return;
        }

        // Permission dialog takes priority over all other input.
        if self.pending_permission.is_some() {
            self.handle_permission_key(key).await;
            return;
        }

        // MCP elicitation dialog — same priority tier as permissions.
        if self.pending_elicitation.is_some() {
            self.handle_elicitation_key(key);
            return;
        }

        // Search overlay captures keys when active.
        if self.search.is_active() {
            self.handle_search_key(key);
            return;
        }

        // Help overlay captures keys when visible: filter input, scroll, close.
        if self.shortcuts.is_visible() {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => self.shortcuts.hide(),
                (_, KeyCode::Up) => self.shortcuts.scroll_up(),
                (_, KeyCode::Down) => self.shortcuts.scroll_down(100),
                (_, KeyCode::Char(c)) => self.shortcuts.push_filter_char(c),
                (_, KeyCode::Backspace) => self.shortcuts.pop_filter_char(),
                _ => {}
            }
            return;
        }

        // Model picker captures keys when visible: navigate, filter, select, close.
        if self.model_picker.is_visible() {
            self.handle_model_picker_key(key).await;
            return;
        }

        // Session browser captures keys when visible: navigate, rename, resume, close.
        if self.session_browser.is_visible() {
            self.handle_session_browser_key(key).await;
            return;
        }

        // Rewind overlay captures keys when visible: navigate, confirm, close.
        if self.rewind_overlay.is_visible() {
            self.handle_rewind_key(key).await;
            return;
        }

        // Branches overlay captures keys when visible: navigate, switch, rename, delete.
        if self.branches_overlay.is_visible() {
            self.handle_branches_overlay_key(key).await;
            return;
        }

        // Stats dashboard captures keys when visible: tab switch, close.
        if self.stats_dashboard.is_visible() {
            self.handle_stats_key(key);
            return;
        }

        // Diff viewer captures keys when visible: navigate, scroll, collapse, close.
        if self.diff_viewer.is_visible() {
            self.handle_diff_viewer_key(key);
            return;
        }

        // Agents overlay (/agents).
        if self.agents_overlay.is_visible() {
            self.handle_agents_overlay_key(key).await;
            return;
        }

        // Settings screen overlay (Ctrl+, or /settings).
        if self.pending_settings.is_some() {
            self.handle_settings_key(key);
            return;
        }

        // Ctrl+, opens the settings overlay from anywhere in normal mode.
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char(',') {
            self.open_settings_screen();
            return;
        }

        // Ctrl+B opens the branches overlay from anywhere in normal mode.
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('b') {
            self.open_branches_overlay();
            return;
        }

        // Command autocomplete dropdown captures keys when active.
        if self.autocomplete.active {
            self.handle_autocomplete_key(key);
            return;
        }

        // Vim mode: dispatch through vim state machine.
        if self.vim.enabled {
            self.handle_vim_key(key).await;
            return;
        }

        // Check keybinding registry first for non-vim mode.
        if let Some(action) = self.keybindings.lookup(&key).cloned() {
            self.execute_keybinding_action(action).await;
            return;
        }

        // Default key handling (non-vim mode).
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                self.handle_ctrl_c().await;
            }
            (_, KeyCode::Esc) if self.is_turn_active => {
                self.signal_interrupt().await;
                self.last_interrupt = Some(Instant::now());
                self.notifications.push(Notification::new(
                    "Interrupting...".to_string(),
                    crate::widgets::notification::NotificationLevel::Info,
                ));
            }
            (_, KeyCode::Enter) if key.modifiers.contains(KeyModifiers::ALT) => {
                // Alt+Enter: insert newline for multiline input.
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert(byte_idx, '\n');
                self.input_cursor += 1;
            }
            (_, KeyCode::Enter) => {
                self.submit_input().await;
            }
            // Select suggestion chip with plain digit (1/2/3) when visible.
            // Plain digits are safe here because input is empty — no text to lose.
            // Also support Ctrl+digit as fallback for terminals that pass it.
            (_, KeyCode::Char(c @ '1'..='3'))
                if self.input_text.is_empty() && !self.suggestions.is_empty() =>
            {
                let idx = (c as usize) - ('1' as usize);
                if let Some(suggestion) = self.suggestions.get(idx) {
                    self.input_text.clone_from(&suggestion.prompt);
                    self.input_cursor = self.input_text.chars().count();
                    self.suggestions.clear();
                    self.submit_input().await;
                }
            }
            // `?` toggles help overlay when input is empty (no conflict with typing).
            (_, KeyCode::Char('?')) if self.input_text.is_empty() => {
                self.shortcuts.toggle();
            }
            // Ctrl+V: try image paste first, then let crossterm bracketed paste handle text.
            (KeyModifiers::CONTROL, KeyCode::Char('v')) => {
                if let Some(mut img) = crate::image_paste::read_clipboard_image() {
                    // Use global image counter for session-wide numbering.
                    let image_num =
                        self.message_cache.image_counter() + self.pending_images.len() + 1;
                    // Cache image to session-scoped directory.
                    let session_id = self.state_rx.borrow().session_id.clone();
                    img.path = crate::image_paste::cache_image(&session_id, image_num, &img.path);
                    let msg = match img.dimensions {
                        Some((w, h)) => {
                            format!("[Image #{image_num}] attached ({w}x{h})")
                        }
                        None => format!("[Image #{image_num}] attached"),
                    };
                    self.pending_images.push(img);
                    // Insert [Image #N] placeholder at cursor position in input text.
                    let tag = format!("[Image #{image_num}]");
                    let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                    // Add trailing space for readability.
                    let insert = format!("{tag} ");
                    let char_len = insert.chars().count();
                    self.input_text.insert_str(byte_idx, &insert);
                    self.input_cursor += char_len;
                    self.notifications.push(Notification::new(
                        msg,
                        crate::widgets::notification::NotificationLevel::Info,
                    ));
                }
                // If no image found, text paste is handled by Event::Paste from crossterm.
            }
            // Ctrl+R: open reverse history search.
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                self.open_history_search();
            }
            // H3 FIX: cursor operates on char count, insert at byte offset.
            (_, KeyCode::Char(c)) => {
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert(byte_idx, c);
                self.input_cursor += 1;
                self.update_ghost_text();
                self.suggestions.clear(); // hide chips once user starts typing
                                          // Activate autocomplete when typing '/' at start of input.
                if c == '/' && self.input_text == "/" {
                    self.activate_autocomplete();
                }
            }
            (_, KeyCode::Backspace) => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let start = char_to_byte_index(&self.input_text, self.input_cursor);
                    let end = char_to_byte_index(&self.input_text, self.input_cursor + 1);
                    self.input_text.replace_range(start..end, "");
                }
                self.update_ghost_text();
            }
            // Ctrl+Left / Ctrl+Right adjust the split ratio by ±5 %.
            (KeyModifiers::CONTROL, KeyCode::Left) => {
                self.split_pane.adjust_ratio(-5);
            }
            (KeyModifiers::CONTROL, KeyCode::Right) => {
                self.split_pane.adjust_ratio(5);
            }
            (_, KeyCode::Left) => {
                self.input_cursor = self.input_cursor.saturating_sub(1);
            }
            (_, KeyCode::Right) => {
                let char_count = self.input_text.chars().count();
                if self.input_cursor < char_count {
                    self.input_cursor += 1;
                } else {
                    self.accept_ghost_text();
                }
            }
            (_, KeyCode::Up) => {
                if self.input_text.is_empty() && !self.message_queue.is_empty() {
                    let (combined, images) = self.message_queue.pop_all_editable();
                    self.input_text = combined;
                    self.input_cursor = self.input_text.chars().count();
                    self.pending_images = images;
                } else {
                    self.history_prev();
                }
            }
            (_, KeyCode::Down) => {
                self.history_next();
            }
            (_, KeyCode::PageUp) => {
                self.scroll_up_by(20);
            }
            (_, KeyCode::PageDown) => {
                self.scroll_down_by(20);
            }
            (_, KeyCode::Home) => {
                self.input_cursor = 0;
            }
            (_, KeyCode::End) => {
                self.input_cursor = self.input_text.chars().count();
            }
            // Tab: accept ghost completion → select first suggestion → toggle panel.
            (_, KeyCode::Tab) => {
                if !self.accept_ghost_text() {
                    if self.input_text.is_empty() && !self.suggestions.is_empty() {
                        // Select the first suggestion chip.
                        if let Some(suggestion) = self.suggestions.first() {
                            self.input_text.clone_from(&suggestion.prompt);
                            self.input_cursor = self.input_text.chars().count();
                            self.suggestions.clear();
                            self.submit_input().await;
                        }
                    } else {
                        self.split_pane.toggle_right();
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle vim mode key dispatch.
    #[allow(clippy::too_many_lines)]
    pub(super) async fn handle_vim_key(&mut self, key: KeyEvent) {
        let text_len = self.input_text.chars().count();
        let action = self.vim.handle_key(key, text_len);

        match action {
            VimAction::Passthrough(k) => {
                // Let Ctrl+C through for quit/interrupt.
                if k.modifiers == KeyModifiers::CONTROL && k.code == KeyCode::Char('c') {
                    self.handle_ctrl_c().await;
                }
            }
            VimAction::Noop => {
                // When entering visual mode, set the anchor to current cursor.
                if self.vim.mode == crate::vim_mode::Mode::Visual
                    || self.vim.mode == crate::vim_mode::Mode::VisualLine
                {
                    self.vim.set_visual_anchor(self.input_cursor);
                }
            }
            VimAction::SwitchToInsert
            | VimAction::EnterCommandMode
            | VimAction::ExecuteCommand(_) => {}
            VimAction::InsertChar(c) => {
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert(byte_idx, c);
                self.input_cursor += 1;
            }
            VimAction::DeleteChar => {
                let char_count = self.input_text.chars().count();
                if self.input_cursor < char_count {
                    let start = char_to_byte_index(&self.input_text, self.input_cursor);
                    let end = char_to_byte_index(&self.input_text, self.input_cursor + 1);
                    self.input_text.replace_range(start..end, "");
                }
            }
            VimAction::DeleteCharBefore => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let start = char_to_byte_index(&self.input_text, self.input_cursor);
                    let end = char_to_byte_index(&self.input_text, self.input_cursor + 1);
                    self.input_text.replace_range(start..end, "");
                }
            }
            VimAction::MoveCursor(pos) => {
                let char_count = self.input_text.chars().count();
                self.input_cursor = pos.min(char_count);
            }
            VimAction::MoveCursorBy(offset) => {
                let char_count = self.input_text.chars().count();
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
                let new_pos = (self.input_cursor as isize + offset).max(0) as usize;
                self.input_cursor = new_pos.min(char_count);
            }
            VimAction::MoveToLineStart | VimAction::MoveToStart | VimAction::InsertAtLineStart => {
                self.input_cursor = 0;
            }
            VimAction::MoveToLineEnd
            | VimAction::MoveToEnd
            | VimAction::AppendAtEnd
            | VimAction::OpenLineBelow => {
                self.input_cursor = self.input_text.chars().count();
            }
            VimAction::MoveWordForward(count) => {
                for _ in 0..count {
                    self.input_cursor =
                        vim_mode::next_word_pos(&self.input_text, self.input_cursor);
                }
            }
            VimAction::MoveWordBackward(count) => {
                for _ in 0..count {
                    self.input_cursor =
                        vim_mode::prev_word_pos(&self.input_text, self.input_cursor);
                }
            }
            VimAction::MoveWordEnd(count) => {
                for _ in 0..count {
                    self.input_cursor = vim_mode::word_end_pos(&self.input_text, self.input_cursor);
                }
            }
            VimAction::DeleteLine => {
                self.vim.yank(&self.input_text);
                self.input_text.clear();
                self.input_cursor = 0;
            }
            VimAction::YankLine => {
                self.vim.yank(&self.input_text);
            }
            VimAction::Paste => {
                let yanked = self.vim.yanked().to_string();
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert_str(byte_idx, &yanked);
                self.input_cursor += yanked.chars().count();
            }
            VimAction::Undo => {
                // Simplified undo: clears entire input. Full undo stack not yet implemented.
                self.input_text.clear();
                self.input_cursor = 0;
            }
            VimAction::DeleteToEnd | VimAction::ChangeToEnd => {
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                let deleted = self.input_text[byte_idx..].to_string();
                self.vim.yank(&deleted);
                self.input_text.truncate(byte_idx);
            }
            VimAction::DeleteWordForward => {
                let next = vim_mode::next_word_pos(&self.input_text, self.input_cursor);
                let start_byte = char_to_byte_index(&self.input_text, self.input_cursor);
                let end_byte = char_to_byte_index(&self.input_text, next);
                let deleted = self.input_text[start_byte..end_byte].to_string();
                self.vim.yank(&deleted);
                self.input_text.replace_range(start_byte..end_byte, "");
            }
            VimAction::DeleteWordBackward => {
                let prev = vim_mode::prev_word_pos(&self.input_text, self.input_cursor);
                let start_byte = char_to_byte_index(&self.input_text, prev);
                let end_byte = char_to_byte_index(&self.input_text, self.input_cursor);
                let deleted = self.input_text[start_byte..end_byte].to_string();
                self.vim.yank(&deleted);
                self.input_text.replace_range(start_byte..end_byte, "");
                self.input_cursor = prev;
            }
            VimAction::AppendAfterCursor => {
                let char_count = self.input_text.chars().count();
                if self.input_cursor < char_count {
                    self.input_cursor += 1;
                }
            }
            VimAction::Submit => {
                self.submit_input().await;
            }
            VimAction::Quit => {
                let _ = self.ui_tx.send(crate::events::UiEvent::Quit).await;
                self.should_quit = true;
            }
            VimAction::EnterSearch => {
                self.search.activate();
            }
            VimAction::DeleteRange(start, end) => {
                // For visual mode: range is (0,0) placeholder — use anchor+cursor.
                let (s, e) = if start == 0 && end == 0 {
                    let anchor = self.vim.visual_anchor();
                    let cursor = self.input_cursor;
                    (anchor.min(cursor), anchor.max(cursor).saturating_add(1))
                } else {
                    (start, end)
                };
                let char_count = self.input_text.chars().count();
                let e = e.min(char_count);
                let start_byte = char_to_byte_index(&self.input_text, s);
                let end_byte = char_to_byte_index(&self.input_text, e);
                let deleted = self.input_text[start_byte..end_byte].to_string();
                self.vim.yank(&deleted);
                self.input_text.replace_range(start_byte..end_byte, "");
                self.input_cursor = s;
            }
            VimAction::ChangeRange(start, end) => {
                let (s, e) = if start == 0 && end == 0 {
                    let anchor = self.vim.visual_anchor();
                    let cursor = self.input_cursor;
                    (anchor.min(cursor), anchor.max(cursor).saturating_add(1))
                } else {
                    (start, end)
                };
                let char_count = self.input_text.chars().count();
                let e = e.min(char_count);
                let start_byte = char_to_byte_index(&self.input_text, s);
                let end_byte = char_to_byte_index(&self.input_text, e);
                let deleted = self.input_text[start_byte..end_byte].to_string();
                self.vim.yank(&deleted);
                self.input_text.replace_range(start_byte..end_byte, "");
                self.input_cursor = s;
            }
            VimAction::YankRange(start, end) => {
                let (s, e) = if start == 0 && end == 0 {
                    let anchor = self.vim.visual_anchor();
                    let cursor = self.input_cursor;
                    (anchor.min(cursor), anchor.max(cursor).saturating_add(1))
                } else {
                    (start, end)
                };
                let char_count = self.input_text.chars().count();
                let e = e.min(char_count);
                let start_byte = char_to_byte_index(&self.input_text, s);
                let end_byte = char_to_byte_index(&self.input_text, e);
                let yanked = self.input_text[start_byte..end_byte].to_string();
                self.vim.yank(&yanked);
            }
            VimAction::EnterVisualLine => {
                self.vim.set_visual_anchor(self.input_cursor);
            }
            VimAction::DeleteTextObject(modifier, target)
            | VimAction::ChangeTextObject(modifier, target)
            | VimAction::YankTextObject(modifier, target) => {
                if let Some((start, end)) = self.resolve_text_object(modifier, target) {
                    let start_byte = char_to_byte_index(&self.input_text, start);
                    let end_byte = char_to_byte_index(&self.input_text, end);
                    let text = self.input_text[start_byte..end_byte].to_string();
                    self.vim.yank(&text);

                    if let VimAction::YankTextObject(_, _) = action {
                        // Yank only, don't delete.
                    } else {
                        self.input_text.replace_range(start_byte..end_byte, "");
                        self.input_cursor = start;
                    }
                }
            }
        }
    }

    /// Resolve a text object to a char range using the current input text.
    pub(super) fn resolve_text_object(
        &self,
        modifier: char,
        target: char,
    ) -> Option<(usize, usize)> {
        let text = &self.input_text;
        let cursor = self.input_cursor;
        let inner = modifier == 'i';

        match target {
            'w' => {
                if inner {
                    vim_text_objects::inner_word(text, cursor)
                } else {
                    vim_text_objects::a_word(text, cursor)
                }
            }
            '"' => {
                if inner {
                    vim_text_objects::inner_quote(text, cursor, '"')
                } else {
                    vim_text_objects::a_quote(text, cursor, '"')
                }
            }
            '\'' | '`' => {
                let q = target;
                if inner {
                    vim_text_objects::inner_quote(text, cursor, q)
                } else {
                    vim_text_objects::a_quote(text, cursor, q)
                }
            }
            '(' | ')' => {
                if inner {
                    vim_text_objects::inner_bracket(text, cursor, '(', ')')
                } else {
                    vim_text_objects::a_bracket(text, cursor, '(', ')')
                }
            }
            '{' | '}' => {
                if inner {
                    vim_text_objects::inner_bracket(text, cursor, '{', '}')
                } else {
                    vim_text_objects::a_bracket(text, cursor, '{', '}')
                }
            }
            '[' | ']' => {
                if inner {
                    vim_text_objects::inner_bracket(text, cursor, '[', ']')
                } else {
                    vim_text_objects::a_bracket(text, cursor, '[', ']')
                }
            }
            '<' | '>' => {
                if inner {
                    vim_text_objects::inner_bracket(text, cursor, '<', '>')
                } else {
                    vim_text_objects::a_bracket(text, cursor, '<', '>')
                }
            }
            _ => None,
        }
    }

    /// Handle search overlay key events.
    pub(super) fn handle_search_key(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.search.deactivate();
            }
            (_, KeyCode::Enter) => {
                // Close search, keep results.
                self.search.deactivate();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) | (_, KeyCode::Down) => {
                self.search.next_match();
                self.scroll_to_current_match();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) | (_, KeyCode::Up) => {
                self.search.prev_match();
                self.scroll_to_current_match();
            }
            (_, KeyCode::Char(c)) => {
                self.search.push_char(c);
                self.update_search_matches();
            }
            (_, KeyCode::Backspace) => {
                self.search.pop_char();
                self.update_search_matches();
            }
            _ => {}
        }
    }

    /// Scan the message render cache for search matches and update overlay state.
    pub(super) fn update_search_matches(&mut self) {
        let state = self.state_rx.borrow();
        let msg_count = state.messages.len();
        drop(state);
        let cached = self.message_cache.lines(msg_count);
        let positions = crate::widgets::find_matches_in_cache(cached, self.search.query());
        self.search.set_match_positions(positions);
        // Auto-scroll to first match.
        self.scroll_to_current_match();
    }

    /// Scroll the message view so the current search match is visible.
    pub(super) fn scroll_to_current_match(&mut self) {
        if let Some(line_idx) = self.search.current_match_line() {
            // Convert flattened line index to scroll offset.
            // Account for message view inner height (approx).
            let target = line_idx as u16;
            self.scroll_offset = target.saturating_sub(3); // show a few lines above match
            self.auto_scroll = false;
        }
    }

    pub(super) fn scroll_up_by(&mut self, lines: u16) {
        if lines == 0 {
            return;
        }
        if self.auto_scroll {
            self.scroll_offset = self.max_scroll_offset;
            self.auto_scroll = false;
        }
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub(super) fn scroll_down_by(&mut self, lines: u16) {
        if lines == 0 {
            return;
        }
        let current = if self.auto_scroll {
            self.max_scroll_offset
        } else {
            self.scroll_offset
        };
        let next = current.saturating_add(lines).min(self.max_scroll_offset);
        self.scroll_offset = next;
        self.auto_scroll = next >= self.max_scroll_offset;
    }
}
