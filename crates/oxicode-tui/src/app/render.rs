/// Rendering: draw() and all layout/render helpers.
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;

use crate::widgets::{
    AgentPanel, AgentsOverlay, CommandAutocomplete, DiffViewerWidget, HistorySearchWidget,
    InputBox, MessageView, ModelPicker, NotificationWidget, PastePreview, PermissionDialog,
    RewindOverlay, SearchBar, SessionBrowser, SettingsScreenWidget, StatsDashboard, SuggestionChips,
    TaskPanel,
};

// When the `virtual-scroll` feature is active, pull in the VirtualList path.
#[cfg(feature = "virtual-scroll")]
use crate::widgets::{
    message_view_virtual::{build_message_items, MessageRenderCtx},
    virtual_list::VirtualList,
};

use super::utils::{char_to_byte_index, detect_provider_from_model_name};

impl super::App {
    #[allow(clippy::too_many_lines)]
    pub fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> std::io::Result<()> {
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
        self.notifications
            .retain(crate::widgets::Notification::is_active);

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
            cost_usd,
        ) = {
            let state = self.state_rx.borrow();
            // Update render cache with new messages (incremental — only renders new ones).
            let term_width = terminal.size().map(|s| s.width).unwrap_or(80);
            self.message_cache.update(&state.messages, term_width);
            let msg_count = state.messages.len();
            let last_role = state.messages.last().map(|m| m.role);
            let msg_roles: Vec<oxicode_common::Role> =
                state.messages.iter().map(|m| m.role).collect();
            let agents: Vec<crate::widgets::AgentInfo> = state
                .active_agents
                .iter()
                .map(|a| crate::widgets::AgentInfo {
                    name: a.name.clone(),
                    status: a.status.clone(),
                    started_at: a.started_at.clone(),
                    duration: String::new(),
                    model: String::new(),
                    restricted_tools: Vec::new(),
                })
                .collect();
            let tasks: Vec<crate::widgets::TaskInfo> = state
                .background_tasks
                .iter()
                .map(|t| crate::widgets::TaskInfo {
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
                state.cost_tracker.total_cost(),
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
        let status_label = self.compose_status_label();

        terminal.draw(|frame| {
            let base_input_height = InputBox::required_height(&self.input_text);
            let input_height = if search_active {
                base_input_height + 2 // +2 for search bar
            } else {
                base_input_height
            };
            let suggestion_height: u16 = u16::from(show_suggestions);
            // Show "Press Ctrl-C again to exit" hint when active and not expired.
            let ctrl_c_hint = self.ctrl_c_hint_visible
                && self
                    .last_interrupt
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(2));
            let hint_height: u16 = u16::from(ctrl_c_hint);
            let queue_hint_height: u16 = u16::from(!self.message_queue.is_empty());
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),                   // Status bar
                    Constraint::Min(5),                      // Message view
                    Constraint::Length(autocomplete_height), // Command autocomplete (0 when inactive)
                    Constraint::Length(suggestion_height),   // Suggestion chips (0 or 1)
                    Constraint::Length(input_height),        // Input box (+ search bar)
                    Constraint::Length(queue_hint_height),   // Queue hint (0 or 1)
                    Constraint::Length(hint_height),         // Ctrl+C hint (0 or 1)
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
            let status_bar =
                crate::widgets::StatusBar::new(&current_model, &total_usage, is_streaming)
                    .with_provider(&provider_name)
                    .with_vim_badge(vim_badge)
                    .with_auth_label(&auth_label)
                    .with_context_pct(context_pct)
                    .with_permission_mode(&permission_mode)
                    .with_cwd(&cwd)
                    .with_session_start(Some(self.session_start))
                    .with_cost(cost_usd)
                    .with_retry_status(&status_label);
            frame.render_widget(status_bar, chunks[0]);

            // Content area — optionally split into left (messages) + right (agents/tasks)
            let content_area = chunks[1];
            let (left_area, right_area) = self.split_pane.split(content_area);
            // Store message area for scrollbar hit-testing in handle_mouse().
            self.message_area = left_area;

            // Message view (left pane, or full area when right pane is hidden)
            // Show streaming lines when turn is active OR when committed lines
            // exist (between StreamEnd and MessageComplete).
            //
            // Suppress streaming if the engine has already committed the response
            // to state (message_count > pre_streaming_msg_count). A 50ms tick draw
            // could otherwise show the same content from both the cache and the
            // streaming committed-lines buffer before MessageComplete fires.
            let streaming_suppressed =
                self.is_turn_active && message_count > self.pre_streaming_msg_count;
            let streaming_lines = if streaming_suppressed {
                None
            } else if !self.streaming_committed_lines.is_empty() {
                Some(self.streaming_committed_lines.as_slice())
            } else if self.is_turn_active {
                // Turn active but no lines yet — pass empty slice so thinking indicator shows.
                Some([].as_slice())
            } else {
                None
            };
            // Also suppress tail when streaming is suppressed — responses without
            // newlines live entirely in trailing_fragment() and would still
            // duplicate even when streaming_lines is None.
            let streaming_tail_owned: Option<String> =
                if self.is_turn_active && !streaming_suppressed {
                    self.streaming_collector
                        .trailing_fragment()
                        .map(std::string::ToString::to_string)
                } else {
                    None
                };
            let streaming_tail = streaming_tail_owned.as_deref();
            // Build active tools snapshot for streaming display.
            let active_tool_info: Vec<crate::widgets::ActiveToolInfo<'_>> = self
                .active_tools
                .iter()
                .map(|t| crate::widgets::ActiveToolInfo {
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
            .with_message_roles(message_roles.clone())
            .with_image_paths(&self.sent_image_paths)
            .with_tool_only_indices(self.message_cache.tool_only_indices());

            // Pass queued messages to render inside the conversation view.
            let queued_texts: Vec<&str> = (0..self.message_queue.len())
                .filter_map(|i| self.message_queue.peek_at(i).map(|m| m.text.as_str()))
                .collect();
            let message_view = message_view.with_queued_messages(queued_texts);

            frame.render_widget(message_view, left_area);

            // Virtual-scroll path: sync item list and render via VirtualList.
            // Runs in addition to the legacy path so parity can be verified.
            // When flipping the default, remove the legacy `message_view` above
            // and replace with just this block.
            #[cfg(feature = "virtual-scroll")]
            {
                let cached = self.message_cache.lines(message_count);
                let tool_only = self.message_cache.tool_only_indices();
                // Rebuild item vec from cache (incremental: push new items only).
                let current_len = self.virtual_list.len();
                if current_len > cached.len() {
                    // Cache shrank (e.g. /compact) — reset.
                    self.virtual_list = VirtualList::new();
                }
                // Bring VirtualList up to date with newly appended cache entries.
                let offset = self.virtual_list.len();
                let items_to_add = build_message_items(
                    &cached[offset..],
                    &message_roles,
                    tool_only,
                    offset,
                );
                for item in items_to_add {
                    self.virtual_list.push(item);
                }
                // Sync scroll state from App → VirtualList.
                if self.auto_scroll {
                    self.virtual_list.scroll_to_bottom();
                }
                // Render into inner area of the Block (inside the 1-cell border).
                let inner = ratatui::layout::Rect {
                    x: left_area.x + 1,
                    y: left_area.y + 1,
                    width: left_area.width.saturating_sub(2),
                    height: left_area.height.saturating_sub(2),
                };
                let vl_ctx = MessageRenderCtx { tool_only_indices: tool_only };
                // Note: `frame.buffer_mut()` is not directly accessible inside
                // the draw closure via the typed API; the legacy path via Widget
                // is the primary render. The VirtualList render is exercised in
                // the integration/parity tests instead of here in draw().
                // This block's primary purpose is keeping the scroll state sync'd.
                let _ = (inner, vl_ctx); // suppress unused-variable warnings
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
                let picker = ModelPicker::new(&self.model_picker);
                frame.render_widget(picker, content_area);
            }

            // Session browser overlay (drawn on top of content area).
            if self.session_browser.is_visible() {
                let browser = SessionBrowser::new(&self.session_browser);
                frame.render_widget(browser, content_area);
            }

            // Rewind overlay (interactive turn selector).
            if self.rewind_overlay.is_visible() {
                let rewind = RewindOverlay::new(&self.rewind_overlay);
                frame.render_widget(rewind, content_area);
            }

            // Stats dashboard overlay (session cost/token overview).
            if self.stats_dashboard.is_visible() {
                let state = self.state_rx.borrow();
                let elapsed = self.session_start.elapsed();
                let dashboard = StatsDashboard::new(
                    &self.stats_dashboard,
                    &state.cost_tracker,
                    &state.messages,
                    elapsed,
                );
                frame.render_widget(dashboard, content_area);
            }

            // Interactive diff viewer overlay (two-pane file diff review).
            if self.diff_viewer.is_visible() {
                let viewer = DiffViewerWidget::new(&self.diff_viewer);
                frame.render_widget(viewer, content_area);
            }

            // Agents overlay.
            if self.agents_overlay.is_visible() {
                frame.render_widget(
                    AgentsOverlay::new(&self.agents_overlay).with_frame(self.frame_count),
                    content_area,
                );
            }

            // Settings screen overlay (Ctrl+, or /settings).
            if let Some(ref screen) = self.pending_settings {
                frame.render_widget(SettingsScreenWidget::new(screen), content_area);
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

            // MCP elicitation overlay (rendered after permission so both can
            // co-exist in the state machine; key dispatch only routes to one).
            if let Some(ref pending) = self.pending_elicitation {
                frame.render_widget(&pending.dialog, content_area);
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

            // Queue hint below input box.
            if !self.message_queue.is_empty() {
                let muted = crate::render::CHROME_MUTED;
                let hint = ratatui::widgets::Paragraph::new(ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(
                        "  \u{276f} ",
                        ratatui::style::Style::default().fg(muted),
                    ),
                    ratatui::text::Span::styled(
                        "Press up to edit queued messages",
                        ratatui::style::Style::default()
                            .fg(muted)
                            .add_modifier(ratatui::style::Modifier::ITALIC),
                    ),
                ]));
                frame.render_widget(hint, chunks[5]);
            }

            // Ctrl+C exit hint below input box.
            if ctrl_c_hint {
                let hint = ratatui::widgets::Paragraph::new(ratatui::text::Line::from(vec![
                    ratatui::text::Span::raw("  "),
                    ratatui::text::Span::styled(
                        "Press Ctrl-C again to exit",
                        ratatui::style::Style::default().fg(crate::render::CHROME_MUTED),
                    ),
                ]));
                frame.render_widget(hint, chunks[6]);
            }
        })?;

        Ok(())
    }
}
