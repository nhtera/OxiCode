//! Settings screen overlay — 4-tab in-TUI settings editor.
//!
//! Tabs: General | Providers | Permissions | Hooks (readonly)
//!
//! Keybindings:
//!   Tab / Shift+Tab  — cycle tabs
//!   Up / Down        — navigate within tab
//!   Enter            — activate / toggle selected field
//!   Ctrl+S           — save to ~/.oxicode/settings.toml
//!   Esc              — close overlay
//!
//! Save flow: writes back to `~/.oxicode/settings.toml` using toml serialization.
//! NEVER touches `~/.claude/settings.json`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::modal_helpers::{
    begin_modal, render_modal_title, render_separator, DIALOG_ACCENT, DIALOG_MUTED, DIALOG_TEXT,
};
use super::settings::{
    DisplayPermMode, GeneralTab, HookEntry, HookGroup, HookSource, HooksTab, PermissionsTab,
    ProviderRow, ProviderStatus, ProvidersTab,
};

// ── Tab enum ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Providers,
    Permissions,
    Hooks,
}

impl SettingsTab {
    const ALL: [Self; 4] = [
        Self::General,
        Self::Providers,
        Self::Permissions,
        Self::Hooks,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Providers => "Providers",
            Self::Permissions => "Permissions",
            Self::Hooks => "Hooks",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::General => Self::Providers,
            Self::Providers => Self::Permissions,
            Self::Permissions => Self::Hooks,
            Self::Hooks => Self::General,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::General => Self::Hooks,
            Self::Providers => Self::General,
            Self::Permissions => Self::Providers,
            Self::Hooks => Self::Permissions,
        }
    }
}

// ── Main screen state ─────────────────────────────────────────────────────────

/// Settings screen overlay state tracked by `App`.
pub struct SettingsScreen {
    pub active_tab: SettingsTab,
    pub general: GeneralTab,
    pub providers: ProvidersTab,
    pub permissions: PermissionsTab,
    pub hooks: HooksTab,
    /// True when any field has been modified but not yet saved.
    pub dirty: bool,
    /// Transient save-result message (shown in footer briefly).
    pub save_message: Option<String>,
}

