use oxicode_common::PermissionResponse;
use tokio::sync::oneshot;

/// Events emitted by QueryEngine during `execute_turn()`.
///
/// Sent via an optional `mpsc::Sender<TurnEvent>` so the TUI (or other
/// consumers) can render streaming progress, tool calls, and permission
/// dialogs in real-time.  When the sender is `None` (single-prompt mode),
/// no events are emitted and behavior is unchanged.
pub enum TurnEvent {
    /// Streaming text delta from the LLM.
    TextDelta(String),
    /// Streaming thinking/reasoning delta from extended thinking models.
    ThinkingDelta(String),
    /// A new LLM turn has started streaming.
    TurnStart,
    /// The current LLM turn finished streaming.
    TurnEnd,
    /// A tool use block was finalized (tool call about to execute).
    ToolUseStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool execution completed.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// Permission required — caller must send response via `reply_tx`.
    /// Engine blocks on the oneshot receiver until a response arrives.
    PermissionAsk {
        tool_name: String,
        input_summary: String,
        prompt: String,
        reply_tx: oneshot::Sender<PermissionResponse>,
    },
    /// Error during the turn.
    Error(String),
    /// Non-rate-limit retry in progress (e.g., 502, connection error).
    Retrying {
        message: String,
        attempt: u32,
        max_retries: u32,
        retry_in_secs: f64,
    },
    /// Rate limited — provider returned 429, retry in progress.
    RateLimited {
        message: String,
        attempt: u32,
        max_retries: u32,
        retry_in_secs: f64,
    },
}

/// Send a `TurnEvent` if a sender is present. Ignores send failures
/// (e.g. receiver dropped).
pub async fn emit(tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>, event: TurnEvent) {
    if let Some(tx) = tx {
        let _ = tx.send(event).await;
    }
}
