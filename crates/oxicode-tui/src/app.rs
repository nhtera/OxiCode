use std::cell::Cell;
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use oxicode_state::{AppState, StateStore};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Terminal;
use tokio::sync::{mpsc, watch};

use crate::events::{CoreEvent, UiEvent};
use crate::keybindings::{Action, KeybindingRegistry};
use crate::prompt_suggestions::{suggest_prompts, PromptSuggestion};
use crate::streaming_markdown::MarkdownStreamCollector;
use crate::vim_mode::{self, VimAction, VimState};
use crate::vim_text_objects;
use crate::widgets::{
    permission_dialog::RiskLevel, ActiveToolInfo, AgentInfo, AgentPanel, AutocompleteState,
    CommandAutocomplete, HistorySearchState, HistorySearchWidget, InputBox, MessageRenderCache,
    MessageView, ModelPickerState, Notification, NotificationWidget, PastePreview,
    PermissionDialog, SearchBar, SearchOverlay, SessionBrowserState, SessionEntry, ShortcutsState,
    SlashCommandMeta, SplitPane, StatusBar, SuggestionChips, TaskInfo, TaskPanel,
    PASTE_PREVIEW_THRESHOLD,
};

/// A tool call in progress (between ToolUseStart and ToolResult events).
struct ActiveToolCall {
    id: String,
    name: String,
    input_summary: String,
    /// Raw input JSON for tool-specific rendering (e.g., Bash shows command).
    raw_input: serde_json::Value,
    /// When this tool call started (for elapsed time display).
    started_at: std::time::Instant,
    /// `Some((content, is_error))` when the tool has completed.
    result: Option<(String, bool)>,
}

/// Result from `find_image_tag_at()` — identifies an `[Image #N]` span hit.
struct ImageTagHit {
    /// Message index in state.messages.
    msg_index: usize,
    /// Image number (1-based, from `[Image #N]`).
    image_number: usize,
    /// Column start within the line (relative to inner area left edge).
    col_start: usize,
    /// Column end (exclusive) within the line.
    col_end: usize,
}

/// A pending permission request awaiting user response.
struct PendingPermission {
    tool_name: String,
    input_summary: String,
    prompt: String,
    selected: usize,
    /// Risk level derived from tool name and input (drives dialog border color).
    risk_level: RiskLevel,
    /// Tool-specific dialog variant (drives options and content layout).
    kind: crate::widgets::PermissionDialogKind,
    /// When this permission request was created (for countdown timer).
    created_at: Instant,
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
    /// Current message-view scroll offset from top.
    scroll_offset: u16,
    /// When true, keep message view pinned to bottom as new content arrives.
    auto_scroll: bool,
    /// Last computed max scroll offset for the current viewport/content.
    max_scroll_offset: u16,
    streaming_text: String,
    /// Newline-gated markdown stream collector (Codex-rs pattern).
    streaming_collector: MarkdownStreamCollector,
    /// Pre-rendered committed lines from streaming collector.
    streaming_committed_lines: Vec<ratatui::text::Line<'static>>,
    should_quit: bool,
    /// True from StreamStart until MessageComplete — tracks whether a turn is active.
    is_turn_active: bool,
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
    history: oxicode_session::prompt_history::PersistentHistory,
    /// Current position in history navigation (-1 = not navigating).
    history_index: Option<usize>,
    /// Saved input before history navigation started.
    history_saved_input: String,
    /// Reverse history search overlay state (Ctrl+R).
    history_search: Option<HistorySearchState>,
    /// Timestamp of last interrupt (for double Ctrl+C force quit).
    last_interrupt: Option<Instant>,
    /// Per-message render cache — avoids re-parsing markdown for unchanged messages.
    message_cache: MessageRenderCache,
    /// Ghost text completion suffix (shown dimmed after cursor).
    ghost_text: Option<String>,
    /// Large paste text awaiting confirmation (shown in preview modal).
    pending_paste: Option<String>,
    /// Pasted images waiting to be sent with the next message.
    pending_images: Vec<crate::image_paste::PastedImage>,
    /// Image file paths per message index (for click-to-open after sending).
    /// Key = message index in state.messages, Value = image paths in order.
    sent_image_paths: std::collections::HashMap<usize, Vec<std::path::PathBuf>>,
    /// Context-aware follow-up suggestions shown as chips.
    suggestions: Vec<PromptSuggestion>,
    /// Timestamp when the current turn started (for thinking indicator).
    turn_started_at: Option<Instant>,
    /// Slash command metadata for autocomplete dropdown.
    slash_commands: Vec<SlashCommandMeta>,
    /// Pre-built (name, description, category) tuples for help overlay right column.
    help_commands: Vec<(String, String, String)>,
    /// Pre-built keyboard shortcut entries for help overlay left column.
    help_shortcuts: Vec<crate::widgets::shortcuts_overlay::ShortcutEntry>,
    /// Autocomplete dropdown state (active when typing `/...`).
    autocomplete: AutocompleteState,
    /// Model picker overlay state.
    model_picker: ModelPickerState,
    /// Session browser overlay state.
    session_browser: SessionBrowserState,
    /// Cached message area rect from last draw (for scrollbar hit-testing).
    message_area: Rect,
    /// Screen row where a hovered `[Image #N]` tag starts (for underline styling).
    /// Stores (row, col_start, col_end) of the hovered image tag.
    hovered_image_tag: Option<(u16, u16, u16)>,
    /// Frame counter — incremented each draw call, drives spinner animation.
    frame_count: u64,
    /// Session start time for elapsed display in status bar.
    session_start: Instant,
    /// Set when streaming starts, reset on each delta. If >3s since last delta,
    /// spinner turns red (stall detection).
    stall_start: Option<Instant>,
    /// Thinking text accumulated during streaming (from ThinkingDelta events).
    streaming_thinking: String,
    /// Duration of the last completed turn (set on MessageComplete).
    last_turn_duration: Option<Duration>,
}

