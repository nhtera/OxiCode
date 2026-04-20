/// App root module: App struct, new(), private types, and submodule declarations.
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use oxicode_state::{AppState, StateStore};
use ratatui::layout::Rect;
use tokio::sync::{mpsc, watch};

use crate::events::{CoreEvent, UiEvent};
use crate::message_queue::MessageQueue;
use crate::keybindings::KeybindingRegistry;
use crate::prompt_suggestions::PromptSuggestion;
use crate::streaming_markdown::MarkdownStreamCollector;
use crate::vim_mode::VimState;
use crate::widgets::{
    permission_dialog::RiskLevel, AutocompleteState, DiffViewerState, HistorySearchState,
    MessageRenderCache, ModelPickerState, Notification, RewindOverlayState, SearchOverlay,
    SessionBrowserState, ShortcutsState, SlashCommandMeta, SplitPane, StatsDashboardState,
};

// ── Submodules ────────────────────────────────────────────────────────────────
pub(crate) mod utils;
mod state;
mod event_loop;
mod render;
mod key_dispatch;
mod overlay_keys;
mod streaming;
mod permission_handler;
mod hook_display;
#[cfg(test)]
mod tests;

// ── Private types ─────────────────────────────────────────────────────────────

/// A tool call in progress (between ToolUseStart and ToolResult events).
pub(crate) struct ActiveToolCall {
    id: String,
    name: String,
    input_summary: String,
    /// Raw input JSON for tool-specific rendering (e.g., Bash shows command).
    raw_input: serde_json::Value,
    /// When this tool call started (for elapsed time display).
    started_at: std::time::Instant,
    /// `Some((content, is_error))` when the tool has completed.
    result: Option<(String, bool)>,
}

/// Result from `find_image_tag_at()` — identifies an `[Image #N]` span hit.
pub(crate) struct ImageTagHit {
    /// Global image number (1-based, from `[Image #N]`).
    image_number: usize,
}

/// Maximum length of a hook message rendered in the TUI.
/// Hook content is untrusted — truncate to avoid overrunning the transcript.
pub(crate) const MAX_HOOK_LINE_CHARS: usize = 200;

/// Persistent record of a hook message (kept for transcript / scrollback).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields read by tests + reserved for future transcript scrollback.
pub(crate) struct HookLogEntry {
    event: String,
    kind: HookKind,
    content: String,
}

/// TUI-local mirror of `oxicode_core::HookMessageKind` — drives color choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookKind {
    System,
    BlockError,
    Context,
}

impl HookKind {
    fn parse(s: &str) -> Self {
        match s {
            "block_error" => Self::BlockError,
            "context" => Self::Context,
            _ => Self::System,
        }
    }
}

/// Truncate hook content for display. Hooks are untrusted; cap at
/// `MAX_HOOK_LINE_CHARS` and append ellipsis when over limit.
pub(crate) fn truncate_hook_content(s: &str, max: usize) -> String {
    let cleaned: String = s.chars().filter(|c| *c != '\x1b').collect();
    if cleaned.chars().count() <= max {
        return cleaned;
    }
    let mut out: String = cleaned.chars().take(max).collect();
    out.push('…');
    out
}

/// A pending permission request awaiting user response.
pub(crate) struct PendingPermission {
    tool_name: String,
    input_summary: String,
    prompt: String,
    selected: usize,
    /// Risk level derived from tool name and input (drives dialog border color).
    risk_level: RiskLevel,
    /// Tool-specific dialog variant (drives options and content layout).
    kind: crate::widgets::PermissionDialogKind,
    /// When this permission request was created (for countdown timer).
    created_at: Instant,
    reply_tx: tokio::sync::oneshot::Sender<oxicode_common::PermissionResponse>,
}

// ── App struct ────────────────────────────────────────────────────────────────

