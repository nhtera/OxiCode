/// Streaming: submit_input, history nav, ghost text, autocomplete.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::message_queue::QueuedMessage;
use crate::widgets::HistorySearchState;

use super::utils::char_to_byte_index;

impl super::App {
    /// Submit the current input text.
    pub(super) async fn submit_input(&mut self) {
        // Queue input when a turn is active — don't block.
        if self.is_turn_active {
            if !self.input_text.is_empty() || !self.pending_images.is_empty() {
                let text = std::mem::take(&mut self.input_text);
                let images = std::mem::take(&mut self.pending_images);
                self.input_cursor = 0;
                self.history.add(&text, None);
                self.message_queue.enqueue(QueuedMessage::new(text, images));
            }
            return;
        }

        if !self.input_text.is_empty() || !self.pending_images.is_empty() {
            let text = std::mem::take(&mut self.input_text);
            self.input_cursor = 0;
            self.history_index = None;

            // Save to history (dedup consecutive duplicates).
            self.history.add(&text, None);

            if let Some(trimmed) = text.strip_prefix('/') {
                let trimmed = trimmed.trim();
                // Handle /quit inline — set should_quit and notify engine.
                if trimmed == "quit" || trimmed == "exit" {
                    let _ = self.ui_tx.send(crate::events::UiEvent::Quit).await;
                    self.should_quit = true;
                    return;
                }
                // Handle /vim toggle inline.
                if trimmed == "vim" {
                    let new_state = !self.vim.enabled;
                    self.vim.set_enabled(new_state);
                    return;
                }
                // Handle /sessions (and /session) inline — open browser overlay.
                // `/resume` and `/continue` with no args share the same UX.
                if matches!(
                    trimmed,
                    "sessions" | "session" | "resume" | "continue"
                ) {
                    self.open_session_browser();
                    return;
                }
                // Handle /model with no args — open model picker overlay.
                if trimmed == "model" {
                    let state = self.state_rx.borrow();
                    let current_model = state.current_model.clone();
                    drop(state);
                    self.model_picker.open(&current_model);
                    return;
                }
                // Handle /rewind with no args — open interactive rewind overlay.
                if trimmed == "rewind" {
                    self.open_rewind_overlay();
                    return;
                }
                // Handle /stats, /cost, /dashboard — open stats dashboard overlay.
                if trimmed == "stats" || trimmed == "cost" || trimmed == "dashboard" {
                    self.stats_dashboard.open();
                    return;
                }
                // Handle /diff, /review — open interactive diff viewer overlay.
                if trimmed == "diff" || trimmed == "review" {
                    let state = self.state_rx.borrow();
                    let cwd = state.cwd.clone();
                    drop(state);
                    self.diff_viewer.open(&cwd);
                    return;
                }
                let (name, args) = match trimmed.split_once(char::is_whitespace) {
                    Some((n, a)) => (n.to_string(), a.trim().to_string()),
                    None => (trimmed.to_string(), String::new()),
                };
                let _ = self
                    .ui_tx
                    .send(crate::events::UiEvent::SlashCommand { name, args })
                    .await;
            } else {
                // Keep [Image #N] tags in text — they render inline in the message
                // and the API (Claude) can see both images and text references.
                // Retain image paths for click-to-open in message view.
                // Use global image counter from cache so numbering is session-wide.
                if !self.pending_images.is_empty() {
                    let base = self.message_cache.image_counter();
                    for (i, img) in self.pending_images.iter().enumerate() {
                        self.sent_image_paths.insert(base + 1 + i, img.path.clone());
                    }
                }
                let images = std::mem::take(&mut self.pending_images);
                let _ = self
                    .ui_tx
                    .send(crate::events::UiEvent::UserInput {
                        text,
                        images,
                    })
                    .await;
            }
        }
    }