impl SettingsScreen {
    /// Build a `SettingsScreen` from a live `oxicode_config::Settings` snapshot.
    pub fn from_config(settings: &oxicode_config::Settings) -> Self {
        let vim_mode = settings.editor_mode == "vim";
        let ghost_completion = settings.features.is_enabled("ghost_completion");

        let general = GeneralTab::new(
            settings.model.clone(),
            settings.output_style.clone(),
            settings.theme.clone(),
            vim_mode,
            ghost_completion,
        );

        // Build provider list from the api_key present in settings.
        // In v1 we surface a single "Anthropic" row; future phases wire multi-provider.
        let mut providers_list = Vec::new();
        if settings.api_key.is_some() {
            providers_list.push(ProviderRow {
                name: "Anthropic".to_string(),
                base_url: "https://api.anthropic.com".to_string(),
                auth_token: settings.api_key.clone().unwrap_or_default(),
                status: ProviderStatus::Present,
            });
        } else {
            providers_list.push(ProviderRow {
                name: "Anthropic".to_string(),
                base_url: "https://api.anthropic.com".to_string(),
                auth_token: String::new(),
                status: ProviderStatus::Missing,
            });
        }
        let providers = ProvidersTab::new(providers_list);

        let perm_mode = DisplayPermMode::from_str(&settings.permission_mode);
        let allow_list = settings.always_allow_tools.join(", ");
        let deny_list = settings.always_deny_tools.join(", ");
        let permissions = PermissionsTab::new(perm_mode, allow_list, deny_list);

        // Build hooks tree from the hook config files found on disk.
        let hook_groups = load_hook_groups_from_disk();
        let hooks = HooksTab::new(hook_groups);

        Self {
            active_tab: SettingsTab::General,
            general,
            providers,
            permissions,
            hooks,
            dirty: false,
            save_message: None,
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = self.active_tab.next();
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = self.active_tab.prev();
    }

    pub fn is_providers_editing(&self) -> bool {
        self.providers.editing_idx.is_some()
    }

    // ── Per-tab key delegates ────────────────────────────────────────────────

    pub fn select_next(&mut self) {
        match self.active_tab {
            SettingsTab::General => self.general.select_next(),
            SettingsTab::Providers => self.providers.select_next(),
            SettingsTab::Permissions => self.permissions.select_next(),
            SettingsTab::Hooks => self.hooks.select_next(),
        }
    }

    pub fn select_prev(&mut self) {
        match self.active_tab {
            SettingsTab::General => self.general.select_prev(),
            SettingsTab::Providers => self.providers.select_prev(),
            SettingsTab::Permissions => self.permissions.select_prev(),
            SettingsTab::Hooks => self.hooks.select_prev(),
        }
    }

    /// Activate / toggle the currently selected field. Marks dirty on change.
    pub fn activate(&mut self) {
        let changed = match self.active_tab {
            SettingsTab::General => self.general.activate_field(),
            SettingsTab::Providers => {
                self.providers.open_edit();
                false // edit dialog opened, not yet committed
            }
            SettingsTab::Permissions => self.permissions.activate(),
            SettingsTab::Hooks => {
                self.hooks.activate();
                false
            }
        };
        if changed {
            self.dirty = true;
        }
    }

    /// Commit an open provider edit dialog. Marks dirty if values changed.
    pub fn commit_provider_edit(&mut self) {
        if self.providers.commit_edit() {
            self.dirty = true;
        }
    }

    pub fn cancel_provider_edit(&mut self) {
        self.providers.cancel_edit();
    }

    pub fn push_char(&mut self, c: char) {
        match self.active_tab {
            SettingsTab::General => {
                self.general.push_char(c);
                self.dirty = true;
            }
            SettingsTab::Providers => {
                self.providers.push_char(c);
            }
            SettingsTab::Permissions => {
                self.permissions.push_char(c);
                self.dirty = true;
            }
            SettingsTab::Hooks => {} // readonly
        }
    }

    pub fn pop_char(&mut self) {
        match self.active_tab {
            SettingsTab::General => {
                self.general.pop_char();
                self.dirty = true;
            }
            SettingsTab::Providers => {
                self.providers.pop_char();
            }
            SettingsTab::Permissions => {
                self.permissions.pop_char();
                self.dirty = true;
            }
            SettingsTab::Hooks => {} // readonly
        }
    }

    pub fn toggle_reveal(&mut self) {
        self.providers.toggle_reveal();
    }

    pub fn toggle_provider_field(&mut self) {
        self.providers.toggle_field();
    }

    // ── Save flow ────────────────────────────────────────────────────────────

    /// Persist current state to `~/.oxicode/settings.toml`.
    ///
    /// Uses `toml::to_string_pretty` to write a clean TOML file.
    /// NEVER touches `~/.claude/settings.json`.
    ///
    /// Returns `Ok(())` on success; the caller should push a notification.
    pub fn save(&mut self) -> Result<(), String> {
        let settings_path = dirs::home_dir()
            .ok_or_else(|| "Cannot resolve home directory".to_string())?
            .join(".oxicode")
            .join("settings.toml");
        self.save_to(&settings_path)
    }

    /// Persist current state to the given path (creates parent dirs).
    ///
    /// Extracted from `save` so tests can pass a `tempfile` path without
    /// mutating env vars (which is unsound under parallel test execution).
    pub fn save_to(&mut self, settings_path: &std::path::Path) -> Result<(), String> {
        // Commit any open general edit first.
        self.general.commit_edit();

        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create config dir: {e}"))?;
        }

        // Load existing TOML text so we preserve unrelated keys.
        let existing_text = std::fs::read_to_string(settings_path).unwrap_or_default();

        // Build updated settings by patching the existing TOML.
        let updated = self.patch_toml(&existing_text)?;

        std::fs::write(settings_path, &updated)
            .map_err(|e| format!("Failed to write settings.toml: {e}"))?;

        self.dirty = false;
        self.save_message = Some("Saved to ~/.oxicode/settings.toml".to_string());
        Ok(())
    }

