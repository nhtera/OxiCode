use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::modal_helpers::{begin_modal, render_modal_title, render_separator, DIALOG_MUTED, PANEL_BG};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsMenuAction {
    List,
    Create,
}

pub struct AgentsMenuState {
    visible: bool,
    selected_idx: usize,
}

impl AgentsMenuState {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected_idx: 0,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn open(&mut self) {
        self.visible = true;
        self.selected_idx = 0;
    }

    pub fn cancel(&mut self) {
        self.visible = false;
        self.selected_idx = 0;
    }

    pub fn select_prev(&mut self) {
        self.selected_idx = if self.selected_idx == 0 { 1 } else { 0 };
    }

    pub fn select_next(&mut self) {
        self.selected_idx = (self.selected_idx + 1) % 2;
    }

    pub fn selected_action(&self) -> AgentsMenuAction {
        match self.selected_idx {
            0 => AgentsMenuAction::List,
            _ => AgentsMenuAction::Create,
        }
    }
}

pub struct AgentsMenu<'a> {
    state: &'a AgentsMenuState,
}

impl<'a> AgentsMenu<'a> {
    pub fn new(state: &'a AgentsMenuState) -> Self {
        Self { state }
    }
}

impl Widget for AgentsMenu<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible {
            return;
        }

        let layout = begin_modal(buf, area, 50, 10, 2, 2);
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

        let options = [
            ("List agents", "Browse configured agents"),
            ("Create agent", "Create a new .md agent file"),
        ];

        for (i, (title, desc)) in options.iter().enumerate() {
            let y = layout.body_area.y + i as u16;
            if y >= layout.body_area.bottom() {
                break;
            }
            let selected = i == self.state.selected_idx;
            let bullet = if selected { "▸" } else { " " };
            let title_style = if selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let line = Line::from(vec![
                Span::styled(format!(" {bullet} "), Style::default().fg(DIALOG_MUTED)),
                Span::styled((*title).to_string(), title_style),
                Span::styled(" — ", Style::default().fg(DIALOG_MUTED)),
                Span::styled((*desc).to_string(), Style::default().fg(DIALOG_MUTED)),
            ]);
            buf.set_line(layout.body_area.x, y, &line, layout.body_area.width);
        }

        let footer = Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ navigate  ", Style::default().fg(DIALOG_MUTED)),
            Span::styled(" Enter select ", Style::default().fg(DIALOG_MUTED)),
        ]))
        .style(Style::default().bg(PANEL_BG));
        footer.render(layout.footer_area, buf);
    }
}
