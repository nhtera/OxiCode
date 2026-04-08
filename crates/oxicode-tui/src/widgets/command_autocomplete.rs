//! Command autocomplete dropdown widget.
//!
//! Shows a filtered list of slash commands above the input area when the user
//! types `/`. Supports keyboard navigation (Up/Down) and selection (Enter/Tab).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

/// Maximum number of items visible in the dropdown at once.
const MAX_VISIBLE: usize = 8;

/// Column width reserved for the command name (left-aligned).
const NAME_COL_WIDTH: usize = 20;

/// Metadata for a slash command (name + description + argument completions).
#[derive(Debug, Clone)]
pub struct SlashCommandMeta {
    /// Command name without leading `/` (e.g., `"clear"`).
    pub name: String,
    /// Short description (e.g., `"Clear conversation history"`).
    pub description: String,
    /// Pre-computed argument completion candidates (e.g., model names for `/model`).
    pub arg_candidates: Vec<String>,
}

/// Autocomplete dropdown state tracked by `App`.
#[derive(Default)]
pub struct AutocompleteState {
    /// Whether the dropdown is currently visible.
    pub active: bool,
    /// Indices into the command list that match the current filter.
    pub filtered: Vec<usize>,
    /// Index into `filtered` for the currently highlighted item.
    pub selected: usize,
}

impl AutocompleteState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Activate the dropdown with the given filtered indices.
    pub fn activate(&mut self, filtered: Vec<usize>) {
        self.active = true;
        self.filtered = filtered;
        self.selected = 0;
    }

    /// Deactivate the dropdown.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.filtered.clear();
        self.selected = 0;
    }

    /// Move selection up (wraps around).
    pub fn select_prev(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.filtered.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Move selection down (wraps around).
    pub fn select_next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
    }

    /// Height needed for the dropdown (clamped to `MAX_VISIBLE`).
    pub fn visible_height(&self) -> u16 {
        if !self.active {
            return 0;
        }
        (self.filtered.len() as u16).min(MAX_VISIBLE as u16)
    }
}

/// Filter command indices by prefix match on name (case-insensitive).
pub fn filter_commands(commands: &[SlashCommandMeta], query: &str) -> Vec<usize> {
    let query_lower = query.to_lowercase();
    commands
        .iter()
        .enumerate()
        .filter(|(_, cmd)| cmd.name.to_lowercase().starts_with(&query_lower))
        .map(|(i, _)| i)
        .collect()
}

/// Renders the command autocomplete dropdown.
///
/// Each row shows: `  /name           description`
/// Selected row is highlighted with a contrasting background.
pub struct CommandAutocomplete<'a> {
    commands: &'a [SlashCommandMeta],
    state: &'a AutocompleteState,
}

impl<'a> CommandAutocomplete<'a> {
    pub fn new(commands: &'a [SlashCommandMeta], state: &'a AutocompleteState) -> Self {
        Self { commands, state }
    }
}