/// Main TUI application.
pub struct App {
    pub(super) state_rx: watch::Receiver<AppState>,
    pub(super) ui_tx: mpsc::Sender<UiEvent>,
    pub(super) core_rx: mpsc::Receiver<CoreEvent>,
    /// Direct cancel flag shared with engine task for immediate Esc interrupt.
    pub(super) cancel_flag: Option<Arc<AtomicBool>>,
    pub(super) input_text: String,
    /// Cursor position as character index (not byte index).
    pub(super) input_cursor: usize,
    /// Current message-view scroll offset from top.
    pub(super) scroll_offset: u16,
    /// When true, keep message view pinned to bottom as new content arrives.
    pub(super) auto_scroll: bool,
    /// Last computed max scroll offset for the current viewport/content.
    pub(super) max_scroll_offset: u16,
    pub(super) streaming_text: String,
    /// Newline-gated markdown stream collector for incremental rendering.
    pub(super) streaming_collector: MarkdownStreamCollector,
    /// Pre-rendered committed lines from streaming collector.
    pub(super) streaming_committed_lines: Vec<ratatui::text::Line<'static>>,
    pub(super) should_quit: bool,
    /// True from StreamStart until MessageComplete — tracks whether a turn is active.
    pub(super) is_turn_active: bool,
    /// Messages queued by the user while a turn is active — drained FIFO on turn complete.
    pub(super) message_queue: MessageQueue,
    /// Manages left/right split layout and ratio.
    pub(super) split_pane: SplitPane,
    /// Toast notifications rendered as an overlay.
    pub(super) notifications: Vec<Notification>,
    /// Tool calls in progress during the current turn.
    pub(super) active_tools: Vec<ActiveToolCall>,
    /// Permission dialog state (blocks input while active).
    pub(super) pending_permission: Option<PendingPermission>,
    /// Vim mode state machine.
    pub(super) vim: VimState,
    /// Keybinding registry.
    pub(super) keybindings: KeybindingRegistry,
    /// Search overlay state.
    pub(super) search: SearchOverlay,
    /// Shortcuts panel visibility.
    pub(super) shortcuts: ShortcutsState,
    /// Command history (most recent last).
    pub(super) history: oxicode_session::prompt_history::PersistentHistory,
    /// Current position in history navigation (-1 = not navigating).
    pub(super) history_index: Option<usize>,
    /// Saved input before history navigation started.
    pub(super) history_saved_input: String,
    /// Reverse history search overlay state (Ctrl+R).
    pub(super) history_search: Option<HistorySearchState>,
    /// Timestamp of last interrupt (for double Ctrl+C force quit).
    pub(super) last_interrupt: Option<Instant>,
    /// Whether to show "Press Ctrl-C again to exit" hint below input box.
    pub(super) ctrl_c_hint_visible: bool,
    /// Per-message render cache — avoids re-parsing markdown for unchanged messages.
    pub(super) message_cache: MessageRenderCache,
    /// Ghost text completion suffix (shown dimmed after cursor).
    pub(super) ghost_text: Option<String>,
    /// Large paste text awaiting confirmation (shown in preview modal).
    pub(super) pending_paste: Option<String>,
    /// Pasted images waiting to be sent with the next message.
    pub(super) pending_images: Vec<crate::image_paste::PastedImage>,
    /// Image file paths keyed by global image number (1-based, session-wide).
    /// Used for click-to-open and OSC 8 hyperlinks.
    pub(super) sent_image_paths: std::collections::HashMap<usize, std::path::PathBuf>,
    /// Context-aware follow-up suggestions shown as chips.
    pub(super) suggestions: Vec<PromptSuggestion>,
    /// Timestamp when the current turn started (for thinking indicator).
    pub(super) turn_started_at: Option<Instant>,
    /// Slash command metadata for autocomplete dropdown.
    pub(super) slash_commands: Vec<SlashCommandMeta>,
    /// Pre-built (name, description, category) tuples for help overlay right column.
    pub(super) help_commands: Vec<(String, String, String)>,
    /// Pre-built keyboard shortcut entries for help overlay left column.
    pub(super) help_shortcuts: Vec<crate::widgets::shortcuts_overlay::ShortcutEntry>,
    /// Autocomplete dropdown state (active when typing `/...`).
    pub(super) autocomplete: AutocompleteState,
    /// Model picker overlay state.
    pub(super) model_picker: ModelPickerState,
    /// Session browser overlay state.
    pub(super) session_browser: SessionBrowserState,
    /// Rewind overlay state (interactive turn selector for /rewind).
    pub(super) rewind_overlay: RewindOverlayState,
    /// Stats dashboard overlay state (session cost/token overview).
    pub(super) stats_dashboard: StatsDashboardState,
    /// Interactive diff viewer overlay state (two-pane file diff review).
    pub(super) diff_viewer: DiffViewerState,
    /// Cached message area rect from last draw (for scrollbar hit-testing).
    pub(super) message_area: Rect,
    /// Frame counter — incremented each draw call, drives spinner animation.
    pub(super) frame_count: u64,
    /// Session start time for elapsed display in status bar.
    pub(super) session_start: Instant,
    /// Set when streaming starts, reset on each delta. If >3s since last delta,
    /// spinner turns red (stall detection).
    pub(super) stall_start: Option<Instant>,
    /// Thinking text accumulated during streaming (from ThinkingDelta events).
    pub(super) streaming_thinking: String,
    /// Duration of the last completed turn (set on MessageComplete).
    pub(super) last_turn_duration: Option<Duration>,
    /// Session-local bash command prefix allowlist. When a user approves a
    /// command with the 'p' (prefix) option, the first word is stored here.
    /// Future bash commands starting with that word are auto-approved.
    pub(super) bash_prefix_allowlist: std::collections::HashSet<String>,
    /// Current retry/rate-limit label shown in status bar (empty when not retrying).
    /// Set by CoreEvent::Retrying / CoreEvent::RateLimited, cleared on StreamStart.
    pub(super) retry_status_label: String,
    /// Name of the hook currently executing (if any). Drives the transient
    /// "Running {hook} hook…" status line. Cleared on HookProgress::Completed.
    pub(super) active_hook: Option<String>,
    /// Persistent log of hook messages surfaced this session — used for
    /// diagnostics and future transcript rendering. Capped to avoid growth.
    pub(super) hook_messages: Vec<HookLogEntry>,
}

