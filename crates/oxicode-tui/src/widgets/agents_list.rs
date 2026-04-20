use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::modal_helpers::{begin_modal, render_modal_title, render_separator, DIALOG_MUTED, PANEL_BG};

#[derive(Debug, Clone)]
pub struct CustomAgentEntry {
    pub name: String,
    pub description: String,
    pub source_path: PathBuf,
}

pub struct AgentsListState {
    visible: bool,
    selected_idx: usize,
    scroll_offset: u16,
    agents: Vec<CustomAgentEntry>,
}

impl AgentsListState {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected_idx: 0,
            scroll_offset: 0,
            agents: Vec::new(),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn open(&mut self, agents: Vec<CustomAgentEntry>) {
        self.visible = true;
        self.selected_idx = 0;
        self.scroll_offset = 0;
        self.agents = agents;
    }

    pub fn cancel(&mut self) {
        self.visible = false;
        self.selected_idx = 0;
        self.scroll_offset = 0;
        self.agents.clear();
    }

    pub fn select_prev(&mut self, view_height: u16) {
        if self.agents.is_empty() {
            return;
        }
        if self.selected_idx == 0 {
            self.selected_idx = self.agents.len() - 1;
        } else {
            self.selected_idx -= 1;
        }
        self.ensure_selected_visible(view_height);
    }

    pub fn select_next(&mut self, view_height: u16) {
        if self.agents.is_empty() {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % self.agents.len();
        self.ensure_selected_visible(view_height);
    }

    pub fn selected(&self) -> Option<&CustomAgentEntry> {
        self.agents.get(self.selected_idx)
    }

    pub fn open_selected_action(&self) -> Option<PathBuf> {
        self.selected().map(|a| a.source_path.clone())
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
        let bottom = self.scroll_offset.saturating_add(view_height.saturating_sub(1));
        if selected > bottom {
            self.scroll_offset = selected.saturating_sub(view_height.saturating_sub(1));
        }
    }
}

pub struct AgentsList<'a> {
    state: &'a AgentsListState,
}

impl<'a> AgentsList<'a> {
    pub fn new(state: &'a AgentsListState) -> Self {
        Self { state }
    }
}

impl Widget for AgentsList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible {
            return;
        }

        let layout = begin_modal(buf, area, 72, 18, 2, 2);
        render_modal_title(buf, layout.header_area, "Agents", "Esc close");
        render_separator(
            buf,
            Rect {
                x: layout.header_area.x,
                y: layout.header_area.y + 1,
                width: layout.header_area.width,
                height: 1,
            },
        );

        if self.state.agents.is_empty() {
            let p = Paragraph::new(Line::from(vec![
                Span::styled(" No custom agents found.", Style::default().fg(DIALOG_MUTED)),
                Span::styled(
                    " Create one in ~/.claude/agents/*.md",
                    Style::default().fg(DIALOG_MUTED),
                ),
            ]))
            .style(Style::default().bg(PANEL_BG));
            p.render(layout.body_area, buf);
            return;
        }

        let visible_rows = layout.body_area.height as usize;
        let start = self.state.scroll_offset as usize;

        for (row, agent) in self
            .state
            .agents
            .iter()
            .skip(start)
            .take(visible_rows)
            .enumerate()
        {
            let y = layout.body_area.y + row as u16;
            if y >= layout.body_area.bottom() {
                break;
            }

            let idx = start + row;
            let selected = idx == self.state.selected_idx;
            let bullet = if selected { "▸" } else { " " };

            let name_style = if selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let line = Line::from(vec![
                Span::styled(format!(" {bullet} "), Style::default().fg(DIALOG_MUTED)),
                Span::styled(agent.name.clone(), name_style),
                Span::styled(" — ", Style::default().fg(DIALOG_MUTED)),
                Span::styled(agent.description.clone(), Style::default().fg(DIALOG_MUTED)),
            ]);
            buf.set_line(layout.body_area.x, y, &line, layout.body_area.width);
        }

        let footer = Line::from(vec![
            Span::styled(" ↑↓ navigate ", Style::default().fg(DIALOG_MUTED)),
            Span::styled(" Enter open ", Style::default().fg(DIALOG_MUTED)),
        ]);
        buf.set_line(layout.footer_area.x, layout.footer_area.y, &footer, layout.footer_area.width);
    }
}
