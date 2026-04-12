# OxiCode TUI Architecture

## Table of Contents
1. [Architecture Overview](#architecture-overview)
2. [App Struct](#app-struct)
3. [Event System](#event-system)
4. [Main Event Loop](#main-event-loop)
5. [Keyboard Handling](#keyboard-handling)
6. [Vim Mode](#vim-mode)
7. [Widgets](#widgets)
8. [Rendering Pipeline](#rendering-pipeline)
9. [Special Features](#special-features)

---

## Architecture Overview

The OxiCode TUI is built on **ratatui + crossterm** with an event-driven channel topology. The application follows a three-layer communication model:

```
┌─────────────────────────────────────────────┐
│     Crossterm Terminal Event Listener       │ (dedicated blocking thread)
├─────────────────────────────────────────────┤
│          Terminal Event (mpsc)              │ (50ms poll, 32-capacity)
├─────────────────────────────────────────────┤
│                                             │
│  ┌─────────────────────────────────────┐   │
│  │     App (Main Event Loop)           │   │
│  ├─────────────────────────────────────┤   │
│  │  Channels:                          │   │
│  │  - state_rx (watch → AppState)      │   │
│  │  - ui_tx (mpsc UiEvent → core)      │   │
│  │  - core_rx (mpsc CoreEvent ← core)  │   │
│  │  - cancel_flag (Arc<AtomicBool>)    │   │
│  │                                     │   │
│  │  View State:                        │   │
│  │  - input_text, input_cursor        │   │
│  │  - scroll_offset, auto_scroll      │   │
│  │  - streaming_collector             │   │
│  │  - active_tools, overlays          │   │
│  │  - vim, keybindings                │   │
│  │                                     │   │
│  │  Render:                            │   │
│  │  - Terminal (ratatui backend)       │   │
│  │  - frame_count (spinner animation) │   │
│  │  - message_cache (render perf)      │   │
│  └─────────────────────────────────────┘   │
│                                             │
└─────────────────────────────────────────────┘
```

**Channel Topology:**
- **UiEvent → Core** (mpsc): User input, slash commands, quit, interrupts, scrolling
- **CoreEvent ← Core** (mpsc): Text/thinking deltas, tool calls, permission requests, errors, streaming lifecycle
- **AppState** (watch): Centralized state broadcast to TUI for display (messages, model, usage, agents, tasks)
- **cancel_flag** (Arc<AtomicBool>): Direct interrupt signal during streaming (set immediately, bypasses channel latency)

**Backend Stack:**
- **Terminal**: Crossterm event polling (50ms tick, max 32 events buffered)
- **Rendering**: ratatui with split-pane layout support
- **Threading**: Main async task + dedicated blocking thread for terminal events
- **Select Loop**: `tokio::select!` polls terminal events, core messages, and timers concurrently

(source: crates/oxicode-tui/src/app.rs:1-160, lib.rs)

---

## App Struct

The main `App` struct aggregates state for input, rendering, overlays, and coordination:

```rust
pub struct App {
    // Channels
    state_rx: watch::Receiver<AppState>,
    ui_tx: mpsc::Sender<UiEvent>,
    core_rx: mpsc::Receiver<CoreEvent>,
    cancel_flag: Option<Arc<AtomicBool>>,
    
    // Input handling
    input_text: String,
    input_cursor: usize,
    history: oxicode_session::prompt_history::PersistentHistory,
    history_index: Option<usize>,
    history_saved_input: String,
    history_search: Option<HistorySearchState>,
    vim: VimState,
    keybindings: KeybindingRegistry,
    ghost_text: Option<String>,
    
    // View state (scroll, layout, cache)
    scroll_offset: u16,
    auto_scroll: bool,
    max_scroll_offset: u16,
    split_pane: SplitPane,
    message_area: Rect,
    
    // Streaming & markdown rendering
    streaming_text: String,
    streaming_collector: MarkdownStreamCollector,
    streaming_committed_lines: Vec<ratatui::text::Line<'static>>,
    streaming_thinking: String,
    is_turn_active: bool,
    turn_started_at: Option<Instant>,
    stall_start: Option<Instant>,
    last_turn_duration: Option<Duration>,
    
    // Overlays & dialogs (modal state)
    pending_permission: Option<PendingPermission>,
    search: SearchOverlay,
    shortcuts: ShortcutsState,
    pending_paste: Option<String>,
    autocomplete: AutocompleteState,
    model_picker: ModelPickerState,
    session_browser: SessionBrowserState,
    
    // Images & paste handling
    pending_images: Vec<crate::image_paste::PastedImage>,
    sent_image_paths: std::collections::HashMap<usize, std::path::PathBuf>,
    
    // Tool tracking
    active_tools: Vec<ActiveToolCall>,
    
    // UI elements
    notifications: Vec<Notification>,
    suggestions: Vec<PromptSuggestion>,
    message_cache: MessageRenderCache,
    slash_commands: Vec<SlashCommandMeta>,
    help_commands: Vec<(String, String, String)>,
    help_shortcuts: Vec<crate::widgets::shortcuts_overlay::ShortcutEntry>,
    
    // Animation & UI feedback
    frame_count: u64,
    session_start: Instant,
    last_interrupt: Option<Instant>,
    
    // Lifecycle
    should_quit: bool,
    cancel_flag: Option<Arc<AtomicBool>>,
}
```

**Field Groups by Purpose:**

1. **Channels** — Integration with core engine and state store
2. **Input** — Text buffer, cursor, history navigation, vim mode, keybindings
3. **View** — Scroll position, split layout, cached rendering bounds
4. **Streaming** — Markdown collector, committed lines, thinking text, turn tracking
5. **Overlays** — Modal dialogs: permissions, search, history search, model picker, session browser
6. **Paste** — Pending paste buffer, image attachments, file path mapping (for click-to-open)
7. **Tools** — Active tool calls with elapsed time and pending results
8. **UI** — Toast notifications, suggestions, command metadata, help overlays
9. **Animation** — Frame counter for spinner, timestamps for duration display

(source: crates/oxicode-tui/src/app.rs:35-159)

---

## Event System

### UiEvent Enum (User → Core)

Events flowing from TUI to the core engine:

```rust
pub enum UiEvent {
    /// User submitted input text (optionally with pasted images).
    UserInput {
        text: String,
        images: Vec<crate::image_paste::PastedImage>,
    },
    /// Slash command parsed from user input.
    SlashCommand { name: String, args: String },
    /// User requested quit.
    Quit,
    /// User requested interrupt of the current turn (Ctrl+C during streaming).
    InterruptTurn,
    /// Terminal resized.
    Resize(u16, u16),
    /// Scroll up in message view.
    ScrollUp,
    /// Scroll down in message view.
    ScrollDown,
}
```

### CoreEvent Enum (Core → TUI)

Events flowing from core engine to TUI for rendering:

```rust
pub enum CoreEvent {
    /// New text delta from streaming response.
    TextDelta(String),
    /// Streaming started.
    StreamStart,
    /// Streaming completed.
    StreamEnd,
    /// Error to display to user.
    Error(String),
    /// New assistant message completed.
    MessageComplete,
    /// A tool call is about to execute.
    ToolUseStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// A tool call completed.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// Permission required — TUI must show dialog and send response.
    PermissionAsk {
        tool_name: String,
        input_summary: String,
        prompt: String,
        reply_tx: tokio::sync::oneshot::Sender<oxicode_common::PermissionResponse>,
    },
    /// Rate limited — provider returned 429, retry in progress.
    RateLimited {
        message: String,
        attempt: u32,
        max_retries: u32,
        retry_in_secs: f64,
    },
    /// Non-rate-limit retry in progress (e.g., 502, connection error).
    Retrying {
        message: String,
        attempt: u32,
        max_retries: u32,
        retry_in_secs: f64,
    },
    /// Thinking text delta from extended thinking / chain-of-thought.
    ThinkingDelta(String),
}
```

**Note:** `CoreEvent` cannot derive `Clone` due to `reply_tx` (oneshot sender). Each event is consumed once.

(source: crates/oxicode-tui/src/events.rs)

---

## Main Event Loop

The event loop is the core of the TUI. It coordinates terminal input, engine messages, and timers:

```
┌─────────────────────────────────────────────────────────────┐
│ run() → terminal setup → install panic hook → raw mode      │
├─────────────────────────────────────────────────────────────┤
│ spawn_terminal_event_listener() → dedicated blocking thread │
│   └─ polls crossterm::event::poll() every 50ms              │
│   └─ sends Event to term_rx (mpsc channel)                  │
├─────────────────────────────────────────────────────────────┤
│ event_loop() — main async loop                              │
│   ┌───────────────────────────────────────────────────────┐ │
│   │ tokio::select!:                                       │ │
│   │                                                       │ │
│   │ Terminal Event → handle_key() / handle_mouse()       │ │
│   │                  handle_paste() → draw()             │ │
│   │                                                       │ │
│   │ CoreEvent → handle_core_event() → batch recv pending │ │
│   │             updates → draw()                         │ │
│   │                                                       │ │
│   │ Timer Tick → tick animation, timeout overlays        │ │
│   │             → draw()                                 │ │
│   └───────────────────────────────────────────────────────┘ │
│                                                               │
│   draw(terminal)                                              │
│   ├─ Layout computation (split panes, regions)               │
│   ├─ Render dispatch (input box, message view, overlays)    │
│   ├─ Flush to terminal                                       │
│   └─ Track message_area Rect for scrollbar hit-testing       │
└─────────────────────────────────────────────────────────────┘
```

**Event Loop Timing:**
- **Dynamic tick**: 50ms when `is_turn_active` (smooth spinner), 100ms idle (lower CPU)
- **`tokio::select!` priority**: Terminal events processed first, then core events, then timer
- **Batching**: `while let Ok(ev) = core_rx.try_recv()` drains all pending core events before redraw

**Key Methods:**
- `async fn run()` — Terminal setup, event listener spawn, panic hook, main loop coordination
- `fn spawn_terminal_event_listener()` — Dedicated blocking thread for crossterm polling
- `async fn event_loop()` — Main select loop, draw coordination
- `async fn handle_key(key: KeyEvent)` — Keyboard dispatch (see Keyboard Handling section)
- `fn handle_core_event(core_event: CoreEvent)` — Process engine messages, update state
- `async fn signal_interrupt()` — Set cancel flag + send InterruptTurn + reset UI state

(source: crates/oxicode-tui/src/app.rs:257-349)

---

## Keyboard Handling

Keyboard input flows through a 10-layer priority stack. Each layer can consume the key or pass it down:

```
Priority 1: Permission Dialog
   └─ Consume: y/n/Tab/Enter for response, Esc to cancel
   └─ Pass: Tab cycles between buttons

Priority 2: Search Overlay
   └─ Consume: text input, Esc to close
   └─ Pass: Up/Down for history

Priority 3: History Search (Ctrl+R)
   └─ Consume: text input, Up/Down in results, Enter to select
   └─ Pass: Esc to cancel

Priority 4: Session Browser
   └─ Consume: Up/Down to navigate, Enter to load, q to close
   └─ Pass: other keys

Priority 5: Model Picker
   └─ Consume: Up/Down to select, Enter to switch, q to close
   └─ Pass: / for filter

Priority 6: Paste Preview Modal
   └─ Consume: y/n to confirm/reject, Esc to cancel
   └─ Pass: none

Priority 7: Autocomplete Dropdown
   └─ Consume: Up/Down to navigate, Enter to insert, Esc to close
   └─ Pass: Tab (cycle), space (dismiss)

Priority 8: Pager (Shortcuts Panel)
   └─ Consume: q to close, PgUp/PgDn to scroll
   └─ Pass: other keys

Priority 9: Keybinding Registry
   └─ Match key combo → Action dispatch (submit, scroll, toggle vim, etc.)
   └─ Pass: unbound keys to priority 10

Priority 10: Input Box (handle_key_inner)
   └─ Vim mode OR regular input mode
   └─ Consume: all keys for text manipulation or vim operations
```

**Key Flow for Regular Input (Priority 10):**

1. **Vim mode enabled?** → `vim.handle_key()` returns `VimAction`
   - Map action to text manipulation (insert char, delete, move cursor, etc.)
   
2. **Vim mode disabled?** → `handle_key_inner()`
   - `Char(c)` → insert at cursor
   - `Enter` → submit (if not multiline) or insert newline
   - `Tab` → show autocomplete or insert spaces
   - `Backspace` → delete before cursor
   - `Delete` → delete at cursor
   - `Left/Right/Home/End` → move cursor
   - `Ctrl+A/E/U/K` → home/end/delete line/delete to end
   - `Ctrl+W` → delete word backward
   - `Ctrl+R` → history search
   - `Up/Down` → history navigation
   - `Esc` → clear suggestions, dismiss autocomplete

**Helper Functions:**
- `handle_key_inner()` — Raw input handling after all overlays
- `handle_key_vim()` — Vim action dispatch to text operations
- `handle_mouse(mouse: MouseEvent)` — Scrollbar, link clicks, selection
- `is_overlay_active()` — Check if any modal blocks input

(source: crates/oxicode-tui/src/app.rs:750+, keybindings.rs)

---

## Vim Mode

Vim mode implements Normal, Insert, Visual, Command modes with standard motions and operators.

### Mode Machine

```
┌──────────┐
│ Insert   │◄────────────────────────────────────┐
│ (text    │                                      │
│ input)   │                                      │
└─────┬────┘                                      │
      │ Esc                                       │
      ▼                                           │
┌──────────┐  /           ┌─────────┐            │
│ Normal   │─────────────►│ Command │            │
│ (nav,    │              │ (: cmd) │            │
│ operators│              └─────────┘            │
└─────┬────┘  i, a, I, A, o, O, c, s             │
      │       ◄─────────────────────────          │
      │ V                                         │
      ▼                                           │
┌──────────┐              ┌─────────┐            │
│ Visual   │              │ Visual  │            │
│ (char    │─────────────►│ Line    │            │
│ select)  │     V        │ (line   │            │
└──────────┘              │ select) │            │
                          └─────────┘            │
```

### VimState Struct

```rust
pub struct VimState {
    pub mode: Mode,                    // Insert | Normal | Visual | VisualLine | Command
    pending_op: Option<char>,          // Operator waiting for motion (d, c, y, etc.)
    pending_text_obj: Option<char>,    // Text object modifier (i for inner, a for around)
    count_buf: String,                 // Count prefix (e.g., "3" in "3dd")
    command_buf: String,               // Command-line buffer (after ":")
    yank_register: String,             // Single line register for yanking
    pub visual_anchor: usize,          // Selection anchor position
    pub enabled: bool,                 // Vim mode on/off
}
```

### VimAction Enum (Result of handle_key)

```rust
pub enum VimAction {
    // Movement
    MoveCursor(usize),                 // Absolute position
    MoveCursorBy(isize),               // Relative offset
    MoveToLineStart, MoveToLineEnd,
    MoveToStart, MoveToEnd,            // gg, G
    MoveWordForward(usize),            // w with count
    MoveWordBackward(usize),           // b with count
    MoveWordEnd(usize),                // e with count
    
    // Insertion & deletion
    InsertChar(char),                  // Insert at cursor
    DeleteChar,                        // Delete at cursor
    DeleteCharBefore,                  // Backspace
    DeleteLine,                        // dd
    DeleteToEnd,                       // D
    DeleteWordForward,                 // dw
    DeleteWordBackward,                // db
    DeleteRange(usize, usize),         // d<motion>
    DeleteTextObject(char, char),      // di", da(, etc.
    
    // Change (delete + insert)
    ChangeToEnd,                       // C
    ChangeRange(usize, usize),         // c<motion>
    ChangeTextObject(char, char),      // ci", ca(, etc.
    
    // Yank (copy)
    YankLine,                          // yy
    YankRange(usize, usize),           // y<motion>
    YankTextObject(char, char),        // yi", ya(, etc.
    
    // Paste
    Paste,                             // p
    
    // Undo
    Undo,                              // u
    
    // Mode switches
    SwitchToInsert,                    // i in normal mode
    AppendAfterCursor,                 // a
    AppendAtEnd,                       // A
    InsertAtLineStart,                 // I
    OpenLineBelow,                     // o
    EnterVisualLine,                   // V
    EnterCommandMode,                  // :
    EnterSearch,                       // /
    
    // Command execution
    ExecuteCommand(String),            // :q, :w, etc.
    
    // UI
    Submit,                            // Enter in insert
    Quit,                              // :q
    
    // No-op & passthrough
    Noop,
    Passthrough(KeyEvent),
}
```

### Operators & Motions

**Operators** (pending_op): d (delete), c (change), y (yank)
**Motions**: w (word), b (back), e (end), h/j/k/l (arrows), f/F (find char), 0/$/^

**Text Objects** (via pending_text_obj):
- Inner: `i` + target (w=word, "=quotes, (=parens, {=braces, etc.)
- Around: `a` + target (same targets, includes delimiters)

**Count Prefix** (count_buf):
- `3dd` → delete 3 lines
- `2w` → move 2 words forward
- `5j` → move 5 lines down

### Integration with App

- `vim.handle_key(key, text_len)` called from keyboard dispatch
- Results in `VimAction` → mapped to text buffer operations (insert, delete, move cursor)
- Visual mode anchor tracked in `app.vim.visual_anchor` for selection rendering
- Yank register persists across operations

(source: crates/oxicode-tui/src/vim_mode.rs:1-200, vim_text_objects.rs)

---

## Widgets

All widget files found in `crates/oxicode-tui/src/widgets/`:

### Core Widgets

| Widget | Lines | Purpose |
|--------|-------|---------|
| **input_box.rs** | ~15.7k | User input line, cursor, vim mode badge, ghost completion suffix |
| **message_view.rs** | ~54.9k | Main conversation display, markdown rendering, syntax highlighting, tool calls |
| **markdown_view.rs** | ~37.6k | Markdown parser & renderer, code blocks, links, images, tables |
| **status_bar.rs** | ~14.4k | Bottom status line: model, mode, usage, elapsed time |
| **notification.rs** | ~5.2k | Toast notifications: dismiss timers, layered rendering |

### Overlay/Modal Widgets

| Widget | Lines | Purpose |
|--------|-------|---------|
| **permission_dialog.rs** | ~23.3k | Permission request with risk level, countdown timer, options |
| **search_overlay.rs** | ~8.1k | Search mode, query input, results highlighting |
| **command_autocomplete.rs** | ~14.3k | `/...` command dropdown, filtering, selection |
| **history_search.rs** | ~9.2k | Reverse history search (Ctrl+R), pattern matching |
| **model_picker.rs** | ~18.5k | Model selection modal, filtering, quick-switch |
| **session_browser.rs** | ~22.3k | Session list, preview, load/delete, creation date display |
| **shortcuts_overlay.rs** | ~17.5k | Help panel: keyboard shortcuts + slash command reference |
| **paste_preview.rs** | ~4.5k | Large paste confirmation modal |
| **pager.rs** | ~4.5k | Scrollable panel for help/shortcuts (PgUp/PgDn) |

### Tool & Content Widgets

| Widget | Lines | Purpose |
|--------|-------|---------|
| **tool_display.rs** | ~23.1k | Tool call rendering: input, output, stderr, streaming |
| **diff_view.rs** | ~6.6k | Unified diff display with syntax highlighting |
| **tool_call.rs** | ~6.6k | Tool call card: status, input summary, result |
| **code_block.rs** | ~4.0k | Syntax-highlighted code in markdown, copy button |
| **highlight.rs** | ~9.3k | Syntax highlighting engine (tree-sitter or fallback) |

### Layout & Utility Widgets

| Widget | Lines | Purpose |
|--------|-------|---------|
| **split_pane.rs** | ~4.2k | Left/right layout toggle, ratio management |
| **suggestion_chips.rs** | ~3.4k | Inline prompt suggestions display |
| **modal_helpers.rs** | ~9.1k | Shared modal UI (centered frame, borders, buttons) |
| **agent_panel.rs** | ~4.0k | Right panel: active agents status |
| **task_panel.rs** | ~3.8k | Right panel: background tasks |

**Total TUI code:** ~752 lines of source files across 26 widget modules

### Widget Responsibilities

**InputBox**: Renders input field with vim mode badge (N/I/V/VL/C), ghost text suffix, cursor position. Calls `App::handle_key` on input.

**MessageView**: Renders full conversation. Manages scroll state, markdown rendering cache, tool call display, image links, selection highlighting.

**PermissionDialog**: Overlays permission request with risk-level border color (red=dangerous, yellow=warning, etc.), countdown timer, Allow/Deny buttons, tool input summary.

**StatusBar**: Bottom line showing current model, vim/editor mode, usage stats (tokens + cost), elapsed time, permission mode, stall indicator (⏸️ if no delta for 3s).

**ToolDisplay**: Renders tool call in-message with input JSON, streaming output, stderr, final result. Supports collapsible sections.

**SearchOverlay**: Modal for Ctrl+F search with pattern match highlighting in message view.

**Autocomplete**: Dropdown showing slash commands matching `/` prefix, filterable, selectable with arrows.

(source: crates/oxicode-tui/src/widgets/ [26 files])

---

## Rendering Pipeline

Rendering happens per-frame in response to terminal events or core messages:

```
┌──────────────────────────────────────────────────┐
│ draw(terminal) → called after event handling     │
├──────────────────────────────────────────────────┤
│ Layout Computation                               │
│   ├─ terminal.size() → (width, height)           │
│   ├─ split_pane ratio → left/right splits       │
│   └─ Regions: input bar, message area, status    │
├──────────────────────────────────────────────────┤
│ Render Dispatch                                  │
│   ├─ terminal.draw(|f: Frame| { ... })           │
│   ├─   MessageView::render() in message area     │
│   ├─   StatusBar::render() at bottom             │
│   ├─   InputBox::render() at input line          │
│   ├─   SplitPane (toggle right panel)            │
│   ├─   Overlay Priority (topmost):              │
│   │     1. PermissionDialog (if pending)         │
│   │     2. PastePreview (if pending_paste)       │
│   │     3. HistorySearch (if searching)          │
│   │     4. SessionBrowser (if open)              │
│   │     5. ModelPicker (if open)                 │
│   │     6. Autocomplete (if "/" matched)         │
│   │     7. SearchOverlay (if Ctrl+F active)      │
│   │     8. Shortcuts pager (if open)             │
│   │     9. Notifications (toast layer)           │
│   └─ terminal.flush() → write buffer to stdout   │
└──────────────────────────────────────────────────┘
```

**Theme System:**
- Themes loaded from `themes/` directory or user config
- `ratatui::style::Style` applied to each widget
- Colors: Text, HighLight, Primary, Secondary, Danger, Warning
- Theme can be toggled at runtime

**Optimization:**
- **MessageRenderCache**: Caches parsed markdown for unchanged messages (keyed by message ID)
- **streaming_committed_lines**: Pre-rendered lines from streaming collector, avoid re-parsing each delta
- **frame_count**: Used for spinner animation (4-frame rotation)

**Text Selection & Copy:**
- Mouse drag selects text in message view
- Shift+arrow keys select in input box
- Visual/VisualLine mode in vim mode for selection

(source: crates/oxicode-tui/src/render.rs, app.rs render dispatch)

---

## Special Features

### Ghost Completion (Ghost Text)

Displays a dimmed, non-selectable completion suffix after the cursor:

```
User types: "impl"
Ghost text shows: "impl Display for MyType"
                   ─────────────────────
                   (rendered in dim/gray)
```

**Implementation:**
- `prompt_suggestions.rs` analyzes input + context (messages, file, line)
- LLM-assisted or pattern-based suggestions
- Displayed in InputBox using `Style::new().dim()`
- User can press `Tab` to accept (replace input with suggestion + continue typing)

### Image Paste

Users can paste images (from clipboard) that are attached to the next message:

**Flow:**
1. User pastes image via Shift+Insert or `cmd+v` (macOS)
2. `image_paste.rs` detects image in clipboard (MIME type check)
3. If size > `PASTE_PREVIEW_THRESHOLD` (5 MB), show confirmation modal
4. On confirm: image cached in `pending_images`, file path stored in `sent_image_paths`
5. Next message includes image data + `[Image #N]` reference in text
6. TUI renders OSC 8 hyperlinks for click-to-open

### Paste Detection & Preview

Large text pastes trigger a preview modal:

**Flow:**
1. User pastes text (Shift+Insert or bracket paste)
2. If text > threshold (e.g., 50 lines), show `PastePreview` modal
3. Modal displays first 10 lines + "..." + byte count
4. User confirms with `y` (include all) or `n` (discard)
5. On confirm: text sent to `UserInput` event

### Prompt Suggestions

Context-aware follow-up suggestions shown as chips below input:

```
You: "How do I sort a Vec in Rust?"
Assistant: "Use .sort() or .sort_by()..."

Suggestion chips:
 [Sort custom types?] [Performance tips?] [Alternatives?]
```

**Context:** Current file, recent messages, cursor position in input

### Streaming Markdown

Markdown is streamed line-by-line with newline boundaries:

- **MarkdownStreamCollector**: Collects text deltas until newline, then parses as markdown block
- **streaming_committed_lines**: Pre-rendered lines appended to message view
- **streaming_markdown.rs**: Incremental markdown parser (tables, code blocks, emphasis)

### Scroll & Auto-Scroll

- **auto_scroll**: When true, pinned to bottom (new content appends at bottom)
- **scroll_offset**: Absolute line offset from top of message view
- **max_scroll_offset**: Computed per-frame based on content height vs viewport
- **ScrollUp/ScrollDown**: Move offset ±1 line
- **PageUp/PageDown**: Move offset ±(viewport height)

When streaming, auto_scroll = true unless user manually scrolled up (scroll_offset < max).

### Tool Call Tracking

**ActiveToolCall** struct tracks in-flight tool execution:

```rust
struct ActiveToolCall {
    id: String,
    name: String,
    input_summary: String,
    raw_input: serde_json::Value,
    started_at: Instant,
    result: Option<(String, bool)>,  // (output, is_error)
}
```

On `ToolUseStart`: Add to `active_tools`, start timer
On `ToolResult`: Set `result` field, display in tool call widget
On `MessageComplete`: Remove all from `active_tools`

### Split Pane

Left/right layout toggle:
- **SplitPane**: Stores left/right ratio (default 70/30)
- **Toggle**: Tab key cycles: Single (left full), Right panel (70/30), Left panel (30/70)
- Right panel shows: Agent status, background tasks, context usage

### Permission Dialog Countdown

Permission requests have optional timeout:

```
┌──────────────────────────────┐
│ Execute: bash (exit_code)    │
│ $ rm -rf /                   │
│                              │
│ [Allow]  [Deny]  (10s auto-) │  ← countdown
└──────────────────────────────┘
```

Auto-deny on timeout (configurable).

(source: crates/oxicode-tui/src/ghost_completion.rs, image_paste.rs, paste_detector.rs, prompt_suggestions.rs, streaming_markdown.rs, widgets/)

---

## Subsystem Integration Points

| Subsystem | Integration |
|-----------|-------------|
| **oxicode-core** | Sends CoreEvent via channel; receives UiEvent |
| **oxicode-state** | Subscribes to AppState via watch channel |
| **oxicode-config** | Loads keybindings, theme from user config |
| **oxicode-session** | Persistence of prompt history (PersistentHistory) |
| **oxicode-permissions** | Displays PermissionAsk dialog, sends PermissionResponse |
| **oxicode-common** | Message, Role, ContentBlock, PermissionResponse types |

---

**Lines of Code:**
- Main app.rs: ~213.4k
- Widgets: ~752 lines combined
- Supporting modules (vim, keybindings, etc.): ~80k
- **Total TUI crate: ~380k LOC**

**Performance Targets:**
- Frames: 20 FPS max (50ms tick during streaming, 100ms idle)
- Message cache: O(n) keys, LRU eviction for unchanged messages
- Terminal event polling: Non-blocking crossterm, 32-event buffer
