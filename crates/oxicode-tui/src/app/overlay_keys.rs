/// Overlay and widget key handlers: mouse, paste, model picker, session browser,
/// rewind, stats dashboard, diff viewer, and keybinding action dispatch.
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

use crate::keybindings::Action;
use crate::vim_mode;
use crate::widgets::{Notification, SessionEntry};

use super::utils::{char_to_byte_index, format_relative_time};

impl super::App {
    /// Handle mouse events: scroll wheel, scrollbar click/drag, Cmd+click image, hover.
    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_up_by(3),
            MouseEventKind::ScrollDown => self.scroll_down_by(3),
            MouseEventKind::Down(MouseButton::Left) => {
                // Click on [Image #N] tag opens image in system viewer.
                if self.handle_image_click(mouse.column, mouse.row) {
                    return;
                }
                self.handle_scrollbar_click(mouse.column, mouse.row);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.handle_scrollbar_click(mouse.column, mouse.row);
            }
            _ => {}
        }
    }

    /// Handle click/drag on the scrollbar track — jump to proportional position.
    ///
    /// The message view has a `Block` with `Borders::ALL`, so the scrollbar track
    /// occupies rows `area.y + 1` to `area.bottom() - 2` (inside borders).
    /// The scrollbar column is at `area.right() - 1` (right border/track).
    pub(super) fn handle_scrollbar_click(&mut self, col: u16, row: u16) {
        let area = self.message_area;
        // Respond to clicks on rightmost 2 columns (scrollbar track + padding).
        let scrollbar_col = area.right().saturating_sub(1);
        if area.width < 2 || col < scrollbar_col.saturating_sub(1) || col > scrollbar_col {
            return;
        }
        // Inner track area excludes top and bottom borders.
        let track_top = area.y.saturating_add(1);
        let track_bottom = area.bottom().saturating_sub(1); // exclusive
        if row < track_top || row >= track_bottom {
            return;
        }
        if self.max_scroll_offset == 0 {
            return;
        }

        // Map row within inner track → proportional scroll position.
        let relative_y = row - track_top;
        let track_height = track_bottom.saturating_sub(track_top).max(1);
        let ratio = f32::from(relative_y) / f32::from((track_height.saturating_sub(1)).max(1));
        #[allow(clippy::cast_sign_loss)]
        let new_offset = (ratio * f32::from(self.max_scroll_offset)).round() as u16;

        self.scroll_offset = new_offset.min(self.max_scroll_offset);
        self.auto_scroll = self.scroll_offset >= self.max_scroll_offset;
    }

    /// Handle Cmd+Click (macOS) / Ctrl+Click on `[Image #N]` in message view.
    ///
    /// Returns `true` if an image was opened (click consumed).
    pub(super) fn handle_image_click(&self, col: u16, row: u16) -> bool {
        if let Some(hit) = self.find_image_tag_at(col, row) {
            if let Some(path) = self.sent_image_paths.get(&hit.image_number) {
                crate::image_paste::open_file_in_viewer(path);
                return true;
            }
        }
        false
    }

    /// Find an image tag at a screen position (gallery box or legacy inline).
    ///
    /// Returns hit info if the position maps to an image span.
    pub(super) fn find_image_tag_at(&self, col: u16, row: u16) -> Option<super::ImageTagHit> {
        let area = self.message_area;
        if col <= area.x
            || col >= area.right().saturating_sub(2)
            || row <= area.y
            || row >= area.bottom().saturating_sub(1)
        {
            return None;
        }

        let inner_top = area.y + 1;
        let inner_left = area.x + 1;
        let abs_line = row.saturating_sub(inner_top) as usize + self.scroll_offset as usize;

        let state = self.state_rx.borrow();
        let msg_count = state.messages.len();
        let msg_roles: Vec<oxicode_common::Role> = state.messages.iter().map(|m| m.role).collect();
        drop(state);

        let entries = self.message_cache.lines(msg_count);
        if entries.is_empty() {
            return None;
        }

        // Walk cached entries to locate message index + line within it.
        let mut cumulative: usize = 0;
        let mut found_msg_idx: Option<usize> = None;
        let mut line_in_msg: usize = 0;
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                let is_turn = msg_roles
                    .get(i)
                    .is_some_and(|r| *r == oxicode_common::Role::User);
                cumulative += if is_turn { 3 } else { 1 };
            }
            if abs_line < cumulative + entry.len() {
                // Click landed on a separator line (between messages), not content.
                if abs_line < cumulative {
                    return None;
                }
                found_msg_idx = Some(i);
                line_in_msg = abs_line - cumulative;
                break;
            }
            cumulative += entry.len();
        }

        let msg_idx = found_msg_idx?;
        if line_in_msg >= entries[msg_idx].len() {
            return None;
        }

        // Scan spans to find which image tag the column falls within.
        let line = &entries[msg_idx][line_in_msg];
        let click_col = col.saturating_sub(inner_left) as usize;
        let mut span_offset: usize = 0;
        for span in &line.spans {
            let content = span.content.as_ref();
            let span_width = content.chars().count();
            // Gallery format: "🖼 Image #N"
            if let Some(rest) = content.strip_prefix("\u{1f5bc} Image #") {
                if click_col >= span_offset && click_col < span_offset + span_width {
                    if let Ok(n) = rest.parse::<usize>() {
                        return Some(super::ImageTagHit { image_number: n });
                    }
                }
            }
            // Legacy format: "[Image #N]"
            if content.starts_with("[Image #")
                && content.ends_with(']')
                && click_col >= span_offset
                && click_col < span_offset + span_width
            {
                let num_str = &content[8..content.len() - 1];
                if let Ok(n) = num_str.parse::<usize>() {
                    return Some(super::ImageTagHit { image_number: n });
                }
            }
            span_offset += span_width;
        }
        None
    }

    /// Handle bracketed paste — insert text at cursor in bulk,
    /// or show preview modal for large pastes.
    pub(super) fn handle_paste(&mut self, text: &str) {
        // Skip paste if a permission dialog, search overlay, or paste preview is active.
        if self.pending_permission.is_some()
            || self.search.is_active()
            || self.pending_paste.is_some()
        {
            return;
        }
        if text.lines().count() > crate::widgets::PASTE_PREVIEW_THRESHOLD {
            self.pending_paste = Some(text.to_string());
        } else {
            let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
            self.input_text.insert_str(byte_idx, text);
            self.input_cursor += text.chars().count();
            self.update_ghost_text();
        }
    }

    /// Handle key events when the paste preview modal is active.
    pub(super) fn handle_paste_preview_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                // Confirm paste — insert at cursor.
                if let Some(text) = self.pending_paste.take() {
                    let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                    self.input_text.insert_str(byte_idx, &text);
                    self.input_cursor += text.chars().count();
                    self.update_ghost_text();
                }
            }
            KeyCode::Esc => {
                // Cancel paste.
                self.pending_paste = None;
            }
            _ => {} // Ignore other keys while preview is active.
        }
    }

    /// Handle key events when the model picker overlay is active.
    pub(super) async fn handle_model_picker_key(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => self.model_picker.close(),
            (_, KeyCode::Up) => self.model_picker.select_prev(),
            (_, KeyCode::Down) => self.model_picker.select_next(),
            (_, KeyCode::Left) => self.model_picker.effort_prev(),
            (_, KeyCode::Right) => self.model_picker.effort_next(),
            (_, KeyCode::Enter) => {
                if let Some((model_id, _effort)) = self.model_picker.confirm() {
                    // Send model change as slash command to core.
                    let _ = self
                        .ui_tx
                        .send(crate::events::UiEvent::SlashCommand {
                            name: "model".to_string(),
                            args: model_id.clone(),
                        })
                        .await;
                    self.notifications.push(Notification::new(
                        format!("Switching to {model_id}"),
                        crate::widgets::notification::NotificationLevel::Info,
                    ));
                }
            }
            (_, KeyCode::Char(c)) => self.model_picker.push_filter_char(c),
            (_, KeyCode::Backspace) => self.model_picker.pop_filter_char(),
            _ => {}
        }
    }

    /// Handle key events when the session browser overlay is active.
    pub(super) async fn handle_session_browser_key(&mut self, key: KeyEvent) {
        use crate::widgets::session_browser::SessionBrowserMode;

        match &self.session_browser.mode() {
            SessionBrowserMode::Browse => match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => self.session_browser.cancel(),
                (_, KeyCode::Up) => self.session_browser.select_prev(),
                (_, KeyCode::Down) => self.session_browser.select_next(),
                (_, KeyCode::Char('r')) => self.session_browser.start_rename(),
                (_, KeyCode::Enter) => {
                    if let Some(session_id) = self.session_browser.confirm_resume() {
                        let _ = self
                            .ui_tx
                            .send(crate::events::UiEvent::SlashCommand {
                                name: "resume".to_string(),
                                args: session_id.clone(),
                            })
                            .await;
                        self.notifications.push(Notification::new(
                            format!(
                                "Resuming session {}",
                                &session_id[..8.min(session_id.len())]
                            ),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                    }
                }
                _ => {}
            },
            SessionBrowserMode::Rename => match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => self.session_browser.cancel(),
                (_, KeyCode::Enter) => {
                    if let Some((session_id, new_name)) = self.session_browser.confirm_rename() {
                        self.notifications.push(Notification::new(
                            format!("Renamed session to \"{new_name}\""),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                        // Send rename command to core for persistence.
                        let _ = self
                            .ui_tx
                            .send(crate::events::UiEvent::SlashCommand {
                                name: "rename-session".to_string(),
                                args: format!("{session_id} {new_name}"),
                            })
                            .await;
                    }
                }
                (_, KeyCode::Char(c)) => self.session_browser.push_rename_char(c),
                (_, KeyCode::Backspace) => self.session_browser.pop_rename_char(),
                _ => {}
            },
            SessionBrowserMode::Confirm => match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => self.session_browser.cancel(),
                (_, KeyCode::Enter) => {
                    // Future: handle confirm action (delete, export).
                    self.session_browser.cancel();
                }
                _ => {}
            },
        }
    }

    /// Public entry for the CLI `--resume` flag — opens the picker
    /// immediately on TUI startup (no slash command required).
    pub fn open_session_browser_on_startup(&mut self) {
        self.open_session_browser();
    }

    /// Open the session browser with sessions loaded from disk.
    pub(super) fn open_session_browser(&mut self) {
        // Load sessions from the oxicode-session crate.
        let summaries = oxicode_session::list_sessions(None).unwrap_or_default();
        // Exclude the session we're currently inside — resuming it would be a
        // no-op and the picker should default-highlight a meaningful choice.
        let current_id = self.state_rx.borrow().session_id.clone();
        let entries: Vec<SessionEntry> = summaries
            .into_iter()
            .filter(|s| s.id != current_id)
            .map(|s| {
                let title = s.title.clone().or_else(|| s.preview.clone()).unwrap_or_else(
                    || format!("[{}]", &s.id[..8.min(s.id.len())]),
                );
                let last_updated = format_relative_time(s.updated_at);
                SessionEntry {
                    id: s.id,
                    title,
                    last_updated,
                    message_count: s.message_count,
                    cost_usd: 0.0, // Cost tracking not yet wired to sessions.
                }
            })
            .collect();
        self.session_browser.open(entries);
    }

    /// Handle key events when the rewind overlay is visible.
    pub(super) async fn handle_rewind_key(&mut self, key: KeyEvent) {
        use crate::widgets::rewind_overlay::RewindOverlayMode;

        match self.rewind_overlay.mode() {
            RewindOverlayMode::Selecting => match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => self.rewind_overlay.cancel(),
                // List is rendered reversed (newest at top), so Up moves toward
                // newer turns (higher idx) and Down toward older (lower idx).
                (_, KeyCode::Up) => self.rewind_overlay.select_next(),
                (_, KeyCode::Down) => self.rewind_overlay.select_prev(),
                (_, KeyCode::Enter) => self.rewind_overlay.begin_confirm(),
                _ => {}
            },
            RewindOverlayMode::Confirming => match (key.modifiers, key.code) {
                (_, KeyCode::Char('y') | KeyCode::Char('Y')) => {
                    if let Some(turns) = self.rewind_overlay.confirm_rewind() {
                        let _ = self
                            .ui_tx
                            .send(crate::events::UiEvent::SlashCommand {
                                name: "rewind".to_string(),
                                args: turns.to_string(),
                            })
                            .await;
                        self.notifications.push(Notification::new(
                            format!("Rewinding {turns} turn(s)..."),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                        // Reset scroll to bottom after rewind.
                        self.auto_scroll = true;
                    }
                }
                (_, KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc) => {
                    self.rewind_overlay.cancel();
                }
                _ => {}
            },
        }
    }

    /// Open the rewind overlay, populated from current messages.
    pub(super) fn open_rewind_overlay(&mut self) {
        let state = self.state_rx.borrow();
        if state.is_streaming {
            drop(state);
            self.notifications.push(Notification::new(
                "Cannot rewind while streaming.".to_string(),
                crate::widgets::notification::NotificationLevel::Warning,
            ));
            return;
        }
        let messages = state.messages.clone();
        drop(state);
        if messages.is_empty() {
            self.notifications.push(Notification::new(
                "No messages to rewind.".to_string(),
                crate::widgets::notification::NotificationLevel::Warning,
            ));
            return;
        }
        self.rewind_overlay.open(&messages);
    }

    /// Handle key events when the stats dashboard is visible.
    pub(super) fn handle_stats_key(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => self.stats_dashboard.cancel(),
            (_, KeyCode::Left) => self.stats_dashboard.prev_tab(),
            (_, KeyCode::Right) => self.stats_dashboard.next_tab(),
            (_, KeyCode::Tab) => self.stats_dashboard.next_tab(),
            _ => {}
        }
    }

    /// Handle keyboard input for the diff viewer overlay.
    pub(super) fn handle_diff_viewer_key(&mut self, key: KeyEvent) {
        use crate::widgets::diff_viewer::DiffPane;

        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) | (_, KeyCode::Char('q')) => self.diff_viewer.close(),
            (_, KeyCode::Tab) | (_, KeyCode::Left) | (_, KeyCode::Right) => {
                self.diff_viewer.toggle_pane();
            }
            (_, KeyCode::Up) => {
                if self.diff_viewer.active_pane == DiffPane::FileList {
                    self.diff_viewer.prev_file();
                } else {
                    self.diff_viewer.scroll_up(3);
                }
            }
            (_, KeyCode::Down) => {
                if self.diff_viewer.active_pane == DiffPane::FileList {
                    self.diff_viewer.next_file();
                } else {
                    // Estimate visible height from last draw area.
                    let visible = self.message_area.height.saturating_sub(6);
                    self.diff_viewer.scroll_down(3, visible);
                }
            }
            (_, KeyCode::PageUp) => self.diff_viewer.scroll_up(10),
            (_, KeyCode::PageDown) => {
                let visible = self.message_area.height.saturating_sub(6);
                self.diff_viewer.scroll_down(10, visible);
            }
            (_, KeyCode::Char(' ')) => self.diff_viewer.toggle_collapse(),
            _ => {}
        }
    }

    /// Execute a keybinding action.
    #[allow(clippy::too_many_lines)]
    pub(super) async fn execute_keybinding_action(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.handle_ctrl_c().await;
            }
            Action::Submit => {
                self.submit_input().await;
            }
            Action::ToggleVim => {
                let new_state = !self.vim.enabled;
                self.vim.set_enabled(new_state);
            }
            Action::TogglePanel => {
                self.split_pane.toggle_right();
            }
            Action::ScrollUp => {
                self.scroll_up_by(1);
            }
            Action::ScrollDown => {
                self.scroll_down_by(1);
            }
            Action::PageUp => {
                self.scroll_up_by(20);
            }
            Action::PageDown => {
                self.scroll_down_by(20);
            }
            Action::ClearLine => {
                self.input_text.clear();
                self.input_cursor = 0;
            }
            Action::DeleteWordBackward => {
                let prev = vim_mode::prev_word_pos(&self.input_text, self.input_cursor);
                let start_byte = char_to_byte_index(&self.input_text, prev);
                let end_byte = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.replace_range(start_byte..end_byte, "");
                self.input_cursor = prev;
            }
            Action::DeleteToLineStart => {
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.replace_range(..byte_idx, "");
                self.input_cursor = 0;
            }
            Action::OpenSearch => {
                self.search.activate();
            }
            Action::ToggleShortcuts => {
                self.shortcuts.toggle();
            }
            Action::ToggleThinking => {
                // Toggle thinking expansion for the last assistant message with thinking.
                let state = self.state_rx.borrow();
                let msg_count = state.messages.len();
                drop(state);
                // Find last assistant message with thinking blocks (iterate backwards).
                let state = self.state_rx.borrow();
                for i in (0..msg_count).rev() {
                    if state.messages[i].role == oxicode_common::Role::Assistant
                        && state.messages[i]
                            .content
                            .iter()
                            .any(|b| matches!(b, oxicode_common::ContentBlock::Thinking { .. }))
                    {
                        drop(state);
                        self.message_cache
                            .toggle_thinking(&self.state_rx.borrow().messages, i);
                        break;
                    }
                }
            }
            Action::CycleOutputStyle => {
                // Handled via slash command, not inline.
            }
            Action::CursorHome => {
                self.input_cursor = 0;
            }
            Action::CursorEnd => {
                self.input_cursor = self.input_text.chars().count();
            }
            Action::CursorWordLeft => {
                self.input_cursor = vim_mode::prev_word_pos(&self.input_text, self.input_cursor);
            }
            Action::CursorWordRight => {
                self.input_cursor = vim_mode::next_word_pos(&self.input_text, self.input_cursor);
            }
            Action::InsertNewline => {
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert(byte_idx, '\n');
                self.input_cursor += 1;
            }
            Action::HistoryPrev => {
                self.history_prev();
            }
            Action::HistoryNext => {
                self.history_next();
            }
            Action::HistorySearch => {
                self.open_history_search();
            }
            Action::OpenModelPicker => {
                let state = self.state_rx.borrow();
                let current_model = state.current_model.clone();
                drop(state);
                self.model_picker.open(&current_model);
            }
            Action::OpenSessionBrowser => {
                self.open_session_browser();
            }
        }
    }
}
