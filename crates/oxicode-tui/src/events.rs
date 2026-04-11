/// Events flowing from TUI to the core engine.
#[derive(Debug, Clone)]
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

/// Events flowing from core engine to TUI for rendering.
///
/// Note: Cannot derive Clone because `PermissionAsk` contains a oneshot sender.
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
    /// Permission required — TUI must show dialog and send response via `reply_tx`.
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