    /// Patch an existing TOML string with the current in-memory state.
    ///
    /// Strategy: deserialise → mutate struct → re-serialise. This preserves
    /// TOML structure for known keys. Comments are not preserved (toml crate
    /// limitation; `toml_edit` would preserve them but is not a workspace dep).
    ///
    /// Per spec risk note: `toml_edit` is preferred. If added to workspace later,
    /// this function can be upgraded to preserve comments. For v1, correctness
    /// over comment preservation is the right trade-off.
    fn patch_toml(&self, existing: &str) -> Result<String, String> {
        // Parse existing into a generic TOML value so unrecognised keys survive.
        let mut doc: toml::Value = if existing.trim().is_empty() {
            toml::Value::Table(toml::map::Map::new())
        } else {
            toml::from_str(existing)
                .map_err(|e| format!("Existing settings.toml is invalid TOML: {e}"))?
        };

        let table = doc
            .as_table_mut()
            .ok_or_else(|| "settings.toml root is not a TOML table".to_string())?;

        // Patch known fields from in-memory state.
        table.insert(
            "model".to_string(),
            toml::Value::String(self.general.model.clone()),
        );
        table.insert(
            "output_style".to_string(),
            toml::Value::String(self.general.output_format.clone()),
        );
        table.insert(
            "theme".to_string(),
            toml::Value::String(self.general.theme.clone()),
        );
        table.insert(
            "editor_mode".to_string(),
            toml::Value::String(if self.general.vim_mode {
                "vim".to_string()
            } else {
                "normal".to_string()
            }),
        );
        table.insert(
            "permission_mode".to_string(),
            toml::Value::String(self.permissions.mode.as_str().to_string()),
        );

        // api_key: write plaintext in v1 (keyring integration deferred).
        // Only write if the user has actually set one (don't overwrite with empty).
        let api_key_from_providers = self
            .providers
            .providers
            .first()
            .map(|p| p.auth_token.clone());
        if let Some(key) = api_key_from_providers {
            if !key.is_empty() {
                table.insert("api_key".to_string(), toml::Value::String(key));
            }
        }

        // Patch always_allow_tools / always_deny_tools from comma-separated text areas.
        let parse_tool_list = |s: &str| -> toml::Value {
            let tools: Vec<toml::Value> = s
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(|t| toml::Value::String(t.to_string()))
                .collect();
            toml::Value::Array(tools)
        };
        table.insert(
            "always_allow_tools".to_string(),
            parse_tool_list(&self.permissions.allow_list),
        );
        table.insert(
            "always_deny_tools".to_string(),
            parse_tool_list(&self.permissions.deny_list),
        );

        // Patch ghost_completion inside [features] sub-table.
        let features_table = table
            .entry("features".to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let Some(ft) = features_table.as_table_mut() {
            ft.insert(
                "ghost_completion".to_string(),
                toml::Value::Boolean(self.general.ghost_completion),
            );
        }

        toml::to_string_pretty(&doc).map_err(|e| format!("Failed to serialise settings: {e}"))
    }
}

// ── Hook loader ──────────────────────────────────────────────────────────────

/// Scan `~/.oxicode/settings.toml`, `~/.claude/settings.json`, and the project
/// `.oxicode/` / `.claude/` directories for hook definitions and return a flat
/// group list for the Hooks tab.
///
/// In v1 this is a best-effort scan; missing files are silently skipped.
fn load_hook_groups_from_disk() -> Vec<HookGroup> {
    let mut groups: std::collections::HashMap<String, Vec<HookEntry>> =
        std::collections::HashMap::new();

    // Try ~/.oxicode/settings.toml.
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".oxicode").join("settings.toml");
        if let Ok(text) = std::fs::read_to_string(&path) {
            scan_toml_hooks(
                &text,
                HookSource::UserOxicode,
                path.to_string_lossy().as_ref(),
                &mut groups,
            );
        }

        // Try ~/.claude/settings.json (read-only inspection, never written).
        let claude_path = home.join(".claude").join("settings.json");
        if let Ok(text) = std::fs::read_to_string(&claude_path) {
            scan_json_hooks(
                &text,
                HookSource::UserClaude,
                claude_path.to_string_lossy().as_ref(),
                &mut groups,
            );
        }
    }

    // Try project .oxicode/settings.toml.
    if let Ok(cwd) = std::env::current_dir() {
        let proj_path = cwd.join(".oxicode").join("settings.toml");
        if let Ok(text) = std::fs::read_to_string(&proj_path) {
            scan_toml_hooks(
                &text,
                HookSource::Project,
                proj_path.to_string_lossy().as_ref(),
                &mut groups,
            );
        }
    }

    // Convert to sorted HookGroups (deterministic display order).
    let mut event_names: Vec<String> = groups.keys().cloned().collect();
    event_names.sort();
    event_names
        .into_iter()
        .map(|event| {
            let entries = groups.remove(&event).unwrap_or_default();
            HookGroup::new(event, entries)
        })
        .collect()
}

