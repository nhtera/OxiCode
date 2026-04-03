use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use oxicode_state::{AppState, StateStore};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use tokio::sync::{mpsc, watch};

use crate::events::{CoreEvent, UiEvent};
use crate::keybindings::{Action, KeybindingRegistry};
use crate::vim_mode::{self, VimAction, VimState};
use crate::widgets::{
    ActiveToolInfo, AgentInfo, AgentPanel, InputBox, MessageView, Notification,
    NotificationWidget, PermissionDialog, SearchBar, SearchOverlay, ShortcutsPanel,
    ShortcutsState, SplitPane, StatusBar, TaskInfo, TaskPanel,
};

/// A tool call in progress (between ToolUseStart and ToolResult events).
struct ActiveToolCall {
    id: String,
    name: String,
    input_summary: String,
    /// `Some((content, is_error))` when the tool has completed.
    result: Option<(String, bool)>,
}

/// A pending permission request awaiting user response.
struct PendingPermission {
    tool_name: String,
    input_summary: String,
    prompt: String,
    selected: usize,
    reply_tx: tokio::sync::oneshot::Sender<oxicode_common::PermissionResponse>,
}

/// Main TUI application.
pub struct App {
    state_rx: watch::Receiver<AppState>,
    ui_tx: mpsc::Sender<UiEvent>,
    core_rx: mpsc::Receiver<CoreEvent>,
    input_text: String,
    /// Cursor position as character index (not byte index).
    input_cursor: usize,
    scroll_offset: u16,
    streaming_text: String,
    should_quit: bool,
    /// Manages left/right split layout and ratio.
    split_pane: SplitPane,
    /// Toast notifications rendered as an overlay.
    notifications: Vec<Notification>,
    /// Tool calls in progress during the current turn.
    active_tools: Vec<ActiveToolCall>,
    /// Permission dialog state (blocks input while active).
    pending_permission: Option<PendingPermission>,
    /// Vim mode state machine.
    vim: VimState,
    /// Keybinding registry.
    keybindings: KeybindingRegistry,
    /// Search overlay state.
    search: SearchOverlay,
    /// Shortcuts panel visibility.
    shortcuts: ShortcutsState,
    /// Command history (most recent last).
    history: Vec<String>,
    /// Current position in history navigation (-1 = not navigating).
    history_index: Option<usize>,
    /// Saved input before history navigation started.
    history_saved_input: String,
}

impl App {
    pub fn new(
        state_store: &Arc<StateStore>,
        ui_tx: mpsc::Sender<UiEvent>,
        core_rx: mpsc::Receiver<CoreEvent>,
    ) -> Self {
        let keybindings = KeybindingRegistry::with_defaults();

        Self {
            state_rx: state_store.subscribe(),
            ui_tx,
            core_rx,
            input_text: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            streaming_text: String::new(),
            should_quit: false,
            split_pane: SplitPane::new(),
            notifications: Vec::new(),
            active_tools: Vec::new(),
            pending_permission: None,
            vim: VimState::new(false),
            keybindings,
            search: SearchOverlay::new(),
            shortcuts: ShortcutsState::new(),
            history: Vec::new(),
            history_index: None,
            history_saved_input: String::new(),
        }
    }

    /// Enable vim mode at runtime.
    pub fn set_vim_mode(&mut self, enabled: bool) {
        self.vim.set_enabled(enabled);
    }

    /// Load user keybindings from a TOML file.
    pub fn load_keybindings(&mut self, path: &std::path::Path) {
        self.keybindings.load_from_file(path);
    }

