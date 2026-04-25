//! Providers settings tab: table of provider configs with masked secrets.
//!
//! Shows Name | Base URL | Auth | Status columns. Enter opens an edit dialog
//! where the token is masked by default; Ctrl+R reveals in that dialog only.
//! `[+]` row adds a custom provider.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::widgets::modal_helpers::{DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT};

/// A single provider row (kept in memory, serialised on save).
#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub name: String,
    pub base_url: String,
    /// Stored in memory; rendered masked unless reveal_mode is active.
    pub auth_token: String,
    /// Derived from connectivity check (not persisted).
    pub status: ProviderStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderStatus {
    /// Not checked yet.
    Unknown,
    /// Key present (not validated online in v1).
    Present,
    /// No key configured.
    Missing,
}

impl ProviderStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "–",
            Self::Present => "✓ key set",
            Self::Missing => "✗ no key",
        }
    }
}

/// Edit dialog fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    BaseUrl,
    AuthToken,
}

/// Providers tab state.
#[derive(Debug, Clone)]
pub struct ProvidersTab {
    /// List of providers; last entry is always the virtual `[+ Add]` row.
    pub providers: Vec<ProviderRow>,
    /// Currently highlighted row index (includes `[+ Add]` sentinel at end).
    pub selected: usize,
    /// When `Some`, the edit dialog is open for that provider index.
    /// `None` means dialog closed.
    pub editing_idx: Option<usize>,
    /// Which field is active inside the edit dialog.
    pub edit_field: EditField,
    /// Temporary buffer for base_url editing.
    pub url_buf: String,
    /// Temporary buffer for auth_token editing.
    pub token_buf: String,
    /// Whether token is revealed in the edit dialog.
    pub reveal_token: bool,
}

/// Mask string for hidden secrets — fixed width bullet sequence.
const SECRET_MASK: &str = "●●●●●●●●";

impl ProvidersTab {
    pub fn new(providers: Vec<ProviderRow>) -> Self {
        Self {
            providers,
            selected: 0,
            editing_idx: None,
            edit_field: EditField::BaseUrl,
            url_buf: String::new(),
            token_buf: String::new(),
            reveal_token: false,
        }
    }

    /// Total selectable rows = providers + 1 (`[+ Add]`).
    pub fn row_count(&self) -> usize {
        self.providers.len() + 1
    }

    pub fn select_next(&mut self) {
        if self.editing_idx.is_none() {
            let max = self.row_count().saturating_sub(1);
            self.selected = (self.selected + 1).min(max);
        }
    }