impl Widget for CommandAutocomplete<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.active || self.state.filtered.is_empty() || area.height == 0 {
            return;
        }

        let visible = area.height as usize;
        let total = self.state.filtered.len();

        // Compute viewport window: keep selected item roughly centered.
        let start = if total <= visible || self.state.selected < visible / 2 {
            0
        } else if self.state.selected + visible / 2 >= total {
            total.saturating_sub(visible)
        } else {
            self.state.selected.saturating_sub(visible / 2)
        };

        for (row_idx, &cmd_idx) in self
            .state
            .filtered
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let y = area.y + row_idx as u16;
            if y >= area.bottom() {
                break;
            }

            let is_selected = start + row_idx == self.state.selected;
            let cmd = &self.commands[cmd_idx];

            // Styling
            let (name_style, desc_style, bg) = if is_selected {
                (
                    Style::default()
                        .fg(Color::Cyan)
                        .bg(Color::Rgb(30, 40, 60))
                        .add_modifier(Modifier::BOLD),
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(30, 40, 60)),
                    Color::Rgb(30, 40, 60),
                )
            } else {
                (
                    Style::default()
                        .fg(Color::Cyan)
                        .bg(Color::Rgb(20, 20, 30)),
                    Style::default()
                        .fg(Color::Gray)
                        .bg(Color::Rgb(20, 20, 30)),
                    Color::Rgb(20, 20, 30),
                )
            };

            // Fill row background.
            for col in area.x..area.right() {
                if let Some(cell) = buf.cell_mut((col, y)) {
                    cell.set_style(Style::default().bg(bg));
                    cell.set_symbol(" ");
                }
            }

            // Build row: "  /name           description"
            let name_display = format!("/{}", cmd.name);
            // Pad name to fixed width for alignment.
            let padded_name = if name_display.len() < NAME_COL_WIDTH {
                format!("{name_display:<NAME_COL_WIDTH$}")
            } else {
                name_display.clone()
            };

            // Truncate description to fit remaining width.
            let desc_max = (area.width as usize).saturating_sub(NAME_COL_WIDTH + 4);
            let desc = if cmd.description.len() > desc_max {
                format!("{}…", &cmd.description[..desc_max.saturating_sub(1)])
            } else {
                cmd.description.clone()
            };

            let line = Line::from(vec![
                Span::styled("  ", desc_style),
                Span::styled(padded_name, name_style),
                Span::styled("  ", desc_style),
                Span::styled(desc, desc_style),
            ]);

            buf.set_line(area.x, y, &line, area.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_commands() -> Vec<SlashCommandMeta> {
        vec![
            SlashCommandMeta {
                name: "clear".into(),
                description: "Clear conversation".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "compact".into(),
                description: "Compact context".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "help".into(),
                description: "Show help".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "model".into(),
                description: "Switch model".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "color".into(),
                description: "Color settings".into(),
                arg_candidates: vec![],
            },
        ]
    }

    #[test]
    fn filter_empty_query_returns_all() {
        let cmds = sample_commands();
        let result = filter_commands(&cmds, "");
        assert_eq!(result.len(), cmds.len());
    }

    #[test]
    fn filter_prefix_match() {
        let cmds = sample_commands();
        let result = filter_commands(&cmds, "cl");
        assert_eq!(result.len(), 1); // "clear"
        assert_eq!(cmds[result[0]].name, "clear");
    }

    #[test]
    fn filter_multiple_matches() {
        let cmds = sample_commands();
        let result = filter_commands(&cmds, "co");
        assert_eq!(result.len(), 2); // "compact", "color"
    }

    #[test]
    fn filter_case_insensitive() {
        let cmds = sample_commands();
        let result = filter_commands(&cmds, "CL");
        assert_eq!(result.len(), 1);
        assert_eq!(cmds[result[0]].name, "clear");
    }

    #[test]
    fn filter_no_match() {
        let cmds = sample_commands();
        let result = filter_commands(&cmds, "xyz");
        assert!(result.is_empty());
    }

    #[test]
    fn select_wraps_around() {
        let mut state = AutocompleteState::new();
        state.activate(vec![0, 1, 2]);

        assert_eq!(state.selected, 0);
        state.select_prev(); // wrap to end
        assert_eq!(state.selected, 2);
        state.select_next(); // wrap to start
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn deactivate_resets() {
        let mut state = AutocompleteState::new();
        state.activate(vec![0, 1, 2]);
        state.selected = 2;
        state.deactivate();

        assert!(!state.active);
        assert!(state.filtered.is_empty());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn visible_height_clamped() {
        let mut state = AutocompleteState::new();
        state.activate((0..20).collect());
        assert_eq!(state.visible_height(), MAX_VISIBLE as u16);
    }

    #[test]
    fn widget_renders_selected_highlight() {
        let cmds = sample_commands();
        let mut state = AutocompleteState::new();
        state.activate(vec![0, 1, 2]);
        state.selected = 1; // "compact" selected

        let area = Rect::new(0, 0, 60, 3);
        let mut buf = Buffer::empty(area);
        CommandAutocomplete::new(&cmds, &state).render(area, &mut buf);

        // Selected row (row 1) should have highlight background.
        let cell = buf.cell((2, 1)).unwrap();
        assert_eq!(cell.bg, Color::Rgb(30, 40, 60));

        // Non-selected row (row 0) should have normal background.
        let cell0 = buf.cell((2, 0)).unwrap();
        assert_eq!(cell0.bg, Color::Rgb(20, 20, 30));
    }

    #[test]
    fn empty_state_renders_nothing() {
        let cmds = sample_commands();
        let state = AutocompleteState::new();
        let area = Rect::new(0, 0, 60, 3);
        let mut buf = Buffer::empty(area);
        CommandAutocomplete::new(&cmds, &state).render(area, &mut buf);
        // All cells should be empty.
        let content: String = (0..60)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert!(content.trim().is_empty());
    }
}