impl App {
    pub fn new(
        state_store: &Arc<StateStore>,
        ui_tx: mpsc::Sender<UiEvent>,
        core_rx: mpsc::Receiver<CoreEvent>,
        slash_commands: Vec<SlashCommandMeta>,
    ) -> Self {
        let keybindings = KeybindingRegistry::with_defaults();

        // Build help overlay data from slash_commands.
        let help_commands: Vec<(String, String, String)> = slash_commands
            .iter()
            .map(|c| (c.name.clone(), c.description.clone(), c.category.clone()))
            .collect();
        let help_shortcuts = crate::widgets::shortcuts_overlay::default_shortcut_entries();

        Self {
            state_rx: state_store.subscribe(),
            ui_tx,
            core_rx,
            input_text: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            auto_scroll: true,
            max_scroll_offset: 0,
            streaming_text: String::new(),
            streaming_collector: MarkdownStreamCollector::new(),
            streaming_committed_lines: Vec::new(),
            should_quit: false,
            is_turn_active: false,
            split_pane: SplitPane::new(),
            notifications: Vec::new(),
            active_tools: Vec::new(),
            pending_permission: None,
            vim: VimState::new(false),
            keybindings,
            search: SearchOverlay::new(),
            shortcuts: ShortcutsState::new(),
            history: oxicode_session::prompt_history::PersistentHistory::load(None),
            history_index: None,
            history_saved_input: String::new(),
            history_search: None,
            last_interrupt: None,
            message_cache: MessageRenderCache::new(),
            ghost_text: None,
            pending_paste: None,
            pending_images: Vec::new(),
            sent_image_paths: std::collections::HashMap::new(),
            suggestions: Vec::new(),
            turn_started_at: None,
            slash_commands,
            help_commands,
            help_shortcuts,
            autocomplete: AutocompleteState::new(),
            model_picker: ModelPickerState::new(),
            session_browser: SessionBrowserState::new(),
            message_area: Rect::default(),
            hovered_image_tag: None,
            frame_count: 0,
            session_start: Instant::now(),
            stall_start: None,
            streaming_thinking: String::new(),
            last_turn_duration: None,
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
        // Install panic hook to restore terminal on panic.
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
            let _ = io::stdout().execute(DisableMouseCapture);
            let _ = io::stdout().execute(LeaveAlternateScreen);
            original_hook(info);
        }));

        // Setup terminal
        terminal::enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        io::stdout().execute(EnableMouseCapture)?;
        io::stdout().execute(EnableBracketedPaste)?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let (mut term_rx, term_stop) = Self::spawn_terminal_event_listener();
        let result = self.event_loop(&mut terminal, &mut term_rx).await;

        // Signal polling thread to stop, then restore terminal.
        term_stop.store(true, Ordering::Relaxed);

        // Restore terminal (always, even on error)
        let _ = terminal::disable_raw_mode();
        let _ = io::stdout().execute(DisableMouseCapture);
        let _ = io::stdout().execute(DisableBracketedPaste);
        let _ = io::stdout().execute(LeaveAlternateScreen);

        // Restore original panic hook.
        let _ = std::panic::take_hook();

        result
    }

    /// Spawn a dedicated crossterm event thread and return its receiver + stop flag.
    fn spawn_terminal_event_listener() -> (mpsc::Receiver<Event>, Arc<AtomicBool>) {
        let (term_tx, term_rx) = mpsc::channel::<Event>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop = stop_flag.clone();
        tokio::task::spawn_blocking(move || {
            while !stop.load(Ordering::Relaxed) {
                if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    if let Ok(ev) = event::read() {
                        if term_tx.blocking_send(ev).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        (term_rx, stop_flag)
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<impl Backend>,
        term_rx: &mut mpsc::Receiver<Event>,
    ) -> io::Result<()> {
        // Draw the initial frame before entering the event loop.
        self.draw(terminal)?;

        loop {
            if self.should_quit {
                break;
            }

            // Dynamic tick: 50ms during active streaming for smooth spinner,
            // 100ms when idle (notifications, permission countdown).
            let tick_ms = if self.is_turn_active { 50 } else { 100 };
            let needs_tick = self.is_turn_active
                || !self.notifications.is_empty()
                || self.pending_permission.is_some();

            tokio::select! {
                Some(ev) = term_rx.recv() => {
                    match ev {
                        Event::Key(key) => self.handle_key(key).await,
                        Event::Mouse(mouse) => self.handle_mouse(mouse),
                        Event::Paste(text) => self.handle_paste(&text),
                        _ => {}
                    }
                    self.draw(terminal)?;
                }
                result = self.core_rx.recv() => {
                    if let Some(core_event) = result {
                        self.handle_core_event(core_event);
                        // Drain all pending core events before redrawing (batch updates).
                        while let Ok(ev) = self.core_rx.try_recv() {
                            self.handle_core_event(ev);
                        }
                        self.draw(terminal)?;
                    } else {
                        // Engine channel closed — quit gracefully.
                        self.should_quit = true;
                    }
                }
                // Tick for spinner animation, notification expiry, and permission countdown.
                () = tokio::time::sleep(Duration::from_millis(tick_ms)), if needs_tick => {
                    // Auto-deny permission if countdown expired (30s).
                    if let Some(ref perm) = self.pending_permission {
                        if perm.created_at.elapsed().as_secs() >= 30 {
                            if let Some(perm) = self.pending_permission.take() {
                                let _ = perm.reply_tx.send(oxicode_common::PermissionResponse::Deny);
                            }
                            self.notifications.push(Notification::new(
                                "Permission auto-denied (timeout)".to_string(),
                                crate::widgets::notification::NotificationLevel::Warning,
                            ));
                        }
                    }
                    // Stall recovery: if turn active for 120s+ with no data,
                    // auto-interrupt to prevent permanent hang after API failures.
                    if self.is_turn_active {
                        if let Some(stall) = self.stall_start {
                            if stall.elapsed().as_secs() >= 120 {
                                self.is_turn_active = false;
                                self.turn_started_at = None;
                                self.stall_start = None;
                                self.streaming_text.clear();
                                self.streaming_collector.clear();
                                self.streaming_committed_lines.clear();
                                self.streaming_thinking.clear();
                                self.active_tools.clear();
                                let _ = self.ui_tx.send(UiEvent::InterruptTurn).await;
                                self.notifications.push(Notification::new(
                                    "Stream stalled — auto-interrupted after 2m".to_string(),
                                    crate::widgets::notification::NotificationLevel::Warning,
                                ));
                            }
                        }
                    }
                    self.draw(terminal)?;
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> io::Result<()> {
        // Advance frame counter (drives spinner animation).
        self.frame_count = self.frame_count.wrapping_add(1);
        // Minimum terminal size guard — skip rendering if too small.
        let term_size = terminal.size()?;
        if term_size.width < 40 || term_size.height < 8 {
            terminal.draw(|frame| {
                let area = frame.area();
                let msg = if area.width < 20 || area.height < 3 {
                    "Too small"
                } else {
                    "Terminal too small (min 40x8)"
                };
                let text = ratatui::text::Text::from(msg);
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(text)
                        .style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow)),
                    area,
                );
            })?;
            return Ok(());
        }

        // Prune expired notifications to prevent unbounded growth.
        self.notifications.retain(Notification::is_active);

        // ── Read only what we need from state (NO deep clone) ──
        // Borrow the watch receiver briefly, extract lightweight fields only.
        let (
            current_model,
            total_usage,
            is_streaming,
            auth_label,
            message_count,
            last_role,
            message_roles,
            agent_infos,
            task_infos,
            context_window_max,
            permission_mode,
            cwd,
        ) = {
            let state = self.state_rx.borrow();
            // Update render cache with new messages (incremental — only renders new ones).
            let term_width = terminal.size().map(|s| s.width).unwrap_or(80);
            self.message_cache.update(&state.messages, term_width);
            let msg_count = state.messages.len();
            let last_role = state.messages.last().map(|m| m.role);
            let msg_roles: Vec<oxicode_common::Role> =
                state.messages.iter().map(|m| m.role).collect();
            let agents: Vec<AgentInfo> = state
                .active_agents
                .iter()
                .map(|a| AgentInfo {
                    name: a.name.clone(),
                    status: a.status.clone(),
                    started_at: a.started_at.clone(),
                    duration: String::new(),
                    model: String::new(),
                    restricted_tools: Vec::new(),
                })
                .collect();
            let tasks: Vec<TaskInfo> = state
                .background_tasks
                .iter()
                .map(|t| TaskInfo {
                    id: t.id.clone(),
                    task_type: t.task_type.clone(),
                    status: t.status.clone(),
                    command_preview: t.command_preview.clone(),
                })
                .collect();
            (
                state.current_model.clone(),
                state.total_usage.clone(),
                state.is_streaming,
                state.auth_label.clone(),
                msg_count,
                last_role,
                msg_roles,
                agents,
                tasks,
                state.context_window_max,
                state.permission_mode.clone(),
                state.cwd.clone(),
            )
        }; // state borrow dropped here — no deep clone of messages

        let vim_enabled = self.vim.enabled;
        let vim_badge = if vim_enabled {
            self.vim.mode.badge()
        } else {
            ""
        };
        let search_active = self.search.is_active();
        let shortcuts_visible = self.shortcuts.is_visible();
        let command_buf = if vim_enabled && self.vim.mode == crate::vim_mode::Mode::Command {
            Some(self.vim.command_buffer().to_string())
        } else {
            None
        };
        let ghost_ref = self.ghost_text.as_deref();
        // Show suggestions only when idle, input empty, and suggestions exist.
        let show_suggestions =
            !is_streaming && self.input_text.is_empty() && !self.suggestions.is_empty();
        let autocomplete_active = self.autocomplete.active;
        let autocomplete_height = self.autocomplete.visible_height();

        terminal.draw(|frame| {
            let base_input_height = InputBox::required_height(&self.input_text);
            let input_height = if search_active {
                base_input_height + 2 // +2 for search bar
            } else {
                base_input_height
            };
            let suggestion_height: u16 = u16::from(show_suggestions);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),                   // Status bar
                    Constraint::Min(5),                      // Message view
                    Constraint::Length(autocomplete_height), // Command autocomplete (0 when inactive)
                    Constraint::Length(suggestion_height),   // Suggestion chips (0 or 1)
                    Constraint::Length(input_height),        // Input box (+ search bar)
                ])
                .split(frame.area());

            // Status bar with vim badge and auth status.
            let context_pct = if context_window_max > 0 {
                let used = total_usage.input_tokens + total_usage.output_tokens;
                #[allow(clippy::cast_precision_loss)]
                Some(used as f32 / context_window_max as f32 * 100.0)
            } else {
                None
            };
            let provider_name = detect_provider_from_model_name(&current_model);
            let status_bar = StatusBar::new(&current_model, &total_usage, is_streaming)
                .with_provider(&provider_name)
                .with_vim_badge(vim_badge)
                .with_auth_label(&auth_label)
                .with_context_pct(context_pct)
                .with_permission_mode(&permission_mode)
                .with_cwd(&cwd)
                .with_session_start(Some(self.session_start));
            frame.render_widget(status_bar, chunks[0]);

            // Content area — optionally split into left (messages) + right (agents/tasks)
            let content_area = chunks[1];
            let (left_area, right_area) = self.split_pane.split(content_area);
            // Store message area for scrollbar hit-testing in handle_mouse().
            self.message_area = left_area;

            // Message view (left pane, or full area when right pane is hidden)
            // Show streaming lines when turn is active OR when committed lines
            // exist (between StreamEnd and MessageComplete).
            let streaming_lines = if !self.streaming_committed_lines.is_empty() {
                Some(self.streaming_committed_lines.as_slice())
            } else if self.is_turn_active {
                // Turn active but no lines yet — pass empty slice so thinking indicator shows.
                Some([].as_slice())
            } else {
                None
            };
            let streaming_tail_owned: Option<String> = if self.is_turn_active {
                self.streaming_collector
                    .trailing_fragment()
                    .map(std::string::ToString::to_string)
            } else {
                None
            };
            let streaming_tail = streaming_tail_owned.as_deref();
            // Build active tools snapshot for streaming display.
            let active_tool_info: Vec<ActiveToolInfo<'_>> = self
                .active_tools
                .iter()
                .map(|t| ActiveToolInfo {
                    name: &t.name,
                    input_summary: &t.input_summary,
                    raw_input: &t.raw_input,
                    started_at: t.started_at,
                    result: t.result.as_ref().map(|(c, e)| (c.as_str(), *e)),
                })
                .collect();

            // Compute scroll offset to pass to MessageView.
            // For auto-scroll, use u16::MAX as sentinel ("scroll to bottom");
            // MessageView::render() will resolve it to the actual max offset.
            // For manual scroll, pass the stored offset (will be clamped inside).
            let scroll_for_view = if self.auto_scroll {
                u16::MAX
            } else {
                self.scroll_offset.min(self.max_scroll_offset)
            };

            // Shared cell: MessageView writes the actual max scroll after rendering.
            let scroll_report = Rc::new(Cell::new(0u16));

            let message_view = MessageView::new(
                self.message_cache.lines(message_count),
                message_count,
                last_role,
                streaming_lines,
                streaming_tail,
                &active_tool_info,
                scroll_for_view,
            )
            .with_turn_started_at(self.turn_started_at)
            .with_scroll_report(Rc::clone(&scroll_report))
            .with_frame_count(self.frame_count)
            .with_stall_start(self.stall_start)
            .with_model_name(&current_model)
            .with_cwd(&cwd)
            .with_streaming_thinking(&self.streaming_thinking)
            .with_last_turn_duration(self.last_turn_duration)
            .with_message_roles(message_roles);
            frame.render_widget(message_view, left_area);

            // Apply underline hover style to [Image #N] tag under mouse cursor.
            if let Some((hover_row, col_start, col_end)) = self.hovered_image_tag {
                let buf = frame.buffer_mut();
                for cx in col_start..col_end {
                    if let Some(cell) = buf.cell_mut((cx, hover_row)) {
                        cell.set_style(
                            cell.style()
                                .add_modifier(ratatui::style::Modifier::UNDERLINED),
                        );
                    }
                }
            }

            // Read back the actual max scroll offset computed during rendering.
            self.max_scroll_offset = scroll_report.get();
            if self.auto_scroll {
                self.scroll_offset = self.max_scroll_offset;
            } else {
                self.scroll_offset = self.scroll_offset.min(self.max_scroll_offset);
            }

            // Right pane: agent panel (top) + task panel (bottom)
            if let Some(right) = right_area {
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

            // Help overlay (drawn on top of content area).
            if shortcuts_visible {
                let help = crate::widgets::HelpOverlay::new(
                    &self.shortcuts,
                    &self.help_shortcuts,
                    &self.help_commands,
                );
                frame.render_widget(help, content_area);
            }

            // Model picker overlay (drawn on top of content area).
            if self.model_picker.is_visible() {
                let picker = crate::widgets::ModelPicker::new(&self.model_picker);
                frame.render_widget(picker, content_area);
            }

            // Session browser overlay (drawn on top of content area).
            if self.session_browser.is_visible() {
                let browser = crate::widgets::SessionBrowser::new(&self.session_browser);
                frame.render_widget(browser, content_area);
            }

            // History search overlay (Ctrl+R reverse search).
            if let Some(ref search_state) = self.history_search {
                let search_widget = HistorySearchWidget::new(search_state);
                frame.render_widget(search_widget, content_area);
            }

            // Permission dialog overlay (drawn on top of everything).
            if let Some(ref perm) = self.pending_permission {
                let remaining = 30u32.saturating_sub(perm.created_at.elapsed().as_secs() as u32);
                let dialog =
                    PermissionDialog::new(&perm.tool_name, &perm.input_summary, &perm.prompt)
                        .with_selected(perm.selected)
                        .with_risk_level(perm.risk_level)
                        .with_kind(&perm.kind)
                        .with_countdown(remaining);
                frame.render_widget(dialog, content_area);
            }

            // Paste preview overlay.
            if let Some(ref paste) = self.pending_paste {
                frame.render_widget(PastePreview::new(paste), content_area);
            }

            // Suggestion chips (shown when idle + empty input).
            if show_suggestions {
                frame.render_widget(SuggestionChips::new(&self.suggestions), chunks[3]);
            }

            // Command autocomplete dropdown (shown when typing `/...`).
            if autocomplete_active {
                frame.render_widget(
                    CommandAutocomplete::new(&self.slash_commands, &self.autocomplete),
                    chunks[2],
                );
            }

            // Input box area — may include search bar below.
            if search_active {
                let input_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(base_input_height), Constraint::Length(2)])
                    .split(chunks[4]);

                let byte_cursor = char_to_byte_index(&self.input_text, self.input_cursor);
                let mut input = InputBox::new(&self.input_text, byte_cursor, true);
                if vim_enabled {
                    input = input.with_vim_badge(vim_badge);
                }
                if let Some(g) = ghost_ref {
                    input = input.with_ghost_text(g);
                }
                if !self.input_text.is_empty() {
                    let chars = self.input_text.chars().count();
                    let lines = self.input_text.split('\n').count();
                    input = input.with_metrics(chars, lines);
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
                if let Some(g) = ghost_ref {
                    input = input.with_ghost_text(g);
                }
                if !self.input_text.is_empty() {
                    let chars = self.input_text.chars().count();
                    let lines = self.input_text.split('\n').count();
                    input = input.with_metrics(chars, lines);
                }
                frame.render_widget(input, chunks[4]);
            }
        })?;

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_key(&mut self, key: KeyEvent) {
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
                if self.is_turn_active {
                    // Double Ctrl+C within 1s → force quit.
                    if self
                        .last_interrupt
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                    {
                        let _ = self.ui_tx.send(UiEvent::Quit).await;
                        self.should_quit = true;
                    } else {
                        self.last_interrupt = Some(Instant::now());
                        let _ = self.ui_tx.send(UiEvent::InterruptTurn).await;
                        self.notifications.push(Notification::new(
                            "Interrupting... (Ctrl+C again to quit)".to_string(),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                    }
                } else {
                    let _ = self.ui_tx.send(UiEvent::Quit).await;
                    self.should_quit = true;
                }
            }
            (_, KeyCode::Esc) if self.is_turn_active => {
                let _ = self.ui_tx.send(UiEvent::InterruptTurn).await;
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
                if let Some(img) = crate::image_paste::read_clipboard_image() {
                    let image_num = self.pending_images.len() + 1;
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
                self.history_prev();
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
    async fn handle_vim_key(&mut self, key: KeyEvent) {
        let text_len = self.input_text.chars().count();
        let action = self.vim.handle_key(key, text_len);

        match action {
            VimAction::Passthrough(k) => {
                // Let Ctrl+C through for quit/interrupt.
                if k.modifiers == KeyModifiers::CONTROL && k.code == KeyCode::Char('c') {
                    if self.is_turn_active {
                        if self
                            .last_interrupt
                            .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                        {
                            let _ = self.ui_tx.send(UiEvent::Quit).await;
                            self.should_quit = true;
                        } else {
                            self.last_interrupt = Some(Instant::now());
                            let _ = self.ui_tx.send(UiEvent::InterruptTurn).await;
                            self.notifications.push(Notification::new(
                                "Interrupting... (Ctrl+C again to quit)".to_string(),
                                crate::widgets::notification::NotificationLevel::Info,
                            ));
                        }
                    } else {
                        let _ = self.ui_tx.send(UiEvent::Quit).await;
                        self.should_quit = true;
                    }
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
                let _ = self.ui_tx.send(UiEvent::Quit).await;
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
    fn resolve_text_object(&self, modifier: char, target: char) -> Option<(usize, usize)> {
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
    fn handle_search_key(&mut self, key: KeyEvent) {
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
    fn update_search_matches(&mut self) {
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
    fn scroll_to_current_match(&mut self) {
        if let Some(line_idx) = self.search.current_match_line() {
            // Convert flattened line index to scroll offset.
            // Account for message view inner height (approx).
            let target = line_idx as u16;
            self.scroll_offset = target.saturating_sub(3); // show a few lines above match
            self.auto_scroll = false;
        }
    }

    /// Handle mouse events: scroll wheel, scrollbar click/drag, Cmd+click image, hover.
    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_up_by(3),
            MouseEventKind::ScrollDown => self.scroll_down_by(3),
            MouseEventKind::Down(MouseButton::Left) => {
                // Cmd+Click (macOS) or Ctrl+Click (Linux/Windows) to open image.
                let has_modifier = mouse.modifiers.contains(KeyModifiers::SUPER)
                    || mouse.modifiers.contains(KeyModifiers::CONTROL);
                if has_modifier && self.handle_image_click(mouse.column, mouse.row) {
                    return;
                }
                self.handle_scrollbar_click(mouse.column, mouse.row);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.handle_scrollbar_click(mouse.column, mouse.row);
            }
            MouseEventKind::Moved => {
                self.update_image_hover(mouse.column, mouse.row);
            }
            _ => {}
        }
    }

    /// Handle click/drag on the scrollbar track — jump to proportional position.
    ///
    /// The message view has a `Block` with `Borders::ALL`, so the scrollbar track
    /// occupies rows `area.y + 1` to `area.bottom() - 2` (inside borders).
    /// The scrollbar column is at `area.right() - 1` (right border/track).
    fn handle_scrollbar_click(&mut self, col: u16, row: u16) {
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
    fn handle_image_click(&self, col: u16, row: u16) -> bool {
        if let Some(hit) = self.find_image_tag_at(col, row) {
            if let Some(paths) = self.sent_image_paths.get(&hit.msg_index) {
                if let Some(path) = paths.get(hit.image_number.wrapping_sub(1)) {
                    crate::image_paste::open_file_in_viewer(path);
                    return true;
                }
            }
        }
        false
    }

    /// Update hover state: underline `[Image #N]` tag under the mouse cursor.
    fn update_image_hover(&mut self, col: u16, row: u16) {
        if let Some(hit) = self.find_image_tag_at(col, row) {
            if self.sent_image_paths.contains_key(&hit.msg_index) {
                let area = self.message_area;
                let inner_left = area.x + 1;
                self.hovered_image_tag = Some((
                    row,
                    inner_left + hit.col_start as u16,
                    inner_left + hit.col_end as u16,
                ));
                return;
            }
        }
        self.hovered_image_tag = None;
    }

    /// Find an `[Image #N]` tag at a screen position.
    ///
    /// Returns hit info if the position maps to an `[Image #N]` span.
    fn find_image_tag_at(&self, col: u16, row: u16) -> Option<ImageTagHit> {
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
        let abs_line = (row - inner_top) as usize + self.scroll_offset as usize;

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
        let click_col = (col - inner_left) as usize;
        let mut span_offset: usize = 0;
        for span in &line.spans {
            let content = span.content.as_ref();
            let span_width = content.len();
            if content.starts_with("[Image #")
                && content.ends_with(']')
                && click_col >= span_offset
                && click_col < span_offset + span_width
            {
                let num_str = &content[8..content.len() - 1];
                if let Ok(n) = num_str.parse::<usize>() {
                    return Some(ImageTagHit {
                        msg_index: msg_idx,
                        image_number: n,
                        col_start: span_offset,
                        col_end: span_offset + span_width,
                    });
                }
            }
            span_offset += span_width;
        }
        None
    }

    /// Handle bracketed paste — insert text at cursor in bulk,
    /// or show preview modal for large pastes.
    fn handle_paste(&mut self, text: &str) {
        // Skip paste if a permission dialog, search overlay, or paste preview is active.
        if self.pending_permission.is_some()
            || self.search.is_active()
            || self.pending_paste.is_some()
        {
            return;
        }
        if text.lines().count() > PASTE_PREVIEW_THRESHOLD {
            self.pending_paste = Some(text.to_string());
        } else {
            let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
            self.input_text.insert_str(byte_idx, text);
            self.input_cursor += text.chars().count();
            self.update_ghost_text();
        }
    }

    /// Handle key events when the paste preview modal is active.
    fn handle_paste_preview_key(&mut self, key: KeyEvent) {
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
    async fn handle_model_picker_key(&mut self, key: KeyEvent) {
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
                        .send(UiEvent::SlashCommand {
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
    async fn handle_session_browser_key(&mut self, key: KeyEvent) {
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
                            .send(UiEvent::SlashCommand {
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
                            .send(UiEvent::SlashCommand {
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

    /// Open the session browser with sessions loaded from disk.
    fn open_session_browser(&mut self) {
        // Load sessions from the oxicode-session crate.
        let summaries = oxicode_session::list_sessions(None).unwrap_or_default();
        let entries: Vec<SessionEntry> = summaries
            .into_iter()
            .map(|s| {
                let title = s
                    .preview
                    .unwrap_or_else(|| format!("[{}]", &s.id[..8.min(s.id.len())]));
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

    fn scroll_up_by(&mut self, lines: u16) {
        if lines == 0 {
            return;
        }

        if self.auto_scroll {
            self.scroll_offset = self.max_scroll_offset;
            self.auto_scroll = false;
        }

        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    fn scroll_down_by(&mut self, lines: u16) {
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

    /// Execute a keybinding action.
    #[allow(clippy::too_many_lines)]
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

    /// Submit the current input text.
    async fn submit_input(&mut self) {
        // Block input while a turn is active.
        if self.is_turn_active {
            self.notifications.push(Notification::new(
                "Waiting for current response...".to_string(),
                crate::widgets::notification::NotificationLevel::Info,
            ));
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
                    let _ = self.ui_tx.send(UiEvent::Quit).await;
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
                if trimmed == "sessions" || trimmed == "session" {
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
                let (name, args) = match trimmed.split_once(char::is_whitespace) {
                    Some((n, a)) => (n.to_string(), a.trim().to_string()),
                    None => (trimmed.to_string(), String::new()),
                };
                let _ = self.ui_tx.send(UiEvent::SlashCommand { name, args }).await;
            } else {
                // Strip [Image #N] placeholders from text before sending.
                let clean_text = strip_image_tags(&text);
                // Retain image paths for click-to-open in message view.
                if !self.pending_images.is_empty() {
                    let state = self.state_rx.borrow();
                    let msg_index = state.messages.len();
                    drop(state);
                    let paths: Vec<std::path::PathBuf> = self
                        .pending_images
                        .iter()
                        .map(|img| img.path.clone())
                        .collect();
                    self.sent_image_paths.insert(msg_index, paths);
                }
                let images = std::mem::take(&mut self.pending_images);
                let _ = self
                    .ui_tx
                    .send(UiEvent::UserInput {
                        text: clean_text,
                        images,
                    })
                    .await;
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
        if let Some(content) = self.history.get(idx) {
            self.input_text = content.to_string();
            self.input_cursor = self.input_text.chars().count();
        }
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
            if let Some(content) = self.history.get(idx + 1) {
                self.input_text = content.to_string();
                self.input_cursor = self.input_text.chars().count();
            }
        }
    }

    /// Recompute ghost text completion based on current input.
    fn update_ghost_text(&mut self) {
        self.ghost_text = crate::ghost_completion::complete(&self.input_text, &self.slash_commands);
    }

    /// Accept the current ghost text completion. Returns true if accepted.
    fn accept_ghost_text(&mut self) -> bool {
        if let Some(ghost) = self.ghost_text.take() {
            self.input_text.push_str(&ghost);
            self.input_cursor = self.input_text.chars().count();
            true
        } else {
            false
        }
    }

    /// Open the reverse history search overlay (Ctrl+R).
    fn open_history_search(&mut self) {
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
    fn handle_history_search_key(&mut self, key: KeyEvent) {
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
    fn refresh_history_search(&mut self) {
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
    fn activate_autocomplete(&mut self) {
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
    fn handle_autocomplete_key(&mut self, key: KeyEvent) {
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

    /// Handle key events when the permission dialog is active.
    async fn handle_permission_key(&mut self, key: KeyEvent) {
        let Some(ref mut perm) = self.pending_permission else {
            return;
        };
        let max_idx = perm.kind.option_count().saturating_sub(1);
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) => {
                perm.selected = perm.selected.saturating_sub(1);
            }
            (_, KeyCode::Down) => {
                perm.selected = (perm.selected + 1).min(max_idx);
            }
            (_, KeyCode::Enter) => {
                // Map selected index to response based on dialog kind.
                // FileRead (3 opts): 0=AllowOnce, 1=AlwaysAllow, 2=Deny
                // Others  (4 opts): 0=AllowOnce, 1=AlwaysAllow, 2=Deny, 3=AlwaysDeny
                let response = match perm.selected {
                    0 => oxicode_common::PermissionResponse::AllowOnce,
                    1 => oxicode_common::PermissionResponse::AlwaysAllow,
                    2 => oxicode_common::PermissionResponse::Deny,
                    _ => oxicode_common::PermissionResponse::AlwaysDeny,
                };
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm.reply_tx.send(response);
                }
            }
            // Hotkeys: y = allow once, a = allow session, n = deny, N = always deny
            (_, KeyCode::Char('y')) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm
                        .reply_tx
                        .send(oxicode_common::PermissionResponse::AllowOnce);
                }
            }
            (_, KeyCode::Char('a')) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm
                        .reply_tx
                        .send(oxicode_common::PermissionResponse::AlwaysAllow);
                }
            }
            (_, KeyCode::Char('n') | KeyCode::Esc) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm.reply_tx.send(oxicode_common::PermissionResponse::Deny);
                }
            }
            (KeyModifiers::SHIFT, KeyCode::Char('N')) => {
                // "Always deny" hotkey — only for dialogs that have this option.
                if let Some(ref perm) = self.pending_permission {
                    if matches!(
                        perm.kind,
                        crate::widgets::PermissionDialogKind::FileRead { .. }
                    ) {
                        return; // FileRead has no "Always deny" option
                    }
                }
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm
                        .reply_tx
                        .send(oxicode_common::PermissionResponse::AlwaysDeny);
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm.reply_tx.send(oxicode_common::PermissionResponse::Deny);
                }
                if self.is_turn_active {
                    let _ = self.ui_tx.send(UiEvent::InterruptTurn).await;
                    self.notifications.push(Notification::new(
                        "Permission denied, interrupting...".to_string(),
                        crate::widgets::notification::NotificationLevel::Info,
                    ));
                } else {
                    let _ = self.ui_tx.send(UiEvent::Quit).await;
                    self.should_quit = true;
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_core_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::TextDelta(text) => {
                self.streaming_text.push_str(&text);
                self.streaming_collector.push_delta(&text);
                let new_lines = self.streaming_collector.commit_complete_lines();
                self.streaming_committed_lines.extend(new_lines);
                self.auto_scroll = true;
                // Reset stall timer on each delta (data is flowing).
                self.stall_start = Some(Instant::now());
            }
            CoreEvent::StreamStart => {
                self.is_turn_active = true;
                self.turn_started_at = Some(Instant::now());
                self.stall_start = Some(Instant::now());
                self.last_turn_duration = None; // Clear previous turn's duration.
                self.streaming_text.clear();
                self.streaming_collector.clear();
                self.streaming_committed_lines.clear();
                self.streaming_thinking.clear();
                self.active_tools.clear();
            }
            CoreEvent::StreamEnd => {
                // Stream finished — finalize remaining buffer but keep committed
                // lines visible until MessageComplete (prevents blank frame flash).
                // Tools may still be running in multi-turn loops.
                let final_lines = self.streaming_collector.finalize();
                self.streaming_committed_lines.extend(final_lines);
                // Clear only the raw buffer and collector, NOT the committed lines.
                self.streaming_text.clear();
                self.streaming_collector.clear();
                self.auto_scroll = true;
            }
            CoreEvent::MessageComplete => {
                // Full turn complete — message now persisted in state_store.messages.
                // Save turn duration before clearing timer.
                self.last_turn_duration = self.turn_started_at.map(|t| t.elapsed());
                // Force message cache update BEFORE clearing streaming state to
                // prevent a blank frame between "streaming visible" and "cached visible".
                {
                    let state = self.state_rx.borrow();
                    // Use a reasonable width; draw() will recalculate with actual terminal width.
                    self.message_cache
                        .update(&state.messages, self.message_cache.cached_width());
                }
                // Now safe to clear all transient streaming state.
                self.streaming_text.clear();
                self.streaming_collector.clear();
                self.streaming_committed_lines.clear();
                self.streaming_thinking.clear();
                self.active_tools.clear();
                self.is_turn_active = false;
                self.turn_started_at = None;
                self.stall_start = None;
                self.auto_scroll = true;
                // Compute context-aware suggestions from latest messages.
                let state = self.state_rx.borrow();
                self.suggestions = suggest_prompts(&state.messages);
            }
            CoreEvent::Error(msg) => {
                // Show error as a toast notification (no tracing::error! to avoid
                // corrupting the TUI — stderr leaks into the alternate screen).
                self.notifications.push(Notification::new(
                    msg,
                    crate::widgets::notification::NotificationLevel::Error,
                ));
                // Clear streaming state on error (engine may not send StreamEnd).
                self.streaming_text.clear();
                self.streaming_collector.clear();
                self.streaming_committed_lines.clear();
                self.streaming_thinking.clear();
                self.active_tools.clear();
                self.is_turn_active = false;
                // Reset turn timer — otherwise the "thinking" indicator would
                // keep displaying a stale elapsed duration after an API error.
                self.turn_started_at = None;
                self.stall_start = None;
            }
            CoreEvent::ToolUseStart { id, name, input } => {
                let summary = summarize_input(&input);
                self.active_tools.push(ActiveToolCall {
                    id,
                    name,
                    input_summary: summary,
                    raw_input: input,
                    started_at: Instant::now(),
                    result: None,
                });
                self.auto_scroll = true;
            }
            CoreEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                if let Some(tool) = self.active_tools.iter_mut().find(|t| t.id == tool_use_id) {
                    tool.result = Some((content, is_error));
                }
                self.auto_scroll = true;
            }
            CoreEvent::PermissionAsk {
                tool_name,
                input_summary,
                prompt,
                reply_tx,
            } => {
                // Compute risk level: start from tool name, then escalate if
                // the input contains dangerous patterns (e.g. rm -rf for Bash).
                let mut risk_level = RiskLevel::from_tool(&tool_name);
                if risk_level != RiskLevel::High
                    && is_dangerous_operation(&tool_name, &input_summary)
                {
                    risk_level = RiskLevel::High;
                }
                let kind = crate::widgets::PermissionDialogKind::detect(&tool_name, &input_summary);
                self.pending_permission = Some(PendingPermission {
                    tool_name,
                    input_summary,
                    prompt,
                    selected: 0,
                    risk_level,
                    kind,
                    created_at: Instant::now(),
                    reply_tx,
                });
            }
            CoreEvent::RateLimited {
                message: _,
                attempt,
                max_retries,
                retry_in_secs,
            } => {
                let notif_msg = format!(
                    "Rate limited. Retrying in {retry_in_secs:.0}s... ({attempt}/{max_retries})"
                );
                // No tracing::warn! — would corrupt TUI display via stderr.
                self.notifications.push(Notification::new(
                    notif_msg,
                    crate::widgets::notification::NotificationLevel::RateLimit,
                ));
            }
            CoreEvent::Retrying {
                message,
                attempt,
                max_retries,
                retry_in_secs,
            } => {
                let notif_msg =
                    format!("Retrying ({attempt}/{max_retries}) in {retry_in_secs:.0}s: {message}");
                self.notifications.push(Notification::new(
                    notif_msg,
                    crate::widgets::notification::NotificationLevel::Warning,
                ));
            }
            CoreEvent::ThinkingDelta(text) => {
                // Accumulate thinking text for live display during streaming.
                self.streaming_thinking.push_str(&text);
                self.auto_scroll = true;
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

/// Strip `[Image #N]` placeholder tags from input text, collapsing extra whitespace.
fn strip_image_tags(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find("[Image #") {
        if let Some(end_offset) = remaining[start..].find(']') {
            let end = start + end_offset + 1;
            let inner = &remaining[start + 8..start + end_offset];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                result.push_str(&remaining[..start]);
                remaining = &remaining[end..];
                // Skip trailing space after tag.
                if remaining.starts_with(' ') {
                    remaining = &remaining[1..];
                }
                continue;
            }
        }
        let chunk_end = start + 8;
        result.push_str(&remaining[..chunk_end]);
        remaining = &remaining[chunk_end..];
    }
    result.push_str(remaining);
    result.trim().to_string()
}

/// Format a `DateTime<Utc>` as a human-readable relative time string.
fn format_relative_time(dt: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(dt);

    let secs = diff.num_seconds();
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = diff.num_minutes();
    if mins < 60 {
        return if mins == 1 {
            "1 min ago".to_string()
        } else {
            format!("{mins} mins ago")
        };
    }
    let hours = diff.num_hours();
    if hours < 24 {
        return if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{hours} hours ago")
        };
    }
    let days = diff.num_days();
    if days < 30 {
        return if days == 1 {
            "yesterday".to_string()
        } else {
            format!("{days} days ago")
        };
    }
    let months = days / 30;
    if months < 12 {
        return if months == 1 {
            "1 month ago".to_string()
        } else {
            format!("{months} months ago")
        };
    }
    let years = days / 365;
    if years == 1 {
        "1 year ago".to_string()
    } else {
        format!("{years} years ago")
    }
}

/// Detect provider name from model name for status bar display.
fn detect_provider_from_model_name(model: &str) -> String {
    let m = model.to_lowercase();
    if m.starts_with("anthropic.claude") {
        "bedrock".to_string()
    } else if m.starts_with("claude-") || m.starts_with("anthropic/") {
        "anthropic".to_string()
    } else if m.starts_with("gpt-")
        || m.starts_with("o1-")
        || m.starts_with("o3-")
        || m.starts_with("o4-")
    {
        "openai".to_string()
    } else if m.starts_with("deepseek-") || m.starts_with("deepseek/") {
        "deepseek".to_string()
    } else if m.contains(':') && !m.contains('/') {
        "ollama".to_string()
    } else if m.contains('/') && !m.starts_with("anthropic/") && !m.starts_with("deepseek/") {
        "openrouter".to_string()
    } else {
        String::new()
    }
}

/// Dangerous command patterns for permission dialog warning.
const DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf",
    "rm -r",
    "rmdir",
    "sudo ",
    "chmod 777",
    "chmod -R",
    "> /dev/",
    "mkfs",
    "dd if=",
    ":(){ :|:",
    "shutdown",
    "reboot",
    "kill -9",
    "pkill",
    "git push --force",
    "git reset --hard",
    "DROP TABLE",
    "DROP DATABASE",
    "TRUNCATE",
    "DELETE FROM",
];

/// Check if a tool operation is potentially dangerous.
fn is_dangerous_operation(tool_name: &str, input_summary: &str) -> bool {
    if tool_name == "bash" || tool_name == "Bash" {
        let lower = input_summary.to_lowercase();
        return DANGEROUS_PATTERNS
            .iter()
            .any(|p| lower.contains(&p.to_lowercase()));
    }
    // File write/edit to sensitive paths.
    if tool_name == "file_write"
        || tool_name == "file_edit"
        || tool_name == "Write"
        || tool_name == "Edit"
    {
        let sensitive = [
            "/etc/",
            "/usr/",
            "/bin/",
            "/sbin/",
            ".env",
            "credentials",
            ".ssh/",
        ];
        return sensitive.iter().any(|s| input_summary.contains(s));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use oxicode_common::{Message, PermissionResponse};
    use ratatui::backend::TestBackend;
    use tokio::sync::oneshot;

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

    #[test]
    fn test_char_to_byte_index_empty_string() {
        // Empty string: any char index returns 0 (the length).
        assert_eq!(char_to_byte_index("", 0), 0);
        assert_eq!(char_to_byte_index("", 5), 0);
    }

    #[test]
    fn test_char_to_byte_index_cjk() {
        // CJK characters are 3 bytes each in UTF-8.
        let s = "中文";
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 3); // '文' starts at byte 3
        assert_eq!(char_to_byte_index(s, 2), 6); // past end
    }

    #[test]
    fn test_char_to_byte_index_fire_emoji() {
        // 🔥 is 4 bytes in UTF-8.
        let s = "🔥x";
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 4); // 'x' starts at byte 4
        assert_eq!(char_to_byte_index(s, 2), 5); // past end = s.len()
                                                 // Index beyond string length clamps to s.len().
        assert_eq!(char_to_byte_index(s, 99), 5);
    }

    #[test]
    fn test_summarize_input_command() {
        let v = serde_json::json!({"command": "echo hello"});
        assert_eq!(summarize_input(&v), "echo hello");
    }

    #[test]
    fn test_summarize_input_file_path() {
        let v = serde_json::json!({"file_path": "/foo.rs"});
        assert_eq!(summarize_input(&v), "/foo.rs");
    }

    #[test]
    fn test_summarize_input_pattern() {
        let v = serde_json::json!({"pattern": "test", "path": "src"});
        assert_eq!(summarize_input(&v), "test in src");
    }

    #[test]
    fn test_summarize_input_pattern_default_path() {
        // When "path" is missing, defaults to "."
        let v = serde_json::json!({"pattern": "fn main"});
        assert_eq!(summarize_input(&v), "fn main in .");
    }

    #[test]
    fn test_summarize_input_empty_object() {
        let v = serde_json::json!({});
        // Falls back to JSON serialization of empty object.
        let result = summarize_input(&v);
        assert_eq!(result, "{}");
    }

    #[test]
    fn test_summarize_input_long_command_truncated() {
        // Commands > 80 chars are truncated with "...".
        let long_cmd = "a".repeat(100);
        let v = serde_json::json!({"command": long_cmd});
        let result = summarize_input(&v);
        assert!(result.ends_with("..."), "long input should end with '...'");
        // Truncated at char 80, so total visible prefix chars = 80.
        let prefix: String = result.chars().take(80).collect();
        assert_eq!(prefix, "a".repeat(80));
    }

    #[test]
    fn test_detect_provider_anthropic() {
        assert_eq!(
            detect_provider_from_model_name("claude-sonnet-4-20250514"),
            "anthropic"
        );
        assert_eq!(
            detect_provider_from_model_name("claude-opus-4"),
            "anthropic"
        );
        assert_eq!(
            detect_provider_from_model_name("anthropic/claude-3"),
            "anthropic"
        );
    }

    #[test]
    fn test_detect_provider_openai() {
        assert_eq!(detect_provider_from_model_name("gpt-4"), "openai");
        assert_eq!(detect_provider_from_model_name("gpt-3.5-turbo"), "openai");
        assert_eq!(detect_provider_from_model_name("o1-mini"), "openai");
        assert_eq!(detect_provider_from_model_name("o3-mini"), "openai");
        assert_eq!(detect_provider_from_model_name("o4-preview"), "openai");
    }

    #[test]
    fn test_detect_provider_deepseek() {
        assert_eq!(detect_provider_from_model_name("deepseek-chat"), "deepseek");
        assert_eq!(
            detect_provider_from_model_name("deepseek/coder"),
            "deepseek"
        );
    }

    #[test]
    fn test_detect_provider_ollama() {
        // Ollama model names contain ':' without '/'
        assert_eq!(detect_provider_from_model_name("llama:7b"), "ollama");
        assert_eq!(detect_provider_from_model_name("mistral:latest"), "ollama");
    }

    #[test]
    fn test_detect_provider_openrouter() {
        // OpenRouter model names contain '/' but not known prefixes.
        assert_eq!(
            detect_provider_from_model_name("meta-llama/Llama-3"),
            "openrouter"
        );
        assert_eq!(
            detect_provider_from_model_name("mistralai/mixtral-8x7b"),
            "openrouter"
        );
    }

    #[test]
    fn test_detect_provider_bedrock() {
        assert_eq!(
            detect_provider_from_model_name("anthropic.claude-v2"),
            "bedrock"
        );
        assert_eq!(
            detect_provider_from_model_name("anthropic.claude-3-sonnet"),
            "bedrock"
        );
    }

    #[test]
    fn test_detect_provider_unknown() {
        assert_eq!(detect_provider_from_model_name("some-unknown-model"), "");
        assert_eq!(detect_provider_from_model_name(""), "");
    }

    #[test]
    fn test_handle_core_event_stream_start_activates_turn() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(!app.is_turn_active, "should start inactive");
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.is_turn_active, "StreamStart should activate turn");
        assert!(app.turn_started_at.is_some(), "turn timer should start");
    }

    #[test]
    fn test_handle_core_event_text_delta_updates_streaming_text() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::TextDelta("Hello".to_string()));
        assert_eq!(app.streaming_text, "Hello");
        app.handle_core_event(CoreEvent::TextDelta(" World".to_string()));
        assert_eq!(app.streaming_text, "Hello World");
    }

    #[test]
    fn test_handle_core_event_stream_end_clears_streaming_text() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::TextDelta("some content".to_string()));
        app.handle_core_event(CoreEvent::StreamEnd);
        // Raw buffer is cleared; committed lines may contain finalized content.
        assert!(
            app.streaming_text.is_empty(),
            "streaming_text cleared on StreamEnd"
        );
    }

    #[test]
    fn test_handle_core_event_message_complete_deactivates_turn() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.is_turn_active);
        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(
            !app.is_turn_active,
            "MessageComplete should deactivate turn"
        );
        assert!(app.turn_started_at.is_none(), "turn timer should reset");
        assert!(app.streaming_text.is_empty());
        assert!(app.active_tools.is_empty());
    }

    #[test]
    fn test_handle_core_event_error_adds_notification_and_clears_state() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::TextDelta("partial".to_string()));

        let before_count = app.notifications.len();
        app.handle_core_event(CoreEvent::Error("something went bad".to_string()));

        assert_eq!(
            app.notifications.len(),
            before_count + 1,
            "Error should add a notification"
        );
        assert!(!app.is_turn_active, "Error should deactivate turn");
        assert!(
            app.streaming_text.is_empty(),
            "Error should clear streaming text"
        );
        assert!(
            app.active_tools.is_empty(),
            "Error should clear active tools"
        );
    }

    /// Bug fix regression: Error must also reset turn_started_at so the thinking
    /// indicator does not show a stale elapsed time after an API error.
    #[test]
    fn test_handle_core_event_error_resets_turn_timer() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.turn_started_at.is_some(), "timer set on StreamStart");

        app.handle_core_event(CoreEvent::Error("api error".to_string()));

        assert!(
            app.turn_started_at.is_none(),
            "Error must reset turn_started_at to prevent stale thinking indicator"
        );
    }

    #[test]
    fn test_handle_core_event_tool_use_start_adds_entry() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(app.active_tools.is_empty());

        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tool-1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls -la"}),
        });

        assert_eq!(
            app.active_tools.len(),
            1,
            "ToolUseStart should add tool entry"
        );
        assert_eq!(app.active_tools[0].id, "tool-1");
        assert_eq!(app.active_tools[0].name, "bash");
        assert_eq!(app.active_tools[0].input_summary, "ls -la");
        assert!(app.active_tools[0].result.is_none(), "result not set yet");
    }

    #[test]
    fn test_handle_core_event_tool_result_sets_result_on_matching_tool() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tool-abc".to_string(),
            name: "file_read".to_string(),
            input: serde_json::json!({"file_path": "/tmp/test.txt"}),
        });
        assert!(app.active_tools[0].result.is_none());

        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tool-abc".to_string(),
            content: "file contents here".to_string(),
            is_error: false,
        });

        let result = app.active_tools[0]
            .result
            .as_ref()
            .expect("result should be set after ToolResult");
        assert_eq!(result.0, "file contents here");
        assert!(!result.1, "is_error should be false");
    }

    #[test]
    fn test_handle_core_event_tool_result_error_flag() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tool-err".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "bad_cmd"}),
        });

        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tool-err".to_string(),
            content: "command not found".to_string(),
            is_error: true,
        });

        let result = app.active_tools[0]
            .result
            .as_ref()
            .expect("result should be set");
        assert!(result.1, "is_error should be true for error result");
    }

    #[test]
    fn test_handle_core_event_tool_result_unmatched_id_noop() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tool-x".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "echo"}),
        });

        // Send result for a different ID — should not panic or modify existing tools.
        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tool-y".to_string(),
            content: "output".to_string(),
            is_error: false,
        });

        assert!(
            app.active_tools[0].result.is_none(),
            "unmatched tool_use_id should not set result"
        );
    }

    fn make_test_app() -> (App, mpsc::Receiver<UiEvent>, mpsc::Sender<CoreEvent>) {
        let state_store = Arc::new(StateStore::default());
        let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
        let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(32);
        let app = App::new(&state_store, ui_tx, core_rx, Vec::new());
        (app, ui_rx, core_tx)
    }

    fn normalized_rendered_text(terminal: &Terminal<TestBackend>) -> String {
        format!("{}", terminal.backend())
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn test_draw_with_test_backend_renders_baseline_ui() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        app.draw(&mut terminal).expect("draw succeeds");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_baseline", rendered.as_str());

        assert!(
            rendered.contains("Ready"),
            "status bar should render readiness"
        );
        assert!(
            rendered.contains("Type your message..."),
            "input placeholder should render"
        );
    }

    #[tokio::test]
    async fn test_event_loop_keyboard_input_emits_ui_events() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");
        let (term_tx, mut term_rx) = mpsc::channel::<Event>(16);

        term_tx
            .send(Event::Key(KeyEvent::new(
                KeyCode::Char('h'),
                KeyModifiers::NONE,
            )))
            .await
            .expect("send h");
        term_tx
            .send(Event::Key(KeyEvent::new(
                KeyCode::Char('i'),
                KeyModifiers::NONE,
            )))
            .await
            .expect("send i");
        term_tx
            .send(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            )))
            .await
            .expect("send enter");
        term_tx
            .send(Event::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            )))
            .await
            .expect("send ctrl+c");
        drop(term_tx);

        app.event_loop(&mut terminal, &mut term_rx)
            .await
            .expect("event loop exits");

        assert!(
            matches!(ui_rx.recv().await, Some(UiEvent::UserInput { text, .. }) if text == "hi"),
            "expected submitted input event"
        );
        assert!(
            matches!(ui_rx.recv().await, Some(UiEvent::Quit)),
            "expected quit event"
        );
    }

    #[tokio::test]
    async fn test_event_loop_permission_dialog_overlay_renders_and_denies_on_ctrl_c() {
        let (mut app, mut ui_rx, core_tx) = make_test_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");
        let (reply_tx, reply_rx) = oneshot::channel::<PermissionResponse>();

        core_tx
            .send(CoreEvent::PermissionAsk {
                tool_name: "bash".to_string(),
                input_summary: "echo hello".to_string(),
                prompt: "This command can modify files".to_string(),
                reply_tx,
            })
            .await
            .expect("send permission ask");

        // Pull the core event and render one frame while the permission dialog is active.
        let core_event = app
            .core_rx
            .recv()
            .await
            .expect("permission event delivered to app");
        app.handle_core_event(core_event);
        app.draw(&mut terminal)
            .expect("draw with permission dialog");

        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_permission_dialog_overlay", rendered.as_str());
        assert!(
            rendered.contains("Allow bash"),
            "permission dialog title should render"
        );
        assert!(
            rendered.contains("echo hello"),
            "permission dialog should include command preview"
        );

        // Ctrl+C while the dialog is open should deny the permission and request app quit.
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;

        let response = tokio::time::timeout(Duration::from_secs(1), reply_rx)
            .await
            .expect("permission response timeout")
            .expect("permission response channel closed");
        assert_eq!(
            response,
            PermissionResponse::Deny,
            "Ctrl+C on permission dialog should deny"
        );
        assert!(
            matches!(ui_rx.recv().await, Some(UiEvent::Quit)),
            "Ctrl+C should also emit quit"
        );
    }

    #[test]
    fn test_draw_snapshots_active_tool_lifecycle() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tu_1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "echo hello"}),
        });
        app.draw(&mut terminal).expect("draw with running tool");
        assert_snapshot!(
            "app_active_tool_running",
            normalized_rendered_text(&terminal).as_str()
        );

        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "hello".to_string(),
            is_error: false,
        });
        app.draw(&mut terminal)
            .expect("draw with completed tool result");
        assert_snapshot!(
            "app_active_tool_done",
            normalized_rendered_text(&terminal).as_str()
        );

        app.handle_core_event(CoreEvent::MessageComplete);
        app.draw(&mut terminal)
            .expect("draw after message complete");
        let after_complete = normalized_rendered_text(&terminal);
        assert_snapshot!("app_after_message_complete", after_complete.as_str());
        assert!(
            !after_complete.contains("[running]"),
            "active tool list should clear on message complete"
        );
    }

    #[test]
    fn test_scroll_up_from_auto_scroll_switches_to_manual_mode() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = true;
        app.max_scroll_offset = 120;
        app.scroll_offset = 0;

        app.scroll_up_by(1);

        assert!(!app.auto_scroll, "scrolling up should disable auto-scroll");
        assert_eq!(app.scroll_offset, 119);
    }

    #[test]
    fn test_scroll_down_to_bottom_reenables_auto_scroll() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 120;
        app.scroll_offset = 110;

        app.scroll_down_by(20);

        assert_eq!(app.scroll_offset, 120);
        assert!(app.auto_scroll, "reaching bottom should enable auto-scroll");
    }

    #[test]
    fn test_mouse_wheel_uses_message_scrolling() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = true;
        app.max_scroll_offset = 50;

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 47);
        assert!(!app.auto_scroll);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 50);
        assert!(app.auto_scroll);
    }

    #[test]
    fn test_scrollbar_click_jumps_to_position() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 0;
        // Message area: 80 cols wide, 40 rows tall at (0, 1).
        // Borders::ALL → inner track rows 2..40 (track_top=2, track_bottom=40).
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click on scrollbar col (79), at row 21.
        // relative_y = 21 - 2 = 19, track_height = 38, ratio = 19/37 ≈ 0.514 → offset ≈ 103
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 21,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.scroll_offset > 90 && app.scroll_offset < 115,
            "Expected ~103, got {}",
            app.scroll_offset
        );
        assert!(!app.auto_scroll);
    }

    #[test]
    fn test_scrollbar_click_at_bottom_enables_auto_scroll() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 0;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click at the very bottom of inner track (row 39 = track_bottom - 1).
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 39,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 200);
        assert!(app.auto_scroll);
    }

    #[test]
    fn test_scrollbar_click_on_border_ignored() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 50;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click on top border row (row 1 = area.y) — outside inner track.
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.scroll_offset, 50,
            "Click on top border should be ignored"
        );

        // Click on bottom border row (row 40 = area.bottom()) — outside inner track.
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 40,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.scroll_offset, 50,
            "Click on bottom border should be ignored"
        );
    }

    #[test]
    fn test_scrollbar_click_far_from_track_ignored() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 50;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click on col 77 (more than 1 col away from scrollbar) — ignored.
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 77,
            row: 20,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.scroll_offset, 50,
            "Click far from scrollbar should not change offset"
        );
    }

    #[test]
    fn test_scrollbar_drag_updates_position() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 0;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Drag on scrollbar at row 12.
        // relative_y = 12 - 2 = 10, track_height = 38, ratio = 10/37 ≈ 0.270 → offset ≈ 54
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 79,
            row: 12,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.scroll_offset > 40 && app.scroll_offset < 70,
            "Expected ~54, got {}",
            app.scroll_offset
        );
    }

    #[test]
    fn test_scrollbar_click_at_top_scrolls_to_zero() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 100;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click at very top of inner track (row 2 = track_top).
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 0, "Click at top should scroll to 0");
        assert!(!app.auto_scroll);
    }

    #[tokio::test]
    async fn test_pageup_key_scrolls_up_from_bottom() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = true;
        app.max_scroll_offset = 100;

        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
            .await;

        assert_eq!(app.scroll_offset, 80);
        assert!(!app.auto_scroll);
    }

    /// Helper that also returns the StateStore for pushing messages.
    fn make_test_app_with_store() -> (
        App,
        Arc<StateStore>,
        mpsc::Receiver<UiEvent>,
        mpsc::Sender<CoreEvent>,
    ) {
        let state_store = Arc::new(StateStore::default());
        let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
        let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(32);
        let app = App::new(&state_store, ui_tx, core_rx, Vec::new());
        (app, state_store, ui_rx, core_tx)
    }

    #[test]
    fn test_scroll_to_bottom_shows_last_line_with_many_messages() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Push enough messages to exceed the viewport (24 rows minus borders/status/input).
        for i in 1..=30 {
            store.push_message(Message::user(&format!("Message number {i}")));
            let mut reply = Message::assistant();
            reply.content.push(oxicode_common::ContentBlock::Text {
                text: format!("Reply to message {i}"),
            });
            store.push_message(reply);
        }

        // auto_scroll is true by default — draw should show the very last content.
        app.draw(&mut terminal).expect("draw succeeds");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_scroll_to_bottom_auto", rendered.as_str());

        // The last message ("Reply to message 30") must be visible somewhere.
        assert!(
            rendered.contains("Reply to message 30"),
            "Auto-scroll should show the last message. Rendered:\n{rendered}"
        );
    }

    #[test]
    fn test_manual_scroll_can_reach_bottom() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Push messages exceeding viewport.
        for i in 1..=20 {
            store.push_message(Message::user(&format!("Line {i}")));
            let mut reply = Message::assistant();
            reply.content.push(oxicode_common::ContentBlock::Text {
                text: format!("Answer {i}"),
            });
            store.push_message(reply);
        }

        // First draw to compute max_scroll_offset via the Rc<Cell> feedback.
        app.draw(&mut terminal).expect("initial draw");
        let max_scroll = app.max_scroll_offset;
        assert!(
            max_scroll > 0,
            "max_scroll_offset should be non-zero for overflow content"
        );

        // Disable auto_scroll and manually set to max.
        app.auto_scroll = false;
        app.scroll_offset = max_scroll;
        app.draw(&mut terminal).expect("draw at manual max scroll");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_scroll_manual_at_bottom", rendered.as_str());

        assert!(
            rendered.contains("Answer 20"),
            "Manual scroll to max should show last message. Rendered:\n{rendered}"
        );
    }

    #[test]
    fn test_scroll_top_shows_first_message() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        for i in 1..=20 {
            store.push_message(Message::user(&format!("Msg {i}")));
            let mut reply = Message::assistant();
            reply.content.push(oxicode_common::ContentBlock::Text {
                text: format!("Resp {i}"),
            });
            store.push_message(reply);
        }

        // First draw so max_scroll_offset is computed.
        app.draw(&mut terminal).expect("initial draw");

        // Scroll to top.
        app.auto_scroll = false;
        app.scroll_offset = 0;
        app.draw(&mut terminal).expect("draw at scroll offset 0");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_scroll_at_top", rendered.as_str());

        assert!(
            rendered.contains("Msg 1"),
            "Scroll offset 0 should show the first message. Rendered:\n{rendered}"
        );
    }

    #[test]
    fn test_scroll_with_wide_content_wrapping() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        // Narrow terminal to force wrapping (40 cols).
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Push a message with a long line that will wrap.
        store.push_message(Message::user("Short question"));
        let long_text = "A".repeat(200); // 200 chars will wrap in 40-col terminal
        let mut reply = Message::assistant();
        reply.content.push(oxicode_common::ContentBlock::Text {
            text: long_text.clone(),
        });
        store.push_message(reply);

        // Add a final short message after the wrapping one.
        store.push_message(Message::user("Final question"));
        let mut reply2 = Message::assistant();
        reply2.content.push(oxicode_common::ContentBlock::Text {
            text: "Final answer here".to_string(),
        });
        store.push_message(reply2);

        // Auto-scroll should land at the very bottom showing the final answer.
        app.draw(&mut terminal).expect("draw succeeds");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_scroll_wide_content_wrapping", rendered.as_str());

        assert!(
            rendered.contains("Final answer here"),
            "Auto-scroll with wrapped content should show last message. Rendered:\n{rendered}"
        );
    }

    #[test]
    fn test_streaming_thinking_indicator_snapshot() {
        let (mut app, _store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Simulate StreamStart: turn active, no text yet.
        app.handle_core_event(CoreEvent::StreamStart);
        app.draw(&mut terminal).expect("draw with thinking");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_streaming_thinking_indicator", rendered.as_str());

        assert!(
            rendered.contains("Thinking"),
            "Should show thinking indicator when streaming starts. Rendered:\n{rendered}"
        );
    }

    /// Full lifecycle: stream long content → MessageComplete → scroll up → scroll down.
    /// Full turn lifecycle: StreamStart → TextDelta* → StreamEnd → MessageComplete.
    /// Verifies state at each step with no leaks between turns.
    #[test]
    fn test_full_turn_lifecycle_state_transitions() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();

        // --- Turn 1 ---
        assert!(!app.is_turn_active);
        assert!(app.turn_started_at.is_none());

        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.is_turn_active, "StreamStart activates turn");
        assert!(app.turn_started_at.is_some(), "timer starts on StreamStart");
        assert!(
            app.streaming_text.is_empty(),
            "StreamStart clears streaming_text"
        );
        assert!(
            app.streaming_committed_lines.is_empty(),
            "StreamStart clears committed_lines"
        );
        assert!(
            app.active_tools.is_empty(),
            "StreamStart clears active_tools"
        );

        app.handle_core_event(CoreEvent::TextDelta("Hello".to_string()));
        assert_eq!(app.streaming_text, "Hello");

        app.handle_core_event(CoreEvent::TextDelta(", world!".to_string()));
        assert_eq!(app.streaming_text, "Hello, world!");

        app.handle_core_event(CoreEvent::StreamEnd);
        // After StreamEnd: raw buffer cleared, committed lines may contain finalized content.
        assert!(app.streaming_text.is_empty(), "StreamEnd clears raw buffer");
        assert!(
            app.is_turn_active,
            "StreamEnd does NOT deactivate turn (tools may still run)"
        );

        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(!app.is_turn_active, "MessageComplete deactivates turn");
        assert!(
            app.turn_started_at.is_none(),
            "MessageComplete resets timer"
        );
        assert!(app.streaming_text.is_empty());
        assert!(
            app.streaming_committed_lines.is_empty(),
            "MessageComplete clears committed_lines"
        );
        assert!(app.active_tools.is_empty());

        // --- Turn 2: verify StreamStart clears any lingering state ---
        app.handle_core_event(CoreEvent::TextDelta("leftover".to_string()));
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(
            app.streaming_text.is_empty(),
            "StreamStart clears stale text from prior turn"
        );
        assert!(
            app.streaming_committed_lines.is_empty(),
            "StreamStart clears stale lines"
        );
    }

    /// Verify that a StreamEnd received after Error is idempotent — does not
    /// re-activate the turn or corrupt state.
    #[test]
    fn test_stream_end_after_error_is_idempotent() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::TextDelta("partial text".to_string()));
        // abort_streaming() emits Error then TurnEnd (→ StreamEnd).
        app.handle_core_event(CoreEvent::Error("API error".to_string()));
        assert!(!app.is_turn_active);
        assert!(app.streaming_text.is_empty());

        // StreamEnd arrives after Error (from abort_streaming's TurnEnd).
        app.handle_core_event(CoreEvent::StreamEnd);
        // Must remain inactive and clean.
        assert!(
            !app.is_turn_active,
            "StreamEnd after Error must not re-activate turn"
        );
        assert!(app.streaming_text.is_empty());
    }

    /// submit_input must block (show notification) when a turn is active.
    #[tokio::test]
    async fn test_submit_input_blocked_during_active_turn() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        app.is_turn_active = true;
        app.input_text = "hello".to_string();

        let notif_before = app.notifications.len();
        app.submit_input().await;

        assert_eq!(
            app.notifications.len(),
            notif_before + 1,
            "submit while active should push a notification"
        );
        // No UiEvent should have been sent.
        assert!(
            ui_rx.try_recv().is_err(),
            "no UiEvent sent while turn is active"
        );
        // Input text must be preserved (not consumed).
        assert_eq!(app.input_text, "hello", "input not consumed on block");
    }

    /// History dedup: consecutive identical prompts stored only once.
    #[tokio::test]
    async fn test_submit_input_history_dedup() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();

        let initial_len = app.history.len();

        app.input_text = "cargo build".to_string();
        app.submit_input().await;

        // Submit same text again — should not be pushed again.
        app.input_text = "cargo build".to_string();
        app.submit_input().await;

        assert_eq!(
            app.history.len() - initial_len,
            1,
            "consecutive duplicates deduped"
        );
        // The last entry should be "cargo build".
        assert_eq!(app.history.get(app.history.len() - 1), Some("cargo build"));
    }

    /// /vim is handled inline — no UiEvent sent.
    #[tokio::test]
    async fn test_slash_vim_handled_inline_no_event() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        let was_vim = app.vim.enabled;

        app.input_text = "/vim".to_string();
        app.submit_input().await;

        assert_eq!(app.vim.enabled, !was_vim, "/vim toggles vim mode");
        assert!(ui_rx.try_recv().is_err(), "/vim must not send a UiEvent");
    }

    /// Non-/vim slash commands are forwarded as SlashCommand events.
    #[tokio::test]
    async fn test_slash_compact_forwarded_as_slash_command_event() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();

        app.input_text = "/compact".to_string();
        app.submit_input().await;

        match ui_rx.try_recv() {
            Ok(UiEvent::SlashCommand { name, args }) => {
                assert_eq!(name, "compact");
                assert_eq!(args, "");
            }
            other => panic!("Expected SlashCommand event, got {other:?}"),
        }
    }

    /// Unknown slash commands are forwarded (engine will produce an error).
    #[tokio::test]
    async fn test_slash_unknown_command_forwarded_not_swallowed() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();

        app.input_text = "/unknowncmd foo".to_string();
        app.submit_input().await;

        match ui_rx.try_recv() {
            Ok(UiEvent::SlashCommand { name, args }) => {
                assert_eq!(name, "unknowncmd");
                assert_eq!(args, "foo");
            }
            other => panic!("Expected SlashCommand forwarded, got {other:?}"),
        }
    }

    /// Permission dialog: 'y' hotkey sends AllowOnce and clears pending_permission.
    #[tokio::test]
    async fn test_permission_hotkey_y_sends_allow_once() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "echo hi".to_string(),
            prompt: "Allow?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "echo hi".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
            .await;

        assert!(
            app.pending_permission.is_none(),
            "'y' must clear pending_permission"
        );
        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::AllowOnce);
    }

    /// Permission dialog: 'n' hotkey sends Deny.
    #[tokio::test]
    async fn test_permission_hotkey_n_sends_deny() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "rm -rf /".to_string(),
            prompt: "Allow?".to_string(),
            selected: 2,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "rm -rf /".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .await;

        assert!(app.pending_permission.is_none());
        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::Deny);
    }

    /// Permission dialog: 'a' hotkey sends AlwaysAllow.
    #[tokio::test]
    async fn test_permission_hotkey_a_sends_always_allow() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "make build".to_string(),
            prompt: "Allow always?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "make build".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .await;

        assert!(app.pending_permission.is_none());
        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::AlwaysAllow);
    }

    /// Permission dialog: Esc sends Deny and clears dialog.
    #[tokio::test]
    async fn test_permission_esc_sends_deny() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "curl http://...".to_string(),
            prompt: "Network access?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "curl http://...".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;

        assert!(
            app.pending_permission.is_none(),
            "Esc clears pending_permission"
        );
        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::Deny, "Esc sends Deny");
    }

    /// Permission dialog: Enter on option 0 (AllowOnce) sends AllowOnce.
    #[tokio::test]
    async fn test_permission_enter_option_0_allow_once() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "echo".to_string(),
            prompt: "Allow?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "echo".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;

        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::AllowOnce);
    }

    /// Permission dialog blocks normal input: handle_key should not forward chars
    /// to input_text while a permission dialog is pending.
    #[tokio::test]
    async fn test_permission_dialog_blocks_normal_input() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "cmd".to_string(),
            prompt: "Allow?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "cmd".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        // Type a character — it should go to the permission handler, not input_text.
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .await;

        assert!(
            app.input_text.is_empty(),
            "input_text should not receive chars while permission dialog is open"
        );
    }

    /// Multiple sequential tool calls accumulate correctly in active_tools.
    #[test]
    fn test_multiple_sequential_tool_calls_accumulate() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);

        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "t1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "echo 1"}),
        });
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "t2".to_string(),
            name: "file_read".to_string(),
            input: serde_json::json!({"file_path": "/tmp/f.rs"}),
        });

        assert_eq!(app.active_tools.len(), 2);

        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "t1".to_string(),
            content: "1".to_string(),
            is_error: false,
        });
        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "t2".to_string(),
            content: "fn main() {}".to_string(),
            is_error: false,
        });

        assert_eq!(app.active_tools[0].result.as_ref().unwrap().0, "1");
        assert_eq!(
            app.active_tools[1].result.as_ref().unwrap().0,
            "fn main() {}"
        );

        // MessageComplete must clear all tools.
        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(
            app.active_tools.is_empty(),
            "active_tools cleared after MessageComplete"
        );
    }

    /// No state leaks between two full turns.
    #[test]
    fn test_no_state_leak_between_turns() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();

        // Turn 1 with a tool call.
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::TextDelta("First response".to_string()));
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tid1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        });
        app.handle_core_event(CoreEvent::StreamEnd);
        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tid1".to_string(),
            content: "file.txt".to_string(),
            is_error: false,
        });
        app.handle_core_event(CoreEvent::MessageComplete);

        // At start of Turn 2, StreamStart must wipe previous turn's tools/text.
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(
            app.streaming_text.is_empty(),
            "streaming_text must be empty at start of Turn 2"
        );
        assert!(
            app.active_tools.is_empty(),
            "active_tools must be empty at start of Turn 2 (cleared by StreamStart)"
        );
        assert!(
            app.streaming_committed_lines.is_empty(),
            "committed_lines cleared at start of Turn 2"
        );
    }

    /// Full lifecycle: stream long content → MessageComplete → scroll up → scroll down.
    #[test]
    fn test_full_stream_complete_then_scroll_lifecycle() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Push a user message to state store (simulates the engine adding it).
        store.push_message(Message::user("What can you do?"));

        // Simulate streaming a long response.
        app.handle_core_event(CoreEvent::StreamStart);
        let chunks = vec![
            "## Capabilities\n\n",
            "Here are my main capabilities:\n\n",
            "• **File Operations** — Read, write, edit files\n",
            "• **Code Analysis** — Find definitions, references\n",
            "• **Shell Commands** — Run bash/shell commands\n",
            "• **Web Search** — Search the web for information\n",
            "• **Project Management** — Create and track todos\n\n",
            "## Architecture\n\n",
            "| Layer | Description |\n",
            "|---|---|\n",
            "| Foundation | Shared types and errors |\n",
            "| Core | Query engine and tools |\n",
            "| TUI | Ratatui-based interface |\n",
            "| CLI | Binary entry point |\n\n",
            "## Additional Features\n\n",
            "• Session persistence\n",
            "• Hook system with 26 events\n",
            "• Vim mode keybindings\n",
            "• Multi-provider API support\n",
            "• Agent system for background tasks\n",
        ];

        for chunk in &chunks {
            app.handle_core_event(CoreEvent::TextDelta(chunk.to_string()));
        }

        // Draw during streaming — should show content auto-scrolled to bottom.
        app.draw(&mut terminal).expect("draw during streaming");
        let during_stream = normalized_rendered_text(&terminal);
        assert!(
            during_stream.contains("Agent system"),
            "During streaming, auto-scroll should show latest content"
        );

        // StreamEnd + MessageComplete.
        app.handle_core_event(CoreEvent::StreamEnd);

        // Push the assistant message to state store (simulates engine persisting).
        let full_text: String = chunks.iter().copied().collect();
        let mut assistant_msg = Message::assistant();
        assistant_msg
            .content
            .push(oxicode_common::ContentBlock::Text { text: full_text });
        store.push_message(assistant_msg);

        app.handle_core_event(CoreEvent::MessageComplete);

        // Draw after MessageComplete — should show cached message.
        app.draw(&mut terminal).expect("draw after complete");
        let after_complete = normalized_rendered_text(&terminal);
        assert!(
            after_complete.contains("Agent system") || after_complete.contains("background tasks"),
            "After MessageComplete, last content should be visible. Got:\n{after_complete}"
        );

        // Verify max_scroll_offset is positive (content exceeds viewport).
        assert!(
            app.max_scroll_offset > 0,
            "max_scroll_offset should be > 0 after long message, got {}",
            app.max_scroll_offset
        );

        // Scroll up — should disable auto_scroll and show earlier content.
        app.scroll_up_by(10);
        assert!(!app.auto_scroll, "scroll_up should disable auto_scroll");
        app.draw(&mut terminal).expect("draw after scroll up");
        let after_scroll_up = normalized_rendered_text(&terminal);
        // After scrolling up 10 lines, we should see earlier content.
        assert!(
            after_scroll_up.contains("Capabilities") || after_scroll_up.contains("File Operations"),
            "After scroll up, earlier content should be visible. Got:\n{after_scroll_up}"
        );

        // Scroll back down to bottom.
        app.scroll_down_by(100); // large number to reach bottom
        assert!(
            app.auto_scroll,
            "scrolling to bottom should re-enable auto_scroll"
        );
        app.draw(&mut terminal).expect("draw after scroll down");
        let after_scroll_down = normalized_rendered_text(&terminal);
        assert!(
            after_scroll_down.contains("background tasks") || after_scroll_down.contains("Agent system"),
            "After scroll down to bottom, last content should be visible. Got:\n{after_scroll_down}"
        );
    }
}