impl App {
    pub fn new(
        state_store: &Arc<StateStore>,
        ui_tx: mpsc::Sender<UiEvent>,
        core_rx: mpsc::Receiver<CoreEvent>,
        slash_commands: Vec<SlashCommandMeta>,
    ) -> Self {
        let keybindings = KeybindingRegistry::with_defaults();

        // Build help overlay data from slash_commands.
        let help_commands: Vec<(String, String, String)> = slash_commands
            .iter()
            .map(|c| (c.name.clone(), c.description.clone(), c.category.clone()))
            .collect();
        let help_shortcuts = crate::widgets::shortcuts_overlay::default_shortcut_entries();

        Self {
            state_rx: state_store.subscribe(),
            ui_tx,
            core_rx,
            input_text: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            auto_scroll: true,
            max_scroll_offset: 0,
            streaming_text: String::new(),
            streaming_collector: MarkdownStreamCollector::new(),
            streaming_committed_lines: Vec::new(),
            should_quit: false,
            is_turn_active: false,
            message_queue: MessageQueue::default(),
            split_pane: SplitPane::new(),
            notifications: Vec::new(),
            active_tools: Vec::new(),
            pending_permission: None,
            vim: VimState::new(false),
            keybindings,
            search: SearchOverlay::new(),
            shortcuts: ShortcutsState::new(),
            history: oxicode_session::prompt_history::PersistentHistory::load(None),
            history_index: None,
            history_saved_input: String::new(),
            history_search: None,
            last_interrupt: None,
            ctrl_c_hint_visible: false,
            message_cache: MessageRenderCache::new(),
            ghost_text: None,
            pending_paste: None,
            pending_images: Vec::new(),
            sent_image_paths: std::collections::HashMap::new(),
            suggestions: Vec::new(),
            turn_started_at: None,
            slash_commands,
            help_commands,
            help_shortcuts,
            autocomplete: AutocompleteState::new(),
            model_picker: ModelPickerState::new(),
            session_browser: SessionBrowserState::new(),
            rewind_overlay: RewindOverlayState::new(),
            stats_dashboard: StatsDashboardState::new(),
            diff_viewer: DiffViewerState::new(),
            message_area: Rect::default(),
            frame_count: 0,
            session_start: Instant::now(),
            stall_start: None,
            streaming_thinking: String::new(),
            last_turn_duration: None,
            cancel_flag: None,
            bash_prefix_allowlist: std::collections::HashSet::new(),
            retry_status_label: String::new(),
            active_hook: None,
            hook_messages: Vec::new(),
        }
    }

    /// Set the cancel flag shared with the engine task.
    pub fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>) {
        self.cancel_flag = Some(flag);
    }

    /// Rebuild `sent_image_paths` from the resumed session's messages.
    pub fn hydrate_image_paths_from_session(&mut self) {
        use base64::Engine as _;

        let (session_id, messages) = {
            let state = self.state_rx.borrow();
            (state.session_id.clone(), state.messages.clone())
        };
        if messages.is_empty() {
            return;
        }

        let cache_dir = crate::image_paste::image_cache_dir(&session_id);
        let mut counter: usize = 0;
        for msg in &messages {
            for block in &msg.content {
                if let oxicode_common::ContentBlock::Image { source } = block {
                    counter += 1;
                    let path = cache_dir.join(format!("{counter}.png"));
                    if !path.exists() {
                        if let Some(b64) = source.data.as_deref() {
                            if let Ok(bytes) =
                                base64::engine::general_purpose::STANDARD.decode(b64)
                            {
                                let _ = std::fs::write(&path, bytes);
                            }
                        }
                    }
                    if path.exists() {
                        self.sent_image_paths.insert(counter, path);
                    }
                }
            }
        }
    }

    /// Enable vim mode at runtime.
    pub fn set_vim_mode(&mut self, enabled: bool) {
        self.vim.set_enabled(enabled);
    }

    /// Load user keybindings from a TOML file.
    pub fn load_keybindings(&mut self, path: &std::path::Path) {
        self.keybindings.load_from_file(path);
    }
}
