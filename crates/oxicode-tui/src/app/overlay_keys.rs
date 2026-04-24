/// Overlay and widget key handlers: mouse, paste, model picker, session browser,
/// rewind, stats dashboard, diff viewer, and keybinding action dispatch.
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

use crate::agent_editor;
use crate::keybindings::Action;
use crate::vim_mode;
use crate::widgets::agents_overlay::{AgentSource, GenerateStatus, RunningAgentRow};
use crate::widgets::{
    AgentsAgentRow, AgentsOrigin, AgentsRow, AgentsTab, Notification, SessionEntry,
};

use super::utils::{char_to_byte_index, format_relative_time};

use oxicode_agents::loader as agent_loader;

fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn build_agent_markdown(name: &str, description: &str, model: &str, prompt: &str) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {name}\n"));
    out.push_str(&format!("description: {}\n", yaml_escape(description)));
    if model != "default" {
        out.push_str(&format!("model: {model}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(prompt);
    if !prompt.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn yaml_escape(s: &str) -> String {
    // Quote if the value contains characters that could confuse a YAML scalar parser.
    let needs_quote =
        s.contains(':') || s.contains('#') || s.starts_with(['-', '?', '!', '&', '*']);
    if needs_quote {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

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
                let title = s
                    .title
                    .clone()
                    .or_else(|| s.preview.clone())
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

    pub(super) fn open_agents_overlay(&mut self) {
        let by_origin = agent_loader::refresh_with_origins();

        let mut rows: Vec<AgentsRow> = Vec::new();
        rows.push(AgentsRow::CreateNew);

        let mut project_names = std::collections::HashSet::new();
        for a in &by_origin.project_oxicode {
            project_names.insert(a.name.clone());
        }
        for a in &by_origin.project_claude {
            project_names.insert(a.name.clone());
        }

        let mut push_agent = |origin: AgentsOrigin,
                              source: AgentSource,
                              shadowed_by: Option<AgentsOrigin>,
                              a: oxicode_agents::loader::CustomAgent| {
            rows.push(AgentsRow::Agent(AgentsAgentRow {
                name: a.name,
                description: a.description,
                model: a.model,
                memory: None,
                origin,
                source,
                shadowed_by,
                source_path: a.source_path,
            }));
        };

        // Project agents first (.oxicode preferred, then legacy .claude).
        for a in by_origin.project_oxicode {
            push_agent(AgentsOrigin::Project, AgentSource::ProjectOxiCode, None, a);
        }
        for a in by_origin.project_claude {
            push_agent(AgentsOrigin::Project, AgentSource::ProjectClaude, None, a);
        }
        // User agents, dim if shadowed by any project agent with same name.
        for a in by_origin.user_oxicode {
            let shadow = project_names
                .contains(&a.name)
                .then_some(AgentsOrigin::Project);
            push_agent(AgentsOrigin::User, AgentSource::UserOxiCode, shadow, a);
        }
        for a in by_origin.user_claude {
            let shadow = project_names
                .contains(&a.name)
                .then_some(AgentsOrigin::Project);
            push_agent(AgentsOrigin::User, AgentSource::UserClaude, shadow, a);
        }

        // Running: snapshot active agents from state.
        let state = self.state_rx.borrow();
        let running: Vec<RunningAgentRow> = state
            .active_agents
            .iter()
            .map(|a| RunningAgentRow {
                name: a.name.clone(),
                status: a.status.clone(),
                started_at: a.started_at.clone(),
            })
            .collect();
        drop(state);

        self.agents_overlay.open(rows, running);
    }

    pub(super) async fn handle_agents_overlay_key(&mut self, key: KeyEvent) {
        use crate::widgets::agents_overlay::{
            AgentsOverlayMode, CreateAgentLocation, CreateAgentMethod,
        };

        // The CreateGenerate prompt is also a text-input field, but only while idle/error —
        // during in-flight generation we ignore typing and only honor Esc (to cancel).
        let is_text_input = matches!(
            self.agents_overlay.mode(),
            AgentsOverlayMode::CreateType { .. }
                | AgentsOverlayMode::CreateDescription { .. }
                | AgentsOverlayMode::CreatePrompt { .. }
        ) || matches!(
            self.agents_overlay.mode(),
            AgentsOverlayMode::CreateGenerate { status, .. }
                if !matches!(status, GenerateStatus::Generating)
        );

        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                // If a generation is in flight, Esc aborts it but keeps the wizard open.
                if let AgentsOverlayMode::CreateGenerate {
                    status: GenerateStatus::Generating,
                    ..
                } = self.agents_overlay.mode()
                {
                    if let Some(flag) = self.agent_gen_cancel.take() {
                        flag.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    self.agents_overlay
                        .mark_generate_error("Generation cancelled".to_string());
                } else {
                    self.agents_overlay.cancel();
                }
            }
            (_, KeyCode::Backspace) if is_text_input => {
                self.agents_overlay.wizard_backspace();
            }
            (_, KeyCode::Char('e'))
                if matches!(
                    self.agents_overlay.mode(),
                    AgentsOverlayMode::CreateConfirm { .. }
                ) =>
            {
                self.save_agent_from_wizard(true);
            }
            (_, KeyCode::Char(c)) if is_text_input => {
                self.agents_overlay.wizard_push_char(c);
            }
            (_, KeyCode::Left) => self.agents_overlay.prev_tab(),
            (_, KeyCode::Right) => self.agents_overlay.next_tab(),
            (_, KeyCode::Up) => match self.agents_overlay.mode() {
                AgentsOverlayMode::Browse => self.agents_overlay.select_prev(18),
                AgentsOverlayMode::CreateLocation | AgentsOverlayMode::CreateMethod { .. } => {
                    self.agents_overlay.wizard_prev(2);
                }
                AgentsOverlayMode::CreateModel { .. } => {
                    self.agents_overlay.wizard_prev(4);
                }
                _ => {}
            },
            (_, KeyCode::Down) => match self.agents_overlay.mode() {
                AgentsOverlayMode::Browse => self.agents_overlay.select_next(18),
                AgentsOverlayMode::CreateLocation | AgentsOverlayMode::CreateMethod { .. } => {
                    self.agents_overlay.wizard_next(2);
                }
                AgentsOverlayMode::CreateModel { .. } => {
                    self.agents_overlay.wizard_next(4);
                }
                _ => {}
            },
            (_, KeyCode::Enter) => {
                // Clone the current mode so we can mutate state while reading values.
                let mode = self.agents_overlay.mode().clone();
                match mode {
                    AgentsOverlayMode::Browse => {
                        match self.agents_overlay.active_tab() {
                            AgentsTab::Library => {
                                let Some(selected) = self.agents_overlay.selected().cloned() else {
                                    return;
                                };
                                match selected {
                                    AgentsRow::CreateNew => self.agents_overlay.start_create(),
                                    AgentsRow::Agent(a) => {
                                        self.agents_overlay.close();
                                        if let Err(e) = agent_editor::open_in_editor(&a.source_path)
                                        {
                                            self.notifications.push(Notification::new(
                                                format!("Failed to open editor: {e}"),
                                                crate::widgets::notification::NotificationLevel::Error,
                                            ));
                                            return;
                                        }
                                        self.open_agents_overlay();
                                    }
                                }
                            }
                            AgentsTab::Running => {} // v1: read-only
                        }
                    }
                    AgentsOverlayMode::CreateLocation => {
                        let location = if self.agents_overlay.wizard_selected_idx() == 0 {
                            CreateAgentLocation::Project
                        } else {
                            CreateAgentLocation::User
                        };
                        self.agents_overlay.set_create_method_step(location);
                    }
                    AgentsOverlayMode::CreateMethod { location } => {
                        let method = if self.agents_overlay.wizard_selected_idx() == 0 {
                            CreateAgentMethod::Generate
                        } else {
                            CreateAgentMethod::Manual
                        };
                        match method {
                            CreateAgentMethod::Generate => {
                                self.agents_overlay.set_create_generate_step(location);
                            }
                            CreateAgentMethod::Manual => {
                                self.agents_overlay.set_create_type_step(location, method);
                            }
                        }
                    }
                    AgentsOverlayMode::CreateGenerate { status, .. } => {
                        // Ignore Enter while a request is already in flight — prevents
                        // double-spawn and the orphaned cancel flag that comes with it.
                        if matches!(status, GenerateStatus::Generating) {
                            return;
                        }
                        let prompt = self.agents_overlay.wizard_input().trim().to_string();
                        if prompt.is_empty() {
                            self.notifications.push(Notification::new(
                                "Describe what the agent should do.".to_string(),
                                crate::widgets::notification::NotificationLevel::Warning,
                            ));
                            return;
                        }
                        self.spawn_agent_generation(prompt);
                    }
                    AgentsOverlayMode::CreateType {
                        location, method, ..
                    } => {
                        let slug = self.agents_overlay.wizard_input().trim().to_string();
                        if slug.is_empty() || !is_valid_slug(&slug) {
                            self.notifications.push(Notification::new(
                                "Type must be a non-empty slug (a-z, 0-9, - or _).".to_string(),
                                crate::widgets::notification::NotificationLevel::Warning,
                            ));
                            return;
                        }
                        self.agents_overlay
                            .set_create_description_step(location, method, slug);
                    }
                    AgentsOverlayMode::CreateDescription {
                        location,
                        method,
                        agent_type,
                        ..
                    } => {
                        let desc = self.agents_overlay.wizard_input().trim().to_string();
                        if desc.is_empty() {
                            self.notifications.push(Notification::new(
                                "Description cannot be empty.".to_string(),
                                crate::widgets::notification::NotificationLevel::Warning,
                            ));
                            return;
                        }
                        self.agents_overlay
                            .set_create_prompt_step(location, method, agent_type, desc);
                    }
                    AgentsOverlayMode::CreatePrompt {
                        location,
                        method,
                        agent_type,
                        description,
                        ..
                    } => {
                        let prompt = self.agents_overlay.wizard_input().trim().to_string();
                        if prompt.is_empty() {
                            self.notifications.push(Notification::new(
                                "System prompt cannot be empty.".to_string(),
                                crate::widgets::notification::NotificationLevel::Warning,
                            ));
                            return;
                        }
                        self.agents_overlay.set_create_model_step(
                            location,
                            method,
                            agent_type,
                            description,
                            prompt,
                        );
                    }
                    AgentsOverlayMode::CreateModel {
                        location,
                        method,
                        agent_type,
                        description,
                        prompt,
                        ..
                    } => {
                        let model = match self.agents_overlay.wizard_selected_idx() {
                            1 => "sonnet",
                            2 => "opus",
                            3 => "haiku",
                            _ => "default",
                        }
                        .to_string();
                        self.agents_overlay.set_create_confirm_step(
                            location,
                            method,
                            agent_type,
                            description,
                            prompt,
                            model,
                        );
                    }
                    AgentsOverlayMode::CreateConfirm { .. } => {
                        self.save_agent_from_wizard(false);
                    }
                }
            }
            _ => {}
        }
    }

    fn spawn_agent_generation(&mut self, prompt: String) {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        // Snapshot what existing identifiers we have so the model avoids collisions.
        let existing_ids: Vec<String> = self
            .agents_overlay
            .rows()
            .iter()
            .filter_map(|row| match row {
                crate::widgets::AgentsRow::Agent(a) => Some(a.name.clone()),
                _ => None,
            })
            .collect();

        let model = {
            let state = self.state_rx.borrow();
            state.current_model.clone()
        };

        // Move into Generating state and store wizard prompt.
        // `location` lives inside the CreateGenerate mode itself, so no need to thread it.
        self.agents_overlay.mark_generate_running();

        let cancel = Arc::new(AtomicBool::new(false));
        self.agent_gen_cancel = Some(Arc::clone(&cancel));

        let tx = self.agent_gen_tx.clone();
        tokio::spawn(async move {
            let result =
                oxicode_agents::generate_agent(&prompt, &model, &existing_ids, cancel).await;
            let msg = match result {
                Ok(g) => super::AgentGenerateMsg::Ok(g),
                Err(oxicode_agents::GenerateError::Cancelled) => return,
                Err(e) => super::AgentGenerateMsg::Err(e.to_string()),
            };
            let _ = tx.send(msg).await;
        });
    }

    pub(super) fn handle_agent_generate_msg(&mut self, msg: super::AgentGenerateMsg) {
        use crate::widgets::agents_overlay::{
            AgentsOverlayMode, CreateAgentMethod, GenerateStatus,
        };

        // If user has navigated away from CreateGenerate, drop the message.
        let Some((location, _)) = self.agents_overlay.current_generate_state() else {
            self.agent_gen_cancel = None;
            return;
        };
        // Only react if we're still in Generating; otherwise the user cancelled.
        if !matches!(
            self.agents_overlay.mode(),
            AgentsOverlayMode::CreateGenerate {
                status: GenerateStatus::Generating,
                ..
            }
        ) {
            self.agent_gen_cancel = None;
            return;
        }

        self.agent_gen_cancel = None;
        match msg {
            super::AgentGenerateMsg::Ok(g) => {
                // Reject anything we wouldn't accept from the manual flow — the
                // identifier becomes a filename, so guard against path traversal,
                // separators, and other unsafe characters.
                if !is_valid_slug(&g.identifier) {
                    self.agents_overlay.mark_generate_error(format!(
                        "Model returned an invalid identifier '{}' (must be a-z, 0-9, - or _). Try again.",
                        g.identifier
                    ));
                    return;
                }
                // Pre-fill prompt + identifier + description, jump straight to model select
                // (matches openclaude jumping to ToolsStep for the same reason — Tools UI
                // is out of scope v1, so we land on Model).
                self.agents_overlay.set_create_model_step(
                    location,
                    CreateAgentMethod::Generate,
                    g.identifier,
                    g.when_to_use,
                    g.system_prompt,
                );
                self.notifications.push(Notification::new(
                    "Generated agent — review the model and confirm.".to_string(),
                    crate::widgets::notification::NotificationLevel::Info,
                ));
            }
            super::AgentGenerateMsg::Err(e) => {
                self.agents_overlay.mark_generate_error(e);
            }
        }
    }

    fn save_agent_from_wizard(&mut self, open_editor: bool) {
        use crate::widgets::agents_overlay::{AgentsOverlayMode, CreateAgentLocation};

        let AgentsOverlayMode::CreateConfirm {
            location,
            agent_type,
            description,
            prompt,
            model,
            ..
        } = self.agents_overlay.mode().clone()
        else {
            return;
        };

        let dir = match location {
            CreateAgentLocation::Project => {
                std::env::current_dir().map(|c| c.join(".oxicode").join("agents"))
            }
            CreateAgentLocation::User => dirs::home_dir()
                .map(|h| h.join(".oxicode").join("agents"))
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home dir")),
        };
        let dir = match dir {
            Ok(d) => d,
            Err(e) => {
                self.notifications.push(Notification::new(
                    format!("Cannot resolve target directory: {e}"),
                    crate::widgets::notification::NotificationLevel::Error,
                ));
                return;
            }
        };

        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.notifications.push(Notification::new(
                format!("Failed to create {}: {e}", dir.display()),
                crate::widgets::notification::NotificationLevel::Error,
            ));
            return;
        }

        let path = dir.join(format!("{agent_type}.md"));
        let body = build_agent_markdown(&agent_type, &description, &model, &prompt);

        if let Err(e) = std::fs::write(&path, body) {
            self.notifications.push(Notification::new(
                format!("Failed to write {}: {e}", path.display()),
                crate::widgets::notification::NotificationLevel::Error,
            ));
            return;
        }

        self.notifications.push(Notification::new(
            format!("Created agent {} at {}", agent_type, path.display()),
            crate::widgets::notification::NotificationLevel::Info,
        ));

        if open_editor {
            if let Err(e) = agent_editor::open_in_editor(&path) {
                self.notifications.push(Notification::new(
                    format!("Failed to open editor: {e}"),
                    crate::widgets::notification::NotificationLevel::Warning,
                ));
            }
        }

        self.agents_overlay.set_browse_mode();
        self.open_agents_overlay();
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

    /// Open the branches overlay with branch data from the current session tree.
    pub(super) fn open_branches_overlay(&mut self) {
        use crate::widgets::BranchEntry;

        let session_id = self.state_rx.borrow().session_id.clone();
        let entries = match oxicode_session::branch::SessionTree::load_or_migrate(&session_id, None)
        {
            Ok(tree) => {
                // Build a depth map: root depth = 0, children = parent_depth + 1.
                let mut depth_map: std::collections::HashMap<uuid::Uuid, usize> =
                    std::collections::HashMap::new();

                // Collect and sort: root(s) first, then by created_at.
                let mut sorted: Vec<_> = tree.branches.values().collect();
                sorted.sort_by(|a, b| {
                    match (a.parent, b.parent) {
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        _ => a.created_at.cmp(&b.created_at),
                    }
                });

                // First pass: assign depths (process in sorted order so parents come first).
                for branch in &sorted {
                    let depth = branch
                        .parent
                        .and_then(|pid| depth_map.get(&pid))
                        .map(|d| d + 1)
                        .unwrap_or(0);
                    depth_map.insert(branch.id, depth);
                }

                sorted
                    .into_iter()
                    .map(|b| BranchEntry {
                        id: b.id,
                        is_current: b.id == tree.current,
                        title: b.title.clone(),
                        message_count: b.messages.len(),
                        parent_turn: b.parent_turn,
                        parent: b.parent,
                        depth: *depth_map.get(&b.id).unwrap_or(&0),
                    })
                    .collect()
            }
            Err(_) => Vec::new(),
        };

        if entries.is_empty() {
            self.notifications.push(Notification::new(
                "No branches found for this session. Use /fork to create one.".to_string(),
                crate::widgets::notification::NotificationLevel::Info,
            ));
            return;
        }

        self.branches_overlay.open(entries);
    }

    /// Handle key events when the branches overlay is active.
    pub(super) async fn handle_branches_overlay_key(&mut self, key: KeyEvent) {
        use crate::widgets::BranchesOverlayMode;

        match self.branches_overlay.mode().clone() {
            BranchesOverlayMode::Browse => match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => self.branches_overlay.cancel(),
                (_, KeyCode::Up) => self.branches_overlay.select_prev(),
                (_, KeyCode::Down) => self.branches_overlay.select_next(),
                (_, KeyCode::Char('r')) => self.branches_overlay.start_rename(),
                (_, KeyCode::Char('d')) => self.branches_overlay.start_delete_confirm(),
                (_, KeyCode::Enter) => {
                    if let Some(branch_id) = self.branches_overlay.confirm_switch() {
                        let branch_str = branch_id.to_string();
                        // Hot-swap: send branch switch as a slash command to the engine loop.
                        let _ = self
                            .ui_tx
                            .send(crate::events::UiEvent::SlashCommand {
                                name: "switch-branch".to_string(),
                                args: branch_str.clone(),
                            })
                            .await;
                        self.notifications.push(Notification::new(
                            format!(
                                "Switching to branch {}",
                                &branch_str[..8.min(branch_str.len())]
                            ),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                    }
                }
                _ => {}
            },
            BranchesOverlayMode::Rename => match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => self.branches_overlay.cancel(),
                (_, KeyCode::Enter) => {
                    if let Some((branch_id, new_title)) = self.branches_overlay.confirm_rename() {
                        self.notifications.push(Notification::new(
                            format!("Renamed branch to \"{new_title}\""),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                        let _ = self
                            .ui_tx
                            .send(crate::events::UiEvent::SlashCommand {
                                name: "rename-branch".to_string(),
                                args: format!("{branch_id} {new_title}"),
                            })
                            .await;
                    }
                }
                (_, KeyCode::Char(c)) => self.branches_overlay.push_rename_char(c),
                (_, KeyCode::Backspace) => self.branches_overlay.pop_rename_char(),
                _ => {}
            },
            BranchesOverlayMode::Confirm => match (key.modifiers, key.code) {
                (_, KeyCode::Esc) | (_, KeyCode::Char('n') | KeyCode::Char('N')) => {
                    self.branches_overlay.cancel();
                }
                (_, KeyCode::Char('y') | KeyCode::Char('Y')) => {
                    if let Some(branch_id) = self.branches_overlay.confirm_delete() {
                        self.notifications.push(Notification::new(
                            format!(
                                "Deleted branch {}",
                                &branch_id.to_string()[..8]
                            ),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                        let _ = self
                            .ui_tx
                            .send(crate::events::UiEvent::SlashCommand {
                                name: "delete-branch".to_string(),
                                args: branch_id.to_string(),
                            })
                            .await;
                    }
                }
                _ => {}
            },
        }
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

    // ── Settings screen ──────────────────────────────────────────────────────

    /// Open the settings overlay, snapshotting current config into memory.
    ///
    /// Blocked when the agents overlay or permission/elicitation dialogs are
    /// visible — those take priority and cannot coexist with settings.
    pub(super) fn open_settings_screen(&mut self) {
        if self.agents_overlay.is_visible()
            || self.pending_permission.is_some()
            || self.pending_elicitation.is_some()
        {
            return;
        }
        let settings = oxicode_config::load_settings(None);
        self.pending_settings = Some(crate::widgets::SettingsScreen::from_config(&settings));
    }

    /// Handle key events while the settings overlay is open.
    pub(super) fn handle_settings_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        let Some(screen) = self.pending_settings.as_mut() else {
            return;
        };

        // Provider edit dialog gets priority: Tab switches field, Ctrl+R reveals,
        // Enter commits, Esc cancels.
        if screen.is_providers_editing() {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => screen.cancel_provider_edit(),
                (_, KeyCode::Enter) => screen.commit_provider_edit(),
                (_, KeyCode::Tab) => screen.toggle_provider_field(),
                (KeyModifiers::CONTROL, KeyCode::Char('r')) => screen.toggle_reveal(),
                (_, KeyCode::Backspace) => screen.pop_char(),
                (_, KeyCode::Char(c)) => screen.push_char(c),
                _ => {}
            }
            return;
        }

        match (key.modifiers, key.code) {
            // Close overlay.
            (_, KeyCode::Esc) => {
                self.pending_settings = None;
            }
            // Save to disk.
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                match screen.save() {
                    Ok(()) => {
                        // Propagate changed model + permission_mode into the live
                        // StateStore via the engine event loop (spec task 6).
                        let model = screen.general.model.clone();
                        let permission_mode =
                            screen.permissions.mode.as_str().to_string();
                        let _ = self.ui_tx.try_send(crate::events::UiEvent::SettingsSaved {
                            model,
                            permission_mode,
                        });
                        self.notifications.push(crate::widgets::Notification::new(
                            "Settings saved to ~/.oxicode/settings.toml".to_string(),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                    }
                    Err(e) => {
                        self.notifications.push(crate::widgets::Notification::new(
                            format!("Save failed: {e}"),
                            crate::widgets::notification::NotificationLevel::Error,
                        ));
                    }
                }
            }
            // Tab cycle (forward / backward).
            (_, KeyCode::Tab) | (_, KeyCode::Right) => screen.next_tab(),
            (KeyModifiers::SHIFT, KeyCode::BackTab) | (_, KeyCode::Left) => screen.prev_tab(),
            // Navigation within the active tab.
            (_, KeyCode::Down) => screen.select_next(),
            (_, KeyCode::Up) => screen.select_prev(),
            // Activate / toggle / open-edit for the focused element.
            (_, KeyCode::Enter) => screen.activate(),
            // Text input (for editable fields like model name, allow/deny lists).
            (_, KeyCode::Backspace) => screen.pop_char(),
            (_, KeyCode::Char(c)) => screen.push_char(c),
            _ => {}
        }
    }
}
