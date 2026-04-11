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

/// Metadata for a slash command (name + description + category + argument completions).
#[derive(Debug, Clone)]
pub struct SlashCommandMeta {
    /// Command name without leading `/` (e.g., `"clear"`).
    pub name: String,
    /// Short description (e.g., `"Clear conversation history"`).
    pub description: String,
    /// Category for grouping in autocomplete (e.g., `"Session"`, `"Model"`).
    pub category: String,
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

/// Filter command indices by fuzzy subsequence match on name (case-insensitive).
///
/// Results sorted by match quality: prefix matches first, then subsequence.
pub fn filter_commands(commands: &[SlashCommandMeta], query: &str) -> Vec<usize> {
    let query_lower = query.to_lowercase();
    if query_lower.is_empty() {
        return (0..commands.len()).collect();
    }

    let mut scored: Vec<(usize, i32)> = commands
        .iter()
        .enumerate()
        .filter_map(|(i, cmd)| {
            let name_lower = cmd.name.to_lowercase();
            fuzzy_score(&name_lower, &query_lower).map(|score| (i, score))
        })
        .collect();

    // Higher score = better match.
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(i, _)| i).collect()
}

/// Compute fuzzy match score. Returns `None` if not a subsequence.
///
/// Bonuses: +10 per char, +20 prefix, +15 consecutive, +5 word-start.
fn fuzzy_score(name: &str, query: &str) -> Option<i32> {
    let name_chars: Vec<char> = name.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();

    if query_chars.is_empty() {
        return Some(0);
    }
    if query_chars.len() > name_chars.len() {
        return None;
    }

    let mut score: i32 = 0;
    let mut name_idx = 0;
    let mut prev_match_idx: Option<usize> = None;

    for (qi, &qc) in query_chars.iter().enumerate() {
        let mut found = false;
        while name_idx < name_chars.len() {
            if name_chars[name_idx] == qc {
                score += 10;
                if qi == 0 && name_idx == 0 {
                    score += 20; // prefix bonus
                }
                if let Some(prev) = prev_match_idx {
                    if name_idx == prev + 1 {
                        score += 15; // consecutive bonus
                    }
                }
                if name_idx == 0
                    || (name_idx > 0 && matches!(name_chars[name_idx - 1], '-' | '_'))
                {
                    score += 5; // word-start bonus
                }
                prev_match_idx = Some(name_idx);
                name_idx += 1;
                found = true;
                break;
            }
            name_idx += 1;
        }
        if !found {
            return None;
        }
    }

    Some(score)
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

            // Show category header if category changed from previous visible item.
            let prev_category = if row_idx > 0 {
                let prev_idx = self.state.filtered[start + row_idx - 1];
                Some(self.commands[prev_idx].category.as_str())
            } else {
                None
            };
            let show_category = !cmd.category.is_empty()
                && prev_category.map_or(true, |prev| prev != cmd.category);

            // Styling — selected vs normal.
            let (name_style, desc_style, bg) = if is_selected {
                (
                    Style::default()
                        .fg(crate::render::STATUS_CYAN)
                        .bg(Color::Rgb(45, 38, 32))
                        .add_modifier(Modifier::BOLD),
                    Style::default()
                        .fg(crate::render::TRANSCRIPT_TEXT)
                        .bg(Color::Rgb(45, 38, 32)),
                    Color::Rgb(45, 38, 32),
                )
            } else {
                (
                    Style::default()
                        .fg(crate::render::STATUS_CYAN)
                        .bg(Color::Rgb(25, 22, 20)),
                    Style::default()
                        .fg(crate::render::TRANSCRIPT_MUTED)
                        .bg(Color::Rgb(25, 22, 20)),
                    Color::Rgb(25, 22, 20),
                )
            };

            // Fill row background.
            for col in area.x..area.right() {
                if let Some(cell) = buf.cell_mut((col, y)) {
                    cell.set_style(Style::default().bg(bg));
                    cell.set_symbol(" ");
                }
            }

            // Build row: "  /name           description" with optional category prefix.
            let prefix = if is_selected { "\u{25b8} " } else { "  " };
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

            let mut spans = vec![
                Span::styled(prefix.to_string(), desc_style),
                Span::styled(padded_name, name_style),
                Span::styled("  ", desc_style),
                Span::styled(desc, desc_style),
            ];

            // Category badge at the right edge for the first item in a new category.
            if show_category {
                let cat_label = format!("  [{cat}]", cat = cmd.category);
                spans.push(Span::styled(
                    cat_label,
                    Style::default().fg(crate::render::CHROME_MUTED).bg(bg),
                ));
            }

            buf.set_line(area.x, y, &Line::from(spans), area.width);
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
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "compact".into(),
                description: "Compact context".into(),
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "help".into(),
                description: "Show help".into(),
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "model".into(),
                description: "Switch model".into(),
                category: "Model".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "color".into(),
                description: "Color settings".into(),
                category: "Display".into(),
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
        // "clear" has prefix match (highest score), possibly "color" via fuzzy
        assert!(!result.is_empty());
        assert_eq!(cmds[result[0]].name, "clear"); // prefix match ranked first
    }

    #[test]
    fn filter_multiple_matches() {
        let cmds = sample_commands();
        let result = filter_commands(&cmds, "co");
        assert!(result.len() >= 2); // "compact", "color" both prefix match
    }

    #[test]
    fn filter_case_insensitive() {
        let cmds = sample_commands();
        let result = filter_commands(&cmds, "CL");
        assert!(!result.is_empty());
        assert_eq!(cmds[result[0]].name, "clear"); // best match first
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
        assert_eq!(cell.bg, Color::Rgb(45, 38, 32));

        // Non-selected row (row 0) should have normal background.
        let cell0 = buf.cell((2, 0)).unwrap();
        assert_eq!(cell0.bg, Color::Rgb(25, 22, 20));
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
