use std::fmt::Write as _;
use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::modal_helpers::{
    begin_modal, render_separator, DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT, PANEL_BG,
};

/// Top-level tabs in the agents overlay. "Agents" is the modal title (always
/// shown); inside, the user toggles between the live runtime view and the
/// installed-agents library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsTab {
    Running,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsOrigin {
    Project,
    User,
}

/// Fine-grained source for an agent definition. Drives both the row metadata
/// and the section header in the Library tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentSource {
    ProjectOxiCode,
    ProjectClaude,
    UserOxiCode,
    UserClaude,
}

impl AgentSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProjectOxiCode => "Project agents (.oxicode)",
            Self::ProjectClaude => "Project agents (.claude, legacy)",
            Self::UserOxiCode => "User agents (.oxicode)",
            Self::UserClaude => "User agents (.claude, legacy)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentsAgentRow {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub memory: Option<String>,
    pub origin: AgentsOrigin,
    pub source: AgentSource,
    pub shadowed_by: Option<AgentsOrigin>,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum AgentsRow {
    CreateNew,
    Agent(AgentsAgentRow),
}

#[derive(Debug, Clone)]
pub struct RunningAgentRow {
    pub name: String,
    pub status: String,
    pub started_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateAgentLocation {
    Project,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateAgentMethod {
    Generate,
    Manual,
}

/// State of an in-flight LLM-backed agent generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateStatus {
    /// User is composing the description.
    Idle,
    /// Request is in flight; spinner is shown.
    Generating,
    /// Last attempt failed with this message; user can retry or Esc back.
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsOverlayMode {
    Browse,
    CreateLocation,
    CreateMethod {
        location: CreateAgentLocation,
    },
    /// AI-driven generation step. User describes the agent, we ask the model
    /// for a JSON config, then jump straight to the model selector.
    CreateGenerate {
        location: CreateAgentLocation,
        prompt: String,
        status: GenerateStatus,
    },
    CreateType {
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
    },
    CreateDescription {
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
        description: String,
    },
    CreatePrompt {
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
        description: String,
        prompt: String,
    },
    CreateModel {
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
        description: String,
        prompt: String,
        model: String,
    },
    CreateConfirm {
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
        description: String,
        prompt: String,
        model: String,
    },
}

pub struct AgentsOverlayState {
    visible: bool,
    active_tab: AgentsTab,
    mode: AgentsOverlayMode,
    selected_idx: usize,
    scroll_offset: u16,
    rows: Vec<AgentsRow>,
    running: Vec<RunningAgentRow>,
    wizard_selected_idx: usize,
    wizard_input: String,
    wizard_cursor: usize,
}

impl AgentsOverlayState {
    pub fn new() -> Self {
        Self {
            visible: false,
            active_tab: AgentsTab::Library,
            mode: AgentsOverlayMode::Browse,
            selected_idx: 0,
            scroll_offset: 0,
            rows: Vec::new(),
            running: Vec::new(),
            wizard_selected_idx: 0,
            wizard_input: String::new(),
            wizard_cursor: 0,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn open(&mut self, rows: Vec<AgentsRow>, running: Vec<RunningAgentRow>) {
        self.visible = true;
        self.active_tab = AgentsTab::Library;
        self.mode = AgentsOverlayMode::Browse;
        self.selected_idx = 0;
        self.scroll_offset = 0;
        self.rows = rows;
        self.running = running;
        self.wizard_selected_idx = 0;
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.mode = AgentsOverlayMode::Browse;
        self.selected_idx = 0;
        self.scroll_offset = 0;
        self.rows.clear();
        self.running.clear();
        self.wizard_selected_idx = 0;
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    pub fn running(&self) -> &[RunningAgentRow] {
        &self.running
    }

    pub fn next_tab(&mut self) {
        if self.mode != AgentsOverlayMode::Browse {
            return;
        }
        self.active_tab = match self.active_tab {
            AgentsTab::Library => AgentsTab::Running,
            AgentsTab::Running => AgentsTab::Library,
        };
        self.selected_idx = 0;
        self.scroll_offset = 0;
    }

    pub fn prev_tab(&mut self) {
        // With only two tabs, prev == next.
        self.next_tab();
    }

    fn active_tab_len(&self) -> usize {
        match self.active_tab {
            AgentsTab::Library => self.rows.len(),
            AgentsTab::Running => self.running.len(),
        }
    }

    pub fn select_prev(&mut self, view_height: u16) {
        if self.mode != AgentsOverlayMode::Browse {
            return;
        }
        let count = self.active_tab_len();
        if count == 0 {
            return;
        }
        if self.selected_idx == 0 {
            self.selected_idx = count - 1;
        } else {
            self.selected_idx -= 1;
        }
        self.ensure_selected_visible(view_height);
    }

    pub fn select_next(&mut self, view_height: u16) {
        if self.mode != AgentsOverlayMode::Browse {
            return;
        }
        let count = self.active_tab_len();
        if count == 0 {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % count;
        self.ensure_selected_visible(view_height);
    }

    pub fn selected(&self) -> Option<&AgentsRow> {
        match self.active_tab {
            AgentsTab::Library => self.rows.get(self.selected_idx),
            AgentsTab::Running => None, // Running rows aren't AgentsRow; v1 read-only.
        }
    }

    fn ensure_selected_visible(&mut self, view_height: u16) {
        let selected = self.selected_idx as u16;
        if selected < self.scroll_offset {
            self.scroll_offset = selected;
            return;
        }
        if view_height == 0 {
            return;
        }
        let bottom = self
            .scroll_offset
            .saturating_add(view_height.saturating_sub(1));
        if selected > bottom {
            self.scroll_offset = selected.saturating_sub(view_height.saturating_sub(1));
        }
    }

    pub fn active_tab(&self) -> AgentsTab {
        self.active_tab
    }

    pub fn mode(&self) -> &AgentsOverlayMode {
        &self.mode
    }

    pub fn start_create(&mut self) {
        self.mode = AgentsOverlayMode::CreateLocation;
        self.wizard_selected_idx = 0;
        self.wizard_input.clear();
        self.wizard_cursor = 0;
        self.selected_idx = 0;
        self.scroll_offset = 0;
    }

    pub fn cancel(&mut self) {
        if self.mode == AgentsOverlayMode::Browse {
            self.close();
        } else {
            self.mode = AgentsOverlayMode::Browse;
            self.wizard_selected_idx = 0;
            self.wizard_input.clear();
            self.wizard_cursor = 0;
            self.selected_idx = 0;
            self.scroll_offset = 0;
        }
    }

    pub fn wizard_prev(&mut self, count: usize) {
        if count == 0 {
            self.wizard_selected_idx = 0;
            return;
        }
        if self.wizard_selected_idx == 0 {
            self.wizard_selected_idx = count - 1;
        } else {
            self.wizard_selected_idx -= 1;
        }
    }

    pub fn wizard_next(&mut self, count: usize) {
        if count == 0 {
            self.wizard_selected_idx = 0;
            return;
        }
        self.wizard_selected_idx = (self.wizard_selected_idx + 1) % count;
    }

    pub fn wizard_selected_idx(&self) -> usize {
        self.wizard_selected_idx
    }

    pub fn set_create_method_step(&mut self, location: CreateAgentLocation) {
        self.mode = AgentsOverlayMode::CreateMethod { location };
        self.wizard_selected_idx = 0;
    }

    /// Enter the AI-generate flow with an empty prompt.
    pub fn set_create_generate_step(&mut self, location: CreateAgentLocation) {
        self.mode = AgentsOverlayMode::CreateGenerate {
            location,
            prompt: String::new(),
            status: GenerateStatus::Idle,
        };
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    /// Mark the current generate step as in-flight and snapshot the prompt the
    /// async task is using.
    pub fn mark_generate_running(&mut self) {
        if let AgentsOverlayMode::CreateGenerate { status, prompt, .. } = &mut self.mode {
            self.wizard_input.clone_into(prompt);
            *status = GenerateStatus::Generating;
        }
    }

    /// Surface a generation error and let the user retry.
    pub fn mark_generate_error(&mut self, message: String) {
        if let AgentsOverlayMode::CreateGenerate { status, .. } = &mut self.mode {
            *status = GenerateStatus::Error(message);
        }
    }

    /// Read the current generate step (location + prompt) without mutating.
    pub fn current_generate_state(&self) -> Option<(CreateAgentLocation, String)> {
        if let AgentsOverlayMode::CreateGenerate {
            location, prompt, ..
        } = &self.mode
        {
            Some((*location, prompt.clone()))
        } else {
            None
        }
    }

    pub fn set_create_type_step(
        &mut self,
        location: CreateAgentLocation,
        method: CreateAgentMethod,
    ) {
        self.mode = AgentsOverlayMode::CreateType {
            location,
            method,
            agent_type: String::new(),
        };
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    pub fn set_create_description_step(
        &mut self,
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
    ) {
        self.mode = AgentsOverlayMode::CreateDescription {
            location,
            method,
            agent_type,
            description: String::new(),
        };
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    pub fn set_create_prompt_step(
        &mut self,
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
        description: String,
    ) {
        self.mode = AgentsOverlayMode::CreatePrompt {
            location,
            method,
            agent_type,
            description,
            prompt: String::new(),
        };
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    pub fn set_create_model_step(
        &mut self,
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
        description: String,
        prompt: String,
    ) {
        self.mode = AgentsOverlayMode::CreateModel {
            location,
            method,
            agent_type,
            description,
            prompt,
            model: String::new(),
        };
        self.wizard_selected_idx = 0;
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    pub fn set_create_confirm_step(
        &mut self,
        location: CreateAgentLocation,
        method: CreateAgentMethod,
        agent_type: String,
        description: String,
        prompt: String,
        model: String,
    ) {
        self.mode = AgentsOverlayMode::CreateConfirm {
            location,
            method,
            agent_type,
            description,
            prompt,
            model,
        };
        self.wizard_selected_idx = 0;
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    pub fn set_browse_mode(&mut self) {
        self.mode = AgentsOverlayMode::Browse;
        self.wizard_selected_idx = 0;
        self.wizard_input.clear();
        self.wizard_cursor = 0;
    }

    pub fn wizard_push_char(&mut self, c: char) {
        let byte_idx =
            crate::app::utils::char_to_byte_index(&self.wizard_input, self.wizard_cursor);
        self.wizard_input.insert(byte_idx, c);
        self.wizard_cursor += 1;
    }

    pub fn wizard_backspace(&mut self) {
        if self.wizard_cursor == 0 {
            return;
        }
        self.wizard_cursor -= 1;
        let start = crate::app::utils::char_to_byte_index(&self.wizard_input, self.wizard_cursor);
        let end = crate::app::utils::char_to_byte_index(&self.wizard_input, self.wizard_cursor + 1);
        self.wizard_input.replace_range(start..end, "");
    }

    pub fn wizard_input(&self) -> &str {
        &self.wizard_input
    }

    pub fn wizard_cursor(&self) -> usize {
        self.wizard_cursor
    }

    pub fn rows(&self) -> &[AgentsRow] {
        &self.rows
    }

    pub fn scroll_offset(&self) -> u16 {
        self.scroll_offset
    }

    pub fn selected_idx(&self) -> usize {
        self.selected_idx
    }
}

impl Default for AgentsOverlayState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AgentsOverlay<'a> {
    state: &'a AgentsOverlayState,
    /// Monotonic frame counter, used to drive the generation-spinner animation.
    frame: u64,
}

impl<'a> AgentsOverlay<'a> {
    pub fn new(state: &'a AgentsOverlayState) -> Self {
        Self { state, frame: 0 }
    }

    pub fn with_frame(mut self, frame: u64) -> Self {
        self.frame = frame;
        self
    }
}

const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn render_wizard_choice(
    buf: &mut Buffer,
    area: Rect,
    title: &str,
    subtitle: &str,
    options: &[&str],
    selected_idx: usize,
) {
    let header_lines = [
        Line::from(vec![Span::styled(
            title.to_string(),
            Style::default()
                .fg(DIALOG_TEXT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            subtitle.to_string(),
            Style::default().fg(DIALOG_MUTED),
        )]),
        Line::from(""),
    ];

    for (i, line) in header_lines.iter().enumerate() {
        let y = area.y + i as u16;
        if y >= area.bottom() {
            break;
        }
        buf.set_line(area.x, y, line, area.width);
    }

    for (i, label) in options.iter().enumerate() {
        let y = area.y + 3 + i as u16;
        if y >= area.bottom() {
            break;
        }
        let selected = selected_idx == i;
        let bullet = if selected { "▸" } else { " " };
        let style = if selected {
            Style::default()
                .fg(DIALOG_TEXT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIALOG_MUTED)
        };
        let line = Line::from(vec![
            Span::styled(format!(" {bullet} "), Style::default().fg(DIALOG_MUTED)),
            Span::styled((*label).to_string(), style),
        ]);
        buf.set_line(area.x, y, &line, area.width);
    }
}

fn render_wizard_input(
    buf: &mut Buffer,
    area: Rect,
    title: &str,
    subtitle: &str,
    placeholder: &str,
    value: &str,
    cursor: usize,
) {
    let header_lines = [
        Line::from(vec![Span::styled(
            title.to_string(),
            Style::default()
                .fg(DIALOG_TEXT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            subtitle.to_string(),
            Style::default().fg(DIALOG_MUTED),
        )]),
        Line::from(""),
    ];

    for (i, line) in header_lines.iter().enumerate() {
        let y = area.y + i as u16;
        if y >= area.bottom() {
            break;
        }
        buf.set_line(area.x, y, line, area.width);
    }

    let display = if value.is_empty() { placeholder } else { value };
    let style = if value.is_empty() {
        Style::default().fg(DIALOG_MUTED)
    } else {
        Style::default().fg(DIALOG_TEXT)
    };

    let y = area.y + 3;
    if y < area.bottom() {
        let line = Line::from(vec![
            Span::styled(" ".to_string(), Style::default()),
            Span::styled(display.to_string(), style),
        ]);
        buf.set_line(area.x, y, &line, area.width);

        if !value.is_empty() {
            let x = area.x + 1 + cursor as u16;
            if x < area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(
                        Style::default()
                            .fg(ratatui::style::Color::Black)
                            .bg(DIALOG_TEXT)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
        }
    }
}

fn render_running_list(buf: &mut Buffer, area: Rect, state: &AgentsOverlayState) {
    if state.running.is_empty() {
        let line = Line::from(vec![Span::styled(
            "  No running agents.".to_string(),
            Style::default().fg(DIALOG_MUTED),
        )]);
        buf.set_line(area.x, area.y, &line, area.width);
        return;
    }

    let visible_rows = area.height as usize;
    let start = state.scroll_offset as usize;

    for (row, a) in state
        .running
        .iter()
        .skip(start)
        .take(visible_rows)
        .enumerate()
    {
        let y = area.y + row as u16;
        if y >= area.bottom() {
            break;
        }
        let idx = start + row;
        let selected = idx == state.selected_idx;

        let title_style = if selected {
            Style::default()
                .fg(DIALOG_ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIALOG_TEXT)
        };

        let meta = format!(" · {} · started {}", a.status, a.started_at);
        let line = Line::from(vec![
            Span::raw("  "),
            Span::styled(a.name.clone(), title_style),
            Span::styled(meta, Style::default().fg(DIALOG_MUTED)),
        ]);
        buf.set_line(area.x, y, &line, area.width);
    }
}

/// Display path with `~` shortcut for the user's home dir.
fn display_path(path: &std::path::Path) -> String {
    let parent = path.parent().unwrap_or(path);
    let s = parent.to_string_lossy().to_string();
    if let Ok(home) = std::env::var("HOME") {
        if let Some(stripped) = s.strip_prefix(&home) {
            return format!("~{stripped}");
        }
    }
    s
}

// Flat "visual line plan" for render_library_list: each entry is either a header
// (not selectable) or a row index into `state.rows` (selectable).
enum VisualLine<'a> {
    Header { label: &'a str, path: String },
    Row(usize, &'a AgentsRow),
    Spacer,
}

/// Library tab — Create-new-agent row at top, then agents grouped by source
/// (Project .oxicode → Project .claude → User .oxicode → User .claude).
/// Headers are rendered as plain visual rows; only `AgentsRow` entries are
/// selectable, so headers do not consume an index in `selected_idx`.
#[allow(clippy::too_many_lines)]
fn render_library_list(buf: &mut Buffer, area: Rect, state: &AgentsOverlayState) {
    if state.rows.is_empty() {
        let line = Line::from(vec![Span::styled(
            "  No agents found.".to_string(),
            Style::default().fg(DIALOG_MUTED),
        )]);
        buf.set_line(area.x, area.y, &line, area.width);
        return;
    }

    let mut plan: Vec<VisualLine> = Vec::with_capacity(state.rows.len() + 8);

    // CreateNew always pinned at the top, no header.
    let mut current_source: Option<AgentSource> = None;
    let mut last_was_row = false;
    for (idx, row) in state.rows.iter().enumerate() {
        match row {
            AgentsRow::CreateNew => {
                plan.push(VisualLine::Row(idx, row));
                last_was_row = true;
            }
            AgentsRow::Agent(a) => {
                if current_source != Some(a.source) {
                    if last_was_row {
                        plan.push(VisualLine::Spacer);
                    }
                    plan.push(VisualLine::Header {
                        label: a.source.label(),
                        path: display_path(&a.source_path),
                    });
                    current_source = Some(a.source);
                }
                plan.push(VisualLine::Row(idx, row));
                last_was_row = true;
            }
        }
    }

    // Resolve scroll offset (driven by selectable selected_idx). Find the
    // visual line index of the selected row, then keep it in view.
    let visible_rows = area.height as usize;
    let selected_visual = plan
        .iter()
        .position(|v| matches!(v, VisualLine::Row(i, _) if *i == state.selected_idx))
        .unwrap_or(0);
    let scroll_off = state.scroll_offset as usize;
    let start = if selected_visual < scroll_off {
        selected_visual
    } else if selected_visual >= scroll_off + visible_rows {
        selected_visual + 1 - visible_rows
    } else {
        scroll_off
    };

    for (vrow, line) in plan.iter().skip(start).take(visible_rows).enumerate() {
        let y = area.y + vrow as u16;
        if y >= area.bottom() {
            break;
        }
        match line {
            VisualLine::Spacer => {}
            VisualLine::Header { label, path } => {
                let l = Line::from(vec![
                    Span::styled(
                        format!("  {label}"),
                        Style::default()
                            .fg(DIALOG_TEXT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" ({path})"), Style::default().fg(DIALOG_MUTED)),
                ]);
                buf.set_line(area.x, y, &l, area.width);
            }
            VisualLine::Row(idx, row) => {
                let selected = *idx == state.selected_idx;

                let (title, meta, dimmed) = match row {
                    AgentsRow::CreateNew => ("Create new agent".to_string(), String::new(), false),
                    AgentsRow::Agent(a) => {
                        let mut m = String::new();
                        if let Some(model) = &a.model {
                            let _ = write!(m, " · {model}");
                        } else {
                            m.push_str(" · inherit");
                        }
                        if let Some(mem) = &a.memory {
                            let _ = write!(m, " · {mem} memory");
                        }
                        if let Some(shadow) = a.shadowed_by {
                            let by = match shadow {
                                AgentsOrigin::Project => "project",
                                AgentsOrigin::User => "user",
                            };
                            let _ = write!(m, "  ⚠ shadowed by {by}");
                        }
                        (a.name.clone(), m, a.shadowed_by.is_some())
                    }
                };

                let title_style = if selected {
                    Style::default()
                        .fg(DIALOG_ACCENT)
                        .add_modifier(Modifier::BOLD)
                } else if dimmed {
                    Style::default().fg(DIALOG_MUTED)
                } else {
                    Style::default().fg(DIALOG_TEXT)
                };

                let l = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(title, title_style),
                    Span::styled(meta, Style::default().fg(DIALOG_MUTED)),
                ]);
                buf.set_line(area.x, y, &l, area.width);
            }
        }
    }
}

fn render_generate_step(
    buf: &mut Buffer,
    area: Rect,
    value: &str,
    cursor: usize,
    status: &GenerateStatus,
    frame: u64,
) {
    let header_lines = [
        Line::from(vec![Span::styled(
            "Create new agent".to_string(),
            Style::default()
                .fg(DIALOG_TEXT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            "Describe what this agent should do (be comprehensive for best results)".to_string(),
            Style::default().fg(DIALOG_MUTED),
        )]),
        Line::from(""),
    ];
    for (i, line) in header_lines.iter().enumerate() {
        let y = area.y + i as u16;
        if y >= area.bottom() {
            break;
        }
        buf.set_line(area.x, y, line, area.width);
    }

    let display = if value.is_empty() {
        "e.g., Help me write unit tests for my code..."
    } else {
        value
    };
    let style = if value.is_empty() {
        Style::default().fg(DIALOG_MUTED)
    } else {
        Style::default().fg(DIALOG_TEXT)
    };

    let input_y = area.y + 3;
    if input_y < area.bottom() {
        let line = Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(display.to_string(), style),
        ]);
        buf.set_line(area.x, input_y, &line, area.width);
        if !value.is_empty() && matches!(status, GenerateStatus::Idle | GenerateStatus::Error(_)) {
            let x = area.x + 1 + cursor as u16;
            if x < area.right() {
                if let Some(cell) = buf.cell_mut((x, input_y)) {
                    cell.set_style(
                        Style::default()
                            .fg(ratatui::style::Color::Black)
                            .bg(DIALOG_TEXT)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
        }
    }

    let status_y = area.y + 5;
    if status_y >= area.bottom() {
        return;
    }
    match status {
        GenerateStatus::Idle => {}
        GenerateStatus::Generating => {
            #[allow(clippy::cast_possible_truncation)]
            let glyph = SPINNER_FRAMES[(frame as usize) % SPINNER_FRAMES.len()];
            let line = Line::from(vec![Span::styled(
                format!(" {glyph} Generating agent from description..."),
                Style::default().fg(DIALOG_TEXT),
            )]);
            buf.set_line(area.x, status_y, &line, area.width);
        }
        GenerateStatus::Error(msg) => {
            let line = Line::from(vec![Span::styled(
                format!(" ✗ {msg}"),
                Style::default().fg(ratatui::style::Color::Red),
            )]);
            buf.set_line(area.x, status_y, &line, area.width);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn render_confirm_step(
    buf: &mut Buffer,
    area: Rect,
    location: CreateAgentLocation,
    agent_type: &str,
    description: &str,
    model: &str,
) {
    let loc_label = match location {
        CreateAgentLocation::Project => "Project (.oxicode/agents/)",
        CreateAgentLocation::User => "Personal (~/.oxicode/agents/)",
    };

    let lines = [
        Line::from(vec![Span::styled(
            "Create new agent".to_string(),
            Style::default()
                .fg(DIALOG_TEXT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            "Review and confirm".to_string(),
            Style::default().fg(DIALOG_MUTED),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Location ", Style::default().fg(DIALOG_MUTED)),
            Span::styled(loc_label.to_string(), Style::default().fg(DIALOG_TEXT)),
        ]),
        Line::from(vec![
            Span::styled(" Type     ", Style::default().fg(DIALOG_MUTED)),
            Span::styled(agent_type.to_string(), Style::default().fg(DIALOG_TEXT)),
        ]),
        Line::from(vec![
            Span::styled(" Desc     ", Style::default().fg(DIALOG_MUTED)),
            Span::styled(description.to_string(), Style::default().fg(DIALOG_TEXT)),
        ]),
        Line::from(vec![
            Span::styled(" Model    ", Style::default().fg(DIALOG_MUTED)),
            Span::styled(model.to_string(), Style::default().fg(DIALOG_TEXT)),
        ]),
    ];

    for (i, line) in lines.iter().enumerate() {
        let y = area.y + i as u16;
        if y >= area.bottom() {
            break;
        }
        buf.set_line(area.x, y, line, area.width);
    }
}

/// Combined header line: " Agents    [Library]  Running" — the modal label on
/// the left, then spaced "pill" tabs. The active tab gets a high-contrast
/// background; the inactive tab is muted.
fn header_line(active: AgentsTab) -> Line<'static> {
    let title_style = Style::default()
        .fg(DIALOG_TEXT)
        .add_modifier(Modifier::BOLD);
    let active_style = Style::default()
        .fg(ratatui::style::Color::Black)
        .bg(DIALOG_ACCENT)
        .add_modifier(Modifier::BOLD);
    let inactive_style = Style::default().fg(DIALOG_MUTED);

    let pill = |label: &'static str, tab: AgentsTab| {
        let s = if active == tab {
            active_style
        } else {
            inactive_style
        };
        Span::styled(format!(" {label} "), s)
    };

    Line::from(vec![
        Span::styled(" Agents", title_style),
        Span::raw("    "),
        pill("Library", AgentsTab::Library),
        Span::raw("  "),
        pill("Running", AgentsTab::Running),
    ])
}

/// Build a footer line with " · " between hints, matching openclaude's style.
fn bullet_footer(items: &[&str]) -> Line<'static> {
    let dim = Style::default().fg(DIALOG_MUTED);
    let mut spans = vec![Span::raw(" ")];
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ·  ", dim));
        }
        spans.push(Span::styled((*item).to_string(), dim));
    }
    Line::from(spans)
}

impl Widget for AgentsOverlay<'_> {
    #[allow(clippy::too_many_lines)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible {
            return;
        }

        // Wider modal (110 cols) with breathing room: header_h=3 (tabs row,
        // blank, separator), footer_h=2 (blank, hints).
        let layout = begin_modal(buf, area, 110, 26, 3, 2);

        // Header row 0: title + tab pills.
        buf.set_line(
            layout.header_area.x,
            layout.header_area.y,
            &header_line(self.state.active_tab),
            layout.header_area.width,
        );
        // Header row 2: thin separator (row 1 is intentionally blank).
        if layout.header_area.height >= 3 {
            render_separator(
                buf,
                Rect {
                    x: layout.header_area.x,
                    y: layout.header_area.y + 2,
                    width: layout.header_area.width,
                    height: 1,
                },
            );
        }

        // Pad the body with one blank row at the top so content doesn't
        // hug the separator.
        let body = Rect {
            x: layout.body_area.x,
            y: layout.body_area.y + 1,
            width: layout.body_area.width,
            height: layout.body_area.height.saturating_sub(1),
        };

        match &self.state.mode {
            AgentsOverlayMode::Browse => match self.state.active_tab {
                AgentsTab::Running => render_running_list(buf, body, self.state),
                AgentsTab::Library => render_library_list(buf, body, self.state),
            },
            AgentsOverlayMode::CreateLocation => {
                render_wizard_choice(
                    buf,
                    body,
                    "Create new agent",
                    "Choose location",
                    &[
                        "Project (.oxicode/agents/)",
                        "Personal (~/.oxicode/agents/)",
                    ],
                    self.state.wizard_selected_idx,
                );
            }
            AgentsOverlayMode::CreateMethod { .. } => {
                render_wizard_choice(
                    buf,
                    body,
                    "Create new agent",
                    "Creation method",
                    &["Generate with Claude (recommended)", "Manual configuration"],
                    self.state.wizard_selected_idx,
                );
            }
            AgentsOverlayMode::CreateGenerate { status, .. } => {
                render_generate_step(
                    buf,
                    body,
                    self.state.wizard_input(),
                    self.state.wizard_cursor(),
                    status,
                    self.frame,
                );
            }
            AgentsOverlayMode::CreateType { .. } => {
                render_wizard_input(
                    buf,
                    body,
                    "Create new agent",
                    "Agent type (identifier)",
                    "e.g., test-runner, tech-lead",
                    self.state.wizard_input(),
                    self.state.wizard_cursor(),
                );
            }
            AgentsOverlayMode::CreateDescription { .. } => {
                render_wizard_input(
                    buf,
                    body,
                    "Create new agent",
                    "Description (when to use this agent)",
                    "e.g., Runs tests and reports failures",
                    self.state.wizard_input(),
                    self.state.wizard_cursor(),
                );
            }
            AgentsOverlayMode::CreatePrompt { .. } => {
                render_wizard_input(
                    buf,
                    body,
                    "Create new agent",
                    "System prompt (agent instructions)",
                    "You are a careful test runner. Always...",
                    self.state.wizard_input(),
                    self.state.wizard_cursor(),
                );
            }
            AgentsOverlayMode::CreateModel { .. } => {
                render_wizard_choice(
                    buf,
                    body,
                    "Create new agent",
                    "Model",
                    &["default (inherit from session)", "sonnet", "opus", "haiku"],
                    self.state.wizard_selected_idx,
                );
            }
            AgentsOverlayMode::CreateConfirm {
                location,
                agent_type,
                description,
                model,
                ..
            } => {
                render_confirm_step(buf, body, *location, agent_type, description, model);
            }
        }

        // Footer with bullet separators.
        let footer = match &self.state.mode {
            AgentsOverlayMode::Browse => bullet_footer(&[
                "←/→ switch tabs",
                "↑↓ navigate",
                "Enter select",
                "Esc close",
            ]),
            AgentsOverlayMode::CreateConfirm { .. } => {
                bullet_footer(&["Esc back", "Enter save", "e save+edit"])
            }
            AgentsOverlayMode::CreateGenerate { status, .. } => match status {
                GenerateStatus::Generating => bullet_footer(&["Esc cancel"]),
                _ => bullet_footer(&["Esc back", "Enter generate"]),
            },
            _ => bullet_footer(&["Esc back", "Enter next"]),
        };
        for y in layout.footer_area.y..layout.footer_area.y + layout.footer_area.height {
            for x in layout.footer_area.x..layout.footer_area.x + layout.footer_area.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_bg(PANEL_BG);
                }
            }
        }
        // Render hints on the bottom row of the footer area (top row is blank).
        let footer_y = layout.footer_area.y + layout.footer_area.height.saturating_sub(1);
        buf.set_line(
            layout.footer_area.x,
            footer_y,
            &footer,
            layout.footer_area.width,
        );
    }
}