    /// Run the TUI event loop.
    pub async fn run(&mut self) -> io::Result<()> {
        // Setup terminal
        terminal::enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let result = self.event_loop(&mut terminal).await;

        // Restore terminal (always, even on error)
        let _ = terminal::disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        // C1 FIX: Single dedicated thread for all crossterm I/O.
        // Sends terminal events over mpsc channel to avoid race conditions.
        let (term_tx, mut term_rx) = mpsc::channel::<Event>(32);
        tokio::task::spawn_blocking(move || loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(ev) = event::read() {
                    if term_tx.blocking_send(ev).is_err() {
                        break;
                    }
                }
            }
        });

        loop {
            self.draw(terminal)?;

            if self.should_quit {
                break;
            }

            tokio::select! {
                Some(ev) = term_rx.recv() => {
                    if let Event::Key(key) = ev {
                        self.handle_key(key).await;
                    }
                }
                Some(core_event) = self.core_rx.recv() => {
                    self.handle_core_event(core_event);
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
        let state = self.state_rx.borrow().clone();
        let vim_enabled = self.vim.enabled;
        let vim_badge = if vim_enabled {
            self.vim.mode.badge()
        } else {
            ""
        };
        let search_active = self.search.is_active();
        let shortcuts_visible = self.shortcuts.is_visible();
        let command_buf = if vim_enabled
            && self.vim.mode == crate::vim_mode::Mode::Command
        {
            Some(self.vim.command_buffer().to_string())
        } else {
            None
        };

        terminal.draw(|frame| {
            let input_height = if search_active { 5u16 } else { 3u16 };
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),          // Status bar
                    Constraint::Min(5),             // Message view
                    Constraint::Length(input_height), // Input box (+ search bar)
                ])
                .split(frame.area());

            // Status bar with vim badge.
            let status_bar =
                StatusBar::new(&state.current_model, &state.total_usage, state.is_streaming)
                    .with_vim_badge(vim_badge);
            frame.render_widget(status_bar, chunks[0]);

            // Content area — optionally split into left (messages) + right (agents/tasks)
            let content_area = chunks[1];
            let (left_area, right_area) = self.split_pane.split(content_area);

            // Message view (left pane, or full area when right pane is hidden)
            let streaming = if state.is_streaming && !self.streaming_text.is_empty() {
                Some(self.streaming_text.as_str())
            } else {
                None
            };
            // Build active tools snapshot for streaming display.
            let active_tool_info: Vec<ActiveToolInfo<'_>> = self
                .active_tools
                .iter()
                .map(|t| ActiveToolInfo {
                    name: &t.name,
                    input_summary: &t.input_summary,
                    result: t.result.as_ref().map(|(c, e)| (c.as_str(), *e)),
                })
                .collect();
            let message_view =
                MessageView::new(&state.messages, streaming, &active_tool_info, self.scroll_offset);
            frame.render_widget(message_view, left_area);

            // Right pane: agent panel (top) + task panel (bottom)
            if let Some(right) = right_area {
                let agent_infos: Vec<AgentInfo> = state
                    .active_agents
                    .iter()
                    .map(|a| AgentInfo {
                        name: a.name.clone(),
                        status: a.status.clone(),
                        started_at: a.started_at.clone(),
                        duration: String::new(),
                    })
                    .collect();

                let task_infos: Vec<TaskInfo> = state
                    .background_tasks
                    .iter()
                    .map(|t| TaskInfo {
                        id: t.id.clone(),
                        task_type: t.task_type.clone(),
                        status: t.status.clone(),
                        command_preview: t.command_preview.clone(),
                    })
                    .collect();

                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(right);

                frame.render_widget(AgentPanel::new(&agent_infos), right_chunks[0]);
                frame.render_widget(TaskPanel::new(&task_infos), right_chunks[1]);
            }

            // Notification toast overlay (drawn on top of content area)
            if !self.notifications.is_empty() {
                let notif_widget = NotificationWidget::new(&self.notifications).with_max_visible(3);
                frame.render_widget(notif_widget, content_area);
            }

            // Shortcuts panel overlay (drawn on top of content area).
            if shortcuts_visible {
                frame.render_widget(ShortcutsPanel, content_area);
            }

            // Permission dialog overlay (drawn on top of everything).
            if let Some(ref perm) = self.pending_permission {
                let dialog = PermissionDialog::new(
                    &perm.tool_name,
                    &perm.input_summary,
                    &perm.prompt,
                )
                .with_selected(perm.selected);
                frame.render_widget(dialog, content_area);
            }

            // Input box area — may include search bar below.
            if search_active {
                let input_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Length(2)])
                    .split(chunks[2]);

                let byte_cursor = char_to_byte_index(&self.input_text, self.input_cursor);
                let mut input = InputBox::new(&self.input_text, byte_cursor, true);
                if vim_enabled {
                    input = input.with_vim_badge(vim_badge);
                }
                frame.render_widget(input, input_chunks[0]);

                let search_bar = SearchBar::new(&self.search);
                frame.render_widget(search_bar, input_chunks[1]);
            } else {
                let byte_cursor = char_to_byte_index(&self.input_text, self.input_cursor);
                let mut input = InputBox::new(&self.input_text, byte_cursor, true);
                if vim_enabled {
                    input = input.with_vim_badge(vim_badge);
                }
                if let Some(ref cmd) = command_buf {
                    input = input.with_command_line(cmd);
                }
                frame.render_widget(input, chunks[2]);
            }
        })?;

        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        // Permission dialog takes priority over all other input.
        if self.pending_permission.is_some() {
            self.handle_permission_key(key).await;
            return;
        }

        // Search overlay captures keys when active.
        if self.search.is_active() {
            self.handle_search_key(key);
            return;
        }

        // Shortcuts panel: any key hides it.
        if self.shortcuts.is_visible() {
            self.shortcuts.hide();
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
                let _ = self.ui_tx.send(UiEvent::Quit).await;
                self.should_quit = true;
            }
            (_, KeyCode::Enter) => {
                self.submit_input().await;
            }
            // H3 FIX: cursor operates on char count, insert at byte offset
            (_, KeyCode::Char(c)) => {
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert(byte_idx, c);
                self.input_cursor += 1;
            }
            (_, KeyCode::Backspace) => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let start = char_to_byte_index(&self.input_text, self.input_cursor);
                    let end = char_to_byte_index(&self.input_text, self.input_cursor + 1);
                    self.input_text.replace_range(start..end, "");
                }
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
                }
            }
            (_, KeyCode::Up) => {
                self.history_prev();
            }
            (_, KeyCode::Down) => {
                self.history_next();
            }
            (_, KeyCode::Home) => {
                self.input_cursor = 0;
            }
            (_, KeyCode::End) => {
                self.input_cursor = self.input_text.chars().count();
            }
            // Tab toggles the right split pane (agents + tasks panel).
            (_, KeyCode::Tab) => {
                self.split_pane.toggle_right();
            }
            _ => {}
        }
    }

    /// Handle vim mode key dispatch.
    #[allow(clippy::too_many_lines)]
    async fn handle_vim_key(&mut self, key: KeyEvent) {
        let text_len = self.input_text.chars().count();
        let action = self.vim.handle_key(key, text_len);

        match action {
            VimAction::Passthrough(k) => {
                // Let Ctrl+C through for quit.
                if k.modifiers == KeyModifiers::CONTROL && k.code == KeyCode::Char('c') {
                    let _ = self.ui_tx.send(UiEvent::Quit).await;
                    self.should_quit = true;
                }
            }
            VimAction::Noop
            | VimAction::SwitchToInsert
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
            VimAction::MoveWordForward => {
                self.input_cursor = vim_mode::next_word_pos(&self.input_text, self.input_cursor);
            }
            VimAction::MoveWordBackward => {
                self.input_cursor = vim_mode::prev_word_pos(&self.input_text, self.input_cursor);
            }
            VimAction::MoveWordEnd => {
                self.input_cursor = vim_mode::word_end_pos(&self.input_text, self.input_cursor);
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
                let _ = self.ui_tx.send(UiEvent::Quit).await;
                self.should_quit = true;
            }
            VimAction::EnterSearch => {
                self.search.activate();
            }
        }
    }

    /// Handle search overlay key events.
    fn handle_search_key(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.search.deactivate();
            }
            (_, KeyCode::Enter) => {
                // Close search, keep results.
                self.search.deactivate();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => {
                self.search.next_match();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
                self.search.prev_match();
            }
            (_, KeyCode::Char(c)) => {
                self.search.push_char(c);
            }
            (_, KeyCode::Backspace) => {
                self.search.pop_char();
            }
            _ => {}
        }
    }

    /// Execute a keybinding action.
    async fn execute_keybinding_action(&mut self, action: Action) {
        match action {
            Action::Quit => {
                let _ = self.ui_tx.send(UiEvent::Quit).await;
                self.should_quit = true;
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
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            Action::ScrollDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            Action::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(20);
            }
            Action::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(20);
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
                // Future: Ctrl+R search overlay. For now, just navigate history.
                self.history_prev();
            }
        }
    }

    /// Submit the current input text.
    async fn submit_input(&mut self) {
        if !self.input_text.is_empty() {
            let text = std::mem::take(&mut self.input_text);
            self.input_cursor = 0;
            self.history_index = None;

            // Save to history (dedup consecutive duplicates).
            if self.history.last().map_or(true, |last| *last != text) {
                self.history.push(text.clone());
            }

            if let Some(trimmed) = text.strip_prefix('/') {
                let trimmed = trimmed.trim();
                // Handle /vim toggle inline.
                if trimmed == "vim" {
                    let new_state = !self.vim.enabled;
                    self.vim.set_enabled(new_state);
                    return;
                }
                let (name, args) = match trimmed.split_once(char::is_whitespace) {
                    Some((n, a)) => (n.to_string(), a.trim().to_string()),
                    None => (trimmed.to_string(), String::new()),
                };
                let _ = self.ui_tx.send(UiEvent::SlashCommand { name, args }).await;
            } else {
                let _ = self.ui_tx.send(UiEvent::UserInput(text)).await;
            }
        }
    }

    /// Navigate to previous history entry.
    fn history_prev(&mut self) {
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
        self.input_text = self.history[idx].clone();
        self.input_cursor = self.input_text.chars().count();
    }

    /// Navigate to next history entry.
    fn history_next(&mut self) {
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
            self.input_text = self.history[idx + 1].clone();
            self.input_cursor = self.input_text.chars().count();
        }
    }

    /// Handle key events when the permission dialog is active.
    async fn handle_permission_key(&mut self, key: KeyEvent) {
        let Some(ref mut perm) = self.pending_permission else {
            return;
        };
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) => {
                perm.selected = perm.selected.saturating_sub(1);
            }
            (_, KeyCode::Down) => {
                perm.selected = (perm.selected + 1).min(1);
            }
            (_, KeyCode::Enter) => {
                let response = match perm.selected {
                    0 => oxicode_common::PermissionResponse::AllowOnce,
                    _ => oxicode_common::PermissionResponse::Deny,
                };
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm.reply_tx.send(response);
                }
            }
            (_, KeyCode::Esc) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm.reply_tx.send(oxicode_common::PermissionResponse::Deny);
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm.reply_tx.send(oxicode_common::PermissionResponse::Deny);
                }
                let _ = self.ui_tx.send(UiEvent::Quit).await;
                self.should_quit = true;
            }
            _ => {}
        }
    }

    fn handle_core_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::TextDelta(text) => {
                self.streaming_text.push_str(&text);
                self.scroll_offset = u16::MAX;
            }
            CoreEvent::StreamStart => {
                self.streaming_text.clear();
            }
            CoreEvent::StreamEnd | CoreEvent::MessageComplete => {
                self.streaming_text.clear();
                self.active_tools.clear();
                self.scroll_offset = u16::MAX;
            }
            CoreEvent::Error(msg) => {
                tracing::error!("Core error: {}", msg);
                // Clear streaming state on error (engine may not send StreamEnd).
                self.streaming_text.clear();
                self.active_tools.clear();
            }
            CoreEvent::ToolUseStart { id, name, input } => {
                let summary = summarize_input(&input);
                self.active_tools.push(ActiveToolCall {
                    id,
                    name,
                    input_summary: summary,
                    result: None,
                });
                self.scroll_offset = u16::MAX;
            }
            CoreEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                if let Some(tool) = self.active_tools.iter_mut().find(|t| t.id == tool_use_id) {
                    tool.result = Some((content, is_error));
                }
                self.scroll_offset = u16::MAX;
            }
            CoreEvent::PermissionAsk {
                tool_name,
                input_summary,
                prompt,
                reply_tx,
            } => {
                self.pending_permission = Some(PendingPermission {
                    tool_name,
                    input_summary,
                    prompt,
                    selected: 0,
                    reply_tx,
                });
            }
            CoreEvent::RateLimited { message, attempt, max_retries, retry_in_secs } => {
                let notif_msg = format!(
                    "Rate limited. Retrying in {retry_in_secs:.0}s... ({attempt}/{max_retries})"
                );
                tracing::warn!("{}", message);
                self.notifications.push(Notification::new(
                    notif_msg,
                    crate::widgets::notification::NotificationLevel::RateLimit,
                ));
            }
        }
    }
}