/// Parse TOML `[hooks]` section and append entries into `groups`.
fn scan_toml_hooks(
    text: &str,
    source: HookSource,
    path: &str,
    groups: &mut std::collections::HashMap<String, Vec<HookEntry>>,
) {
    let Ok(doc) = toml::from_str::<toml::Value>(text) else {
        return;
    };
    let Some(hooks_val) = doc.get("hooks") else {
        return;
    };
    let Some(hooks_table) = hooks_val.as_table() else {
        return;
    };

    for (event, val) in hooks_table {
        let entries = groups.entry(event.clone()).or_default();
        // Support flat string, single table, or array-of-tables.
        match val {
            toml::Value::String(cmd) => {
                entries.push(HookEntry {
                    source: clone_source(&source),
                    matcher: String::new(),
                    command_summary: format!("command: {cmd}"),
                    timeout_secs: 10,
                    source_path: path.to_string(),
                });
            }
            toml::Value::Table(t) => {
                let cmd = t.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let timeout = t
                    .get("timeout_secs")
                    .and_then(|v| v.as_integer())
                    .unwrap_or(10);
                entries.push(HookEntry {
                    source: clone_source(&source),
                    matcher: String::new(),
                    command_summary: format!("command: {cmd}"),
                    timeout_secs: timeout as u64,
                    source_path: path.to_string(),
                });
            }
            toml::Value::Array(arr) => {
                for item in arr {
                    if let Some(t) = item.as_table() {
                        let matcher = t
                            .get("matcher")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        // Try to find a command in the nested `hooks` array.
                        let summary = if let Some(toml::Value::Array(inner)) = t.get("hooks") {
                            inner
                                .first()
                                .and_then(|h| {
                                    let ht = h.as_table()?;
                                    let cmd = ht.get("command").and_then(|v| v.as_str());
                                    let agent = ht.get("instructions").and_then(|v| v.as_str());
                                    cmd.map(|c| format!("command: {c}"))
                                        .or_else(|| agent.map(|_| "agent hook".to_string()))
                                })
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        let timeout = t
                            .get("timeout_secs")
                            .and_then(|v| v.as_integer())
                            .unwrap_or(10);
                        entries.push(HookEntry {
                            source: clone_source(&source),
                            matcher,
                            command_summary: summary,
                            timeout_secs: timeout as u64,
                            source_path: path.to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Parse JSON `hooks` key (Claude Code format) and append entries.
fn scan_json_hooks(
    text: &str,
    source: HookSource,
    path: &str,
    groups: &mut std::collections::HashMap<String, Vec<HookEntry>>,
) {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(hooks_obj) = doc.get("hooks").and_then(|v| v.as_object()) else {
        return;
    };

    for (event, val) in hooks_obj {
        let entries = groups.entry(event.clone()).or_default();
        if let Some(arr) = val.as_array() {
            for item in arr {
                let matcher = item
                    .get("matcher")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let summary = if let Some(inner_arr) = item.get("hooks").and_then(|v| v.as_array())
                {
                    inner_arr
                        .first()
                        .and_then(|h| {
                            h.get("command")
                                .and_then(|v| v.as_str())
                                .map(|c| format!("command: {c}"))
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                entries.push(HookEntry {
                    source: clone_source(&source),
                    matcher,
                    command_summary: summary,
                    timeout_secs: 10,
                    source_path: path.to_string(),
                });
            }
        }
    }
}

/// Clone a `HookSource` value (enum does not derive Clone to keep it simple).
fn clone_source(s: &HookSource) -> HookSource {
    match s {
        HookSource::UserOxicode => HookSource::UserOxicode,
        HookSource::UserClaude => HookSource::UserClaude,
        HookSource::Project => HookSource::Project,
    }
}

// ── Widget render ─────────────────────────────────────────────────────────────

/// Thin render wrapper so `App::draw()` can call `frame.render_widget(SettingsScreenWidget, area)`.
pub struct SettingsScreenWidget<'a> {
    screen: &'a SettingsScreen,
}

impl<'a> SettingsScreenWidget<'a> {
    pub fn new(screen: &'a SettingsScreen) -> Self {
        Self { screen }
    }
}

impl Widget for SettingsScreenWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Modal frame: width 90, height 32, header 3 rows, footer 1 row.
        let layout = begin_modal(buf, area, 90, 32, 3, 2);

        // Title bar.
        let dirty_marker = if self.screen.dirty { " ●" } else { "" };
        render_modal_title(
            buf,
            layout.header_area,
            &format!("Settings{dirty_marker}"),
            "oxicode",
        );

        // Tab bar on header row 2.
        if layout.header_area.height >= 2 {
            let tab_y = layout.header_area.y + 1;
            render_tab_bar(
                buf,
                layout.header_area.x,
                tab_y,
                layout.header_area.width,
                self.screen.active_tab,
            );
        }

        // Separator between header and body.
        render_separator(buf, layout.body_area);

        // Body: delegate to the active tab.
        let body = Rect {
            x: layout.body_area.x,
            y: layout.body_area.y + 1,
            width: layout.body_area.width,
            height: layout.body_area.height.saturating_sub(1),
        };

        match self.screen.active_tab {
            SettingsTab::General => self.screen.general.render(buf, body),
            SettingsTab::Providers => self.screen.providers.render(buf, body),
            SettingsTab::Permissions => self.screen.permissions.render(buf, body),
            SettingsTab::Hooks => self.screen.hooks.render(buf, body),
        }

        // Footer: keybinding hints + optional save message.
        render_footer(buf, layout.footer_area, self.screen);
    }
}

// ── Render helpers ────────────────────────────────────────────────────────────

fn render_tab_bar(buf: &mut Buffer, x: u16, y: u16, width: u16, active: SettingsTab) {
    let active_style = Style::default()
        .fg(DIALOG_TEXT)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let inactive_style = Style::default().fg(DIALOG_MUTED);
    let sep_style = Style::default().fg(DIALOG_MUTED);

    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, &tab) in SettingsTab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" \u{2502} ", sep_style));
        }
        spans.push(Span::styled(
            tab.label(),
            if tab == active {
                active_style
            } else {
                inactive_style
            },
        ));
    }

    let line = Line::from(spans);
    buf.set_line(x, y, &line, width);
}

fn render_footer(buf: &mut Buffer, area: Rect, screen: &SettingsScreen) {
    if area.height == 0 {
        return;
    }

    // If there's a save message, show it on the first footer row.
    if let Some(ref msg) = screen.save_message {
        let msg_line = Line::from(Span::styled(
            format!(" {msg}"),
            Style::default()
                .fg(DIALOG_ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
        buf.set_line(area.x, area.y, &msg_line, area.width);
    }

    // Keybinding hints on last footer row.
    let hint_y = area.y + area.height.saturating_sub(1);
    let dirty_hint = if screen.dirty { "  ● unsaved" } else { "" };
    let hint = Line::from(vec![
        Span::styled(
            " [Tab] switch  [↑↓] navigate  [Enter] select  [Ctrl+S] save  [Esc] close",
            Style::default().fg(DIALOG_MUTED),
        ),
        Span::styled(dirty_hint, Style::default().fg(DIALOG_ACCENT)),
    ]);
    buf.set_line(area.x, hint_y, &hint, area.width);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn make_screen() -> SettingsScreen {
        let settings = oxicode_config::Settings::default();
        SettingsScreen::from_config(&settings)
    }

    fn render_to_buf(screen: &SettingsScreen, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let _backend = TestBackend::new(width, height);
        let _terminal = Terminal::new(_backend).unwrap();
        let mut buf =
            ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, width, height));
        let widget = SettingsScreenWidget::new(screen);
        widget.render(ratatui::layout::Rect::new(0, 0, width, height), &mut buf);
        buf
    }

    #[test]
    fn general_tab_renders_without_panic() {
        let screen = make_screen();
        let buf = render_to_buf(&screen, 100, 35);
        // Should contain the "Settings" title word.
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(
            content.contains("Settings"),
            "title not found in rendered buffer"
        );
    }

    #[test]
    fn tab_bar_shows_all_tabs() {
        let screen = make_screen();
        let buf = render_to_buf(&screen, 100, 35);
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(content.contains("General"), "General tab label missing");
        assert!(content.contains("Providers"), "Providers tab label missing");
        assert!(
            content.contains("Permissions"),
            "Permissions tab label missing"
        );
        assert!(content.contains("Hooks"), "Hooks tab label missing");
    }

    #[test]
    fn providers_tab_renders() {
        let mut screen = make_screen();
        screen.active_tab = SettingsTab::Providers;
        let buf = render_to_buf(&screen, 100, 35);
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(content.contains("Providers"), "Providers label missing");
    }

    #[test]
    fn permissions_tab_renders_radio_labels() {
        let mut screen = make_screen();
        screen.active_tab = SettingsTab::Permissions;
        let buf = render_to_buf(&screen, 100, 35);
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(content.contains("bypass"), "bypass mode label missing");
        assert!(content.contains("default"), "default mode label missing");
    }

    #[test]
    fn hooks_tab_renders() {
        let mut screen = make_screen();
        screen.active_tab = SettingsTab::Hooks;
        let buf = render_to_buf(&screen, 100, 35);
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        // The readonly notice should always appear.
        assert!(content.contains("Readonly"), "readonly notice missing");
    }

    #[test]
    fn footer_shows_keybindings() {
        let screen = make_screen();
        let buf = render_to_buf(&screen, 100, 35);
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(content.contains("Ctrl+S"), "Ctrl+S save hint missing");
        assert!(content.contains("Esc"), "Esc close hint missing");
    }

    #[test]
    fn dirty_flag_set_on_field_change() {
        let mut screen = make_screen();
        assert!(!screen.dirty);
        // Toggle vim mode — should set dirty.
        screen.activate(); // General tab, Model field selected — opens edit mode; not dirty yet
                           // Navigate to vim mode field (index 3).
        screen.general.selected = 3;
        screen.activate(); // Toggle vim mode.
        assert!(screen.dirty, "dirty should be set after field toggle");
    }

    #[test]
    fn secret_masking_in_providers() {
        let mut screen = make_screen();
        // Set a token in the first provider.
        if let Some(p) = screen.providers.providers.first_mut() {
            p.auth_token = "sk-ant-secret-123".to_string();
            p.status = ProviderStatus::Present;
        }
        screen.active_tab = SettingsTab::Providers;
        let buf = render_to_buf(&screen, 100, 35);
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        // Raw token must NOT appear in rendered output.
        assert!(
            !content.contains("sk-ant-secret-123"),
            "token leaked into render buffer"
        );
        // Mask character must appear instead.
        assert!(content.contains('●'), "mask character not found");
    }

    #[test]
    fn save_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("settings.toml");

        let settings = oxicode_config::Settings::default();
        let mut screen = SettingsScreen::from_config(&settings);
        screen.general.model = "claude-opus-4-test".to_string();
        screen.dirty = true;

        // Use save_to — no env mutation, safe under parallel test execution.
        screen
            .save_to(&config_path)
            .expect("save_to should succeed");

        // Reload and verify.
        let reloaded = oxicode_config::load_settings(Some(tmp.path().to_str().unwrap()));
        assert_eq!(reloaded.model, "claude-opus-4-test", "model not persisted");
    }

    #[test]
    fn save_round_trip_allow_deny_lists() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("settings.toml");

        let settings = oxicode_config::Settings::default();
        let mut screen = SettingsScreen::from_config(&settings);
        screen.permissions.allow_list = "bash, file_read".to_string();
        screen.permissions.deny_list = "file_write".to_string();
        screen.dirty = true;

        screen
            .save_to(&config_path)
            .expect("save_to should succeed");

        let reloaded = oxicode_config::load_settings(Some(tmp.path().to_str().unwrap()));
        assert_eq!(
            reloaded.always_allow_tools,
            vec!["bash".to_string(), "file_read".to_string()],
            "allow_list not persisted"
        );
        assert_eq!(
            reloaded.always_deny_tools,
            vec!["file_write".to_string()],
            "deny_list not persisted"
        );
    }

    #[test]
    fn patch_toml_preserves_unrelated_keys() {
        let existing = r#"
config_version = 3
max_tokens = 8192
model = "claude-sonnet-old"
"#;
        let settings = oxicode_config::Settings::default();
        let screen = SettingsScreen::from_config(&settings);
        let out = screen.patch_toml(existing).unwrap();
        // Unrelated key max_tokens should survive.
        assert!(out.contains("max_tokens"), "max_tokens was dropped");
        // config_version should survive.
        assert!(out.contains("config_version"), "config_version was dropped");
    }

    #[test]
    fn tab_cycle_wraps() {
        let mut screen = make_screen();
        assert_eq!(screen.active_tab, SettingsTab::General);
        screen.next_tab();
        assert_eq!(screen.active_tab, SettingsTab::Providers);
        screen.next_tab();
        assert_eq!(screen.active_tab, SettingsTab::Permissions);
        screen.next_tab();
        assert_eq!(screen.active_tab, SettingsTab::Hooks);
        screen.next_tab();
        assert_eq!(screen.active_tab, SettingsTab::General, "tab should wrap");
    }

    #[test]
    fn permission_mode_radio_activate() {
        let mut screen = make_screen();
        screen.active_tab = SettingsTab::Permissions;
        // Navigate to Bypass (index 2).
        screen.permissions.focus = super::super::settings::PermFocus::Radio(2);
        screen.activate();
        assert_eq!(screen.permissions.mode, DisplayPermMode::Bypass);
        assert!(screen.dirty);
    }
}