    /// Navigate to previous history entry.
    pub(super) fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_index {
            None => {
                self.history_saved_input = self.input_text.clone();
                self.history.len() - 1
            }
            Some(0) => return,
            Some(i) => i - 1,
        };
        self.history_index = Some(idx);
        if let Some(content) = self.history.get(idx) {
            self.input_text = content.to_string();
            self.input_cursor = self.input_text.chars().count();
        }
    }

    /// Navigate to next history entry.
    pub(super) fn history_next(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        if idx + 1 >= self.history.len() {
            // Restore saved input.
            self.history_index = None;
            self.input_text = std::mem::take(&mut self.history_saved_input);
            self.input_cursor = self.input_text.chars().count();
        } else {
            self.history_index = Some(idx + 1);
            if let Some(content) = self.history.get(idx + 1) {
                self.input_text = content.to_string();
                self.input_cursor = self.input_text.chars().count();
            }
        }
    }

    /// Recompute ghost text completion based on current input.
    pub(super) fn update_ghost_text(&mut self) {
        self.ghost_text =
            crate::ghost_completion::complete(&self.input_text, &self.slash_commands);
    }

    /// Accept the current ghost text completion. Returns true if accepted.
    pub(super) fn accept_ghost_text(&mut self) -> bool {
        if let Some(ghost) = self.ghost_text.take() {
            self.input_text.push_str(&ghost);
            self.input_cursor = self.input_text.chars().count();
            true
        } else {
            false
        }
    }

    /// Open the reverse history search overlay (Ctrl+R).
    pub(super) fn open_history_search(&mut self) {
        let mut state = HistorySearchState::new(self.input_text.clone(), self.input_cursor);
        // Initialize with all entries (newest-first).
        let items: Vec<(usize, String)> = self
            .history
            .search("")
            .into_iter()
            .map(|i| (i, self.history.get(i).unwrap_or_default().to_string()))
            .collect();
        state.update_results(items);
        self.history_search = Some(state);
    }

    /// Handle key events in the history search overlay.
    pub(super) fn handle_history_search_key(&mut self, key: KeyEvent) {
        let Some(ref mut search) = self.history_search else {
            return;
        };
        match (key.modifiers, key.code) {
            // Enter: accept selected match.
            (_, KeyCode::Enter) => {
                if let Some(content) = search.selected_content() {
                    self.input_text = content.to_string();
                    self.input_cursor = self.input_text.chars().count();
                }
                self.history_search = None;
            }
            // Esc: cancel search, restore original input.
            (_, KeyCode::Esc) => {
                self.input_text = search.saved_input.clone();
                self.input_cursor = search.saved_cursor;
                self.history_search = None;
            }
            // Ctrl+R again: cycle to next match.
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                search.select_next();
                // Preview the selected match in the input.
                if let Some(content) = search.selected_content() {
                    self.input_text = content.to_string();
                    self.input_cursor = self.input_text.chars().count();
                }
            }
            // Up/Down: navigate results.
            (_, KeyCode::Up) => {
                search.select_prev();
                if let Some(content) = search.selected_content() {
                    self.input_text = content.to_string();
                    self.input_cursor = self.input_text.chars().count();
                }
            }
            (_, KeyCode::Down) => {
                search.select_next();
                if let Some(content) = search.selected_content() {
                    self.input_text = content.to_string();
                    self.input_cursor = self.input_text.chars().count();
                }
            }
            // Backspace: delete from query.
            (_, KeyCode::Backspace) => {
                search.pop_char();
                self.refresh_history_search();
            }
            // Typing: add to query.
            (_, KeyCode::Char(c)) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                search.push_char(c);
                self.refresh_history_search();
            }
            _ => {}
        }
    }

    /// Refresh history search results based on current query.
    pub(super) fn refresh_history_search(&mut self) {
        let Some(ref mut search) = self.history_search else {
            return;
        };
        let query = search.query.clone();
        let items: Vec<(usize, String)> = self
            .history
            .search(&query)
            .into_iter()
            .map(|i| (i, self.history.get(i).unwrap_or_default().to_string()))
            .collect();
        search.update_results(items);
        // Preview the first match.
        if let Some(content) = search.selected_content() {
            self.input_text = content.to_string();
            self.input_cursor = self.input_text.chars().count();
        }
    }

    /// Activate autocomplete dropdown with all commands (or filtered).
    pub(super) fn activate_autocomplete(&mut self) {
        use crate::widgets::command_autocomplete::filter_commands;
        let query = self.input_text.strip_prefix('/').unwrap_or("");
        let filtered = filter_commands(&self.slash_commands, query);
        if filtered.is_empty() {
            self.autocomplete.deactivate();
        } else {
            self.autocomplete.activate(filtered);
        }
    }

    /// Handle key events when the autocomplete dropdown is active.
    pub(super) fn handle_autocomplete_key(&mut self, key: KeyEvent) {
        use crate::widgets::command_autocomplete::filter_commands;

        match key.code {
            KeyCode::Up => {
                self.autocomplete.select_prev();
            }
            KeyCode::Down => {
                self.autocomplete.select_next();
            }
            KeyCode::Enter | KeyCode::Tab => {
                // Select the highlighted command — fill input, don't auto-submit.
                if let Some(&cmd_idx) = self.autocomplete.filtered.get(self.autocomplete.selected) {
                    let cmd_name = &self.slash_commands[cmd_idx].name;
                    self.input_text = format!("/{cmd_name}");
                    self.input_cursor = self.input_text.chars().count();
                    self.ghost_text = None;
                }
                self.autocomplete.deactivate();
            }
            KeyCode::Esc => {
                self.autocomplete.deactivate();
            }
            KeyCode::Backspace => {
                // Remove character from input.
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let start = char_to_byte_index(&self.input_text, self.input_cursor);
                    let end = char_to_byte_index(&self.input_text, self.input_cursor + 1);
                    self.input_text.replace_range(start..end, "");
                }
                // If input no longer starts with '/', deactivate.
                if !self.input_text.starts_with('/') || self.input_text.is_empty() {
                    self.autocomplete.deactivate();
                } else {
                    // Re-filter with updated query.
                    let query = self.input_text.strip_prefix('/').unwrap_or("");
                    let filtered = filter_commands(&self.slash_commands, query);
                    if filtered.is_empty() {
                        self.autocomplete.deactivate();
                    } else {
                        self.autocomplete.filtered = filtered;
                        self.autocomplete.selected = self
                            .autocomplete
                            .selected
                            .min(self.autocomplete.filtered.len().saturating_sub(1));
                    }
                }
                self.update_ghost_text();
            }
            KeyCode::Char(' ') => {
                // Space after command → deactivate dropdown, keep input.
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert(byte_idx, ' ');
                self.input_cursor += 1;
                self.autocomplete.deactivate();
                self.ghost_text = None;
            }
            KeyCode::Char(c) => {
                // Append character, re-filter.
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert(byte_idx, c);
                self.input_cursor += 1;
                let query = self.input_text.strip_prefix('/').unwrap_or("");
                let filtered = filter_commands(&self.slash_commands, query);
                if filtered.is_empty() {
                    self.autocomplete.deactivate();
                } else {
                    self.autocomplete.filtered = filtered;
                    self.autocomplete.selected = self
                        .autocomplete
                        .selected
                        .min(self.autocomplete.filtered.len().saturating_sub(1));
                }
                self.update_ghost_text();
            }
            _ => {}
        }
    }
}