/// Summarize tool input for display (show the most relevant field).
fn summarize_input(input: &serde_json::Value) -> String {
    let raw = if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
        cmd.to_string()
    } else if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
        path.to_string()
    } else if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        format!("{pattern} in {path}")
    } else {
        serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())
    };
    // Truncate to 80 chars.
    if let Some((idx, _)) = raw.char_indices().nth(80) {
        format!("{}...", &raw[..idx])
    } else {
        raw
    }
}

/// Convert a character index to a byte index in a UTF-8 string.
fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_to_byte_index_ascii() {
        assert_eq!(char_to_byte_index("hello", 0), 0);
        assert_eq!(char_to_byte_index("hello", 3), 3);
        assert_eq!(char_to_byte_index("hello", 5), 5);
    }

    #[test]
    fn test_char_to_byte_index_utf8() {
        let s = "héllo"; // é is 2 bytes
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 1); // 'h' at 0, 'é' starts at 1
        assert_eq!(char_to_byte_index(s, 2), 3); // 'l' starts at byte 3
    }

    #[test]
    fn test_char_to_byte_index_emoji() {
        let s = "a😀b";
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 1); // emoji starts at byte 1
        assert_eq!(char_to_byte_index(s, 2), 5); // 'b' starts at byte 5
        assert_eq!(char_to_byte_index(s, 3), 6); // past end
    }
}
