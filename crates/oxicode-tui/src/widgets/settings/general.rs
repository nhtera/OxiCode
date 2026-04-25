//! General settings tab: model, output format, theme, vim mode, ghost completion.
//!
//! Displays labeled fields. The active field is highlighted; Enter toggles or
//! begins inline text editing. Edits stay in memory until `SettingsScreen::save()`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::widgets::modal_helpers::{DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT};

/// Number of editable fields in the General tab.
pub const GENERAL_FIELD_COUNT: usize = 5;

/// Which field is selected in the General tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneralField {
    Model,
    OutputFormat,
    Theme,
    VimMode,
    GhostCompletion,
}

impl GeneralField {
    pub fn index(self) -> usize {
        match self {
            Self::Model => 0,
            Self::OutputFormat => 1,
            Self::Theme => 2,
            Self::VimMode => 3,
            Self::GhostCompletion => 4,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            1 => Self::OutputFormat,
            2 => Self::Theme,
            3 => Self::VimMode,
            4 => Self::GhostCompletion,
            _ => Self::Model,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Model => "Model",
            Self::OutputFormat => "Output format",
            Self::Theme => "Theme",
            Self::VimMode => "Vim mode",
            Self::GhostCompletion => "Ghost completion",
        }
    }
}

/// State for the General settings tab.
#[derive(Debug, Clone)]
pub struct GeneralTab {
    /// Index of the currently selected field (0..GENERAL_FIELD_COUNT).
    pub selected: usize,
    /// Whether the selected field is in edit mode.
    pub editing: bool,
    /// Editable: model ID string.
    pub model: String,
    /// Editable: "markdown" | "plain" | "minimal" | "verbose"
    pub output_format: String,
    /// Editable: theme name.
    pub theme: String,
    /// Toggle: vim mode enabled.
    pub vim_mode: bool,
    /// Toggle: ghost (autocomplete) completion enabled.
    pub ghost_completion: bool,
    /// Buffer for inline text editing.
    pub edit_buffer: String,
}

impl GeneralTab {
    pub fn new(
        model: String,
        output_format: String,
        theme: String,
        vim_mode: bool,
        ghost_completion: bool,
    ) -> Self {
        Self {
            selected: 0,
            editing: false,
            model,
            output_format,
            theme,
            vim_mode,
            ghost_completion,
            edit_buffer: String::new(),
        }
    }

    pub fn selected_field(&self) -> GeneralField {
        GeneralField::from_index(self.selected)
    }

    pub fn select_next(&mut self) {
        if !self.editing {
            self.selected = (self.selected + 1) % GENERAL_FIELD_COUNT;
        }
    }

    pub fn select_prev(&mut self) {
        if !self.editing {
            if self.selected == 0 {
                self.selected = GENERAL_FIELD_COUNT - 1;
            } else {
                self.selected -= 1;
            }
        }
    }

    /// Activate editing or toggle the selected field. Returns true if dirty.
    pub fn activate_field(&mut self) -> bool {
        let field = self.selected_field();
        match field {
            GeneralField::VimMode => {
                self.vim_mode = !self.vim_mode;
                true
            }
            GeneralField::GhostCompletion => {
                self.ghost_completion = !self.ghost_completion;
                true
            }
            GeneralField::OutputFormat => {
                // Cycle through options
                self.output_format = match self.output_format.as_str() {
                    "markdown" => "plain".to_string(),
                    "plain" => "minimal".to_string(),
                    "minimal" => "verbose".to_string(),
                    _ => "markdown".to_string(),
                };
                true
            }
            _ => {
                if self.editing {
                    // Commit edit
                    self.commit_edit();
                } else {
                    self.edit_buffer = self.field_value(field).to_string();
                    self.editing = true;
                }
                false
            }
        }
    }

    /// Commit in-progress text edit into the backing field.
    pub fn commit_edit(&mut self) {
        if !self.editing {
            return;
        }
        let field = self.selected_field();
        let value = self.edit_buffer.trim().to_string();
        if !value.is_empty() {
            match field {
                GeneralField::Model => self.model = value,
                GeneralField::Theme => self.theme = value,
                _ => {}
            }
        }
        self.editing = false;
    }

    /// Cancel in-progress edit without committing.
    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.edit_buffer.clear();
    }

    pub fn push_char(&mut self, c: char) {
        if self.editing {
            self.edit_buffer.push(c);
        }
    }

    pub fn pop_char(&mut self) {
        if self.editing {
            self.edit_buffer.pop();
        }
    }

    fn field_value(&self, field: GeneralField) -> &str {
        match field {
            GeneralField::Model => &self.model,
            GeneralField::OutputFormat => &self.output_format,
            GeneralField::Theme => &self.theme,
            GeneralField::VimMode => {
                if self.vim_mode {
                    "on"
                } else {
                    "off"
                }
            }
            GeneralField::GhostCompletion => {
                if self.ghost_completion {
                    "on"
                } else {
                    "off"
                }
            }
        }
    }

    /// Render the tab body into `area`.
    pub fn render(&self, buf: &mut Buffer, area: Rect) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let label_w: usize = 18;
        let fields = [
            GeneralField::Model,
            GeneralField::OutputFormat,
            GeneralField::Theme,
            GeneralField::VimMode,
            GeneralField::GhostCompletion,
        ];

        for (i, &field) in fields.iter().enumerate() {
            let row = area.y + i as u16 * 2;
            if row >= area.y + area.height {
                break;
            }

            let is_selected = self.selected == i;
            let is_editing = is_selected && self.editing;

            // Value display
            let value = if is_editing {
                format!("{}▊", self.edit_buffer)
            } else {
                match field {
                    GeneralField::VimMode => {
                        if self.vim_mode {
                            "[ON]  ".to_string()
                        } else {
                            "[OFF] ".to_string()
                        }
                    }
                    GeneralField::GhostCompletion => {
                        if self.ghost_completion {
                            "[ON]  ".to_string()
                        } else {
                            "[OFF] ".to_string()
                        }
                    }
                    _ => self.field_value(field).to_string(),
                }
            };

            let label_style = if is_selected {
                Style::default()
                    .fg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_MUTED)
            };
            let value_style = if is_selected {
                Style::default()
                    .fg(DIALOG_TEXT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_TEXT)
            };
            let cursor = if is_selected { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(cursor, Style::default().fg(DIALOG_ACCENT)),
                Span::styled(format!("{:<label_w$}", field.label()), label_style),
                Span::styled(value, value_style),
            ]);
            buf.set_line(area.x, row, &line, area.width);
        }
    }
}