    pub fn select_prev(&mut self) {
        if self.editing_idx.is_none() {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    /// Open the edit dialog for the selected row (or start `[+ Add]` flow).
    pub fn open_edit(&mut self) {
        let idx = self.selected;
        if idx < self.providers.len() {
            let p = &self.providers[idx];
            self.url_buf.clone_from(&p.base_url);
            self.token_buf.clone_from(&p.auth_token);
            self.editing_idx = Some(idx);
            self.edit_field = EditField::BaseUrl;
            self.reveal_token = false;
        } else {
            // `[+ Add]` row — add blank provider and open for editing.
            self.providers.push(ProviderRow {
                name: "new-provider".to_string(),
                base_url: String::new(),
                auth_token: String::new(),
                status: ProviderStatus::Missing,
            });
            let new_idx = self.providers.len() - 1;
            self.selected = new_idx;
            self.url_buf.clear();
            self.token_buf.clear();
            self.editing_idx = Some(new_idx);
            self.edit_field = EditField::BaseUrl;
            self.reveal_token = false;
        }
    }

    /// Commit the edit dialog into the providers list. Returns true if changed.
    pub fn commit_edit(&mut self) -> bool {
        let Some(idx) = self.editing_idx.take() else {
            return false;
        };
        if idx >= self.providers.len() {
            return false;
        }
        let p = &mut self.providers[idx];
        let url_changed = p.base_url != self.url_buf;
        let tok_changed = p.auth_token != self.token_buf;
        p.base_url.clone_from(&self.url_buf);
        p.auth_token.clone_from(&self.token_buf);
        if !self.token_buf.is_empty() {
            p.status = ProviderStatus::Present;
        } else {
            p.status = ProviderStatus::Missing;
        }
        self.reveal_token = false;
        url_changed || tok_changed
    }

    pub fn cancel_edit(&mut self) {
        self.editing_idx = None;
        self.reveal_token = false;
    }

    pub fn toggle_reveal(&mut self) {
        if self.editing_idx.is_some() {
            self.reveal_token = !self.reveal_token;
        }
    }

    pub fn toggle_field(&mut self) {
        self.edit_field = match self.edit_field {
            EditField::BaseUrl => EditField::AuthToken,
            EditField::AuthToken => EditField::BaseUrl,
        };
    }

    pub fn push_char(&mut self, c: char) {
        if self.editing_idx.is_some() {
            match self.edit_field {
                EditField::BaseUrl => self.url_buf.push(c),
                EditField::AuthToken => self.token_buf.push(c),
            }
        }
    }

    pub fn pop_char(&mut self) {
        if self.editing_idx.is_some() {
            match self.edit_field {
                EditField::BaseUrl => {
                    self.url_buf.pop();
                }
                EditField::AuthToken => {
                    self.token_buf.pop();
                }
            }
        }
    }

    /// Render the tab body into `area`.
    pub fn render(&self, buf: &mut Buffer, area: Rect) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Column widths.
        let name_w: usize = 14;
        let url_w: usize = 26;
        let auth_w: usize = 12;

        // Header row.
        if area.height >= 1 {
            let header = Line::from(vec![Span::styled(
                format!(
                    "  {:<name_w$} {:<url_w$} {:<auth_w$} Status",
                    "Name", "Base URL", "Auth"
                ),
                Style::default()
                    .fg(DIALOG_MUTED)
                    .add_modifier(Modifier::BOLD),
            )]);
            buf.set_line(area.x, area.y, &header, area.width);
        }

        // Separator.
        if area.height >= 2 {
            let sep: String = "\u{2500}".repeat(area.width as usize);
            let sep_line = Line::from(Span::styled(sep, Style::default().fg(DIALOG_MUTED)));
            buf.set_line(area.x, area.y + 1, &sep_line, area.width);
        }

        // Provider rows.
        let base_row = area.y + 2;
        for (i, provider) in self.providers.iter().enumerate() {
            let row_y = base_row + i as u16;
            if row_y >= area.y + area.height {
                break;
            }
            let is_selected = self.selected == i && self.editing_idx.is_none();
            let cursor = if is_selected { "▶ " } else { "  " };
            let row_style = if is_selected {
                Style::default()
                    .fg(DIALOG_TEXT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_TEXT)
            };
            let auth_display = if provider.auth_token.is_empty() {
                "—".to_string()
            } else {
                SECRET_MASK.to_string()
            };

            let name_trunc: String = provider.name.chars().take(name_w).collect();
            let url_trunc: String = provider.base_url.chars().take(url_w).collect();
            let auth_trunc: String = auth_display.chars().take(auth_w).collect();

            let line = Line::from(vec![
                Span::styled(cursor, Style::default().fg(DIALOG_ACCENT)),
                Span::styled(format!("{:<name_w$} ", name_trunc), row_style),
                Span::styled(format!("{:<url_w$} ", url_trunc), row_style),
                Span::styled(format!("{:<auth_w$} ", auth_trunc), row_style),
                Span::styled(provider.status.label(), Style::default().fg(DIALOG_MUTED)),
            ]);
            buf.set_line(area.x, row_y, &line, area.width);
        }

        // `[+ Add provider]` virtual row.
        let add_row_y = base_row + self.providers.len() as u16;
        if add_row_y < area.y + area.height && self.editing_idx.is_none() {
            let is_sel = self.selected == self.providers.len();
            let cursor = if is_sel { "▶ " } else { "  " };
            let style = if is_sel {
                Style::default()
                    .fg(DIALOG_ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIALOG_MUTED)
            };
            let line = Line::from(vec![
                Span::styled(cursor, Style::default().fg(DIALOG_ACCENT)),
                Span::styled("[+ Add provider]", style),
            ]);
            buf.set_line(area.x, add_row_y, &line, area.width);
        }

        // Edit dialog overlay (drawn inside the tab body).
        if let Some(edit_idx) = self.editing_idx {
            self.render_edit_dialog(buf, area, edit_idx);
        }
    }

    fn render_edit_dialog(&self, buf: &mut Buffer, area: Rect, idx: usize) {
        // Simple inline dialog: draw a box in the lower half of `area`.
        let dialog_h = 8u16;
        let dialog_w = area.width.saturating_sub(4).max(20);
        let dx = area.x + 2;
        let dy = area.y + area.height.saturating_sub(dialog_h + 1);

        // Background fill.
        for r in dy..dy + dialog_h {
            for c in dx..dx + dialog_w {
                if let Some(cell) = buf.cell_mut((c, r)) {
                    cell.set_char(' ');
                    cell.set_bg(crate::widgets::modal_helpers::PANEL_BG);
                }
            }
        }

        // Border top.
        let name = self
            .providers
            .get(idx)
            .map(|p| p.name.as_str())
            .unwrap_or("Provider");
        let title = format!(" Edit: {name} ");
        let top_line = Line::from(Span::styled(
            title,
            Style::default()
                .fg(DIALOG_ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
        buf.set_line(dx, dy, &top_line, dialog_w);

        // URL field.
        let url_label = "  Base URL:    ";
        let url_value = format!("{}▊", self.url_buf);
        let url_is_active = self.edit_field == EditField::BaseUrl;
        let url_line = Line::from(vec![
            Span::styled(url_label, Style::default().fg(DIALOG_MUTED)),
            Span::styled(
                url_value,
                if url_is_active {
                    Style::default()
                        .fg(DIALOG_TEXT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DIALOG_TEXT)
                },
            ),
        ]);
        buf.set_line(dx, dy + 2, &url_line, dialog_w);

        // Token field.
        let tok_label = "  Auth token:  ";
        let tok_raw = if self.reveal_token {
            format!("{}▊", self.token_buf)
        } else if self.token_buf.is_empty() {
            "▊".to_string()
        } else {
            format!("{}▊", SECRET_MASK)
        };
        let tok_is_active = self.edit_field == EditField::AuthToken;
        let tok_line = Line::from(vec![
            Span::styled(tok_label, Style::default().fg(DIALOG_MUTED)),
            Span::styled(
                tok_raw,
                if tok_is_active {
                    Style::default()
                        .fg(DIALOG_TEXT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DIALOG_TEXT)
                },
            ),
        ]);
        buf.set_line(dx, dy + 4, &tok_line, dialog_w);

        // Hint row.
        let hint = "  [Tab] switch field  [Ctrl+R] reveal  [Enter] save  [Esc] cancel";
        let hint_line = Line::from(Span::styled(hint, Style::default().fg(DIALOG_MUTED)));
        buf.set_line(dx, dy + 6, &hint_line, dialog_w);
    }
}
