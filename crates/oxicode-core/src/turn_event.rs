use oxicode_common::PermissionResponse;
use tokio::sync::oneshot;

/// Lifecycle phase of a hook fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookState {
    /// Hook is currently executing (transient — TUI shows footer indicator).
    Running,
    /// Hook returned (clears the running indicator).
    Completed,
}

impl HookState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
        }
    }
}

/// Classification of a persistent hook message — drives TUI color choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookMessageKind {
    /// Informational (`systemMessage`) — neutral / dim color.
    System,
    /// Hook decided to block / aborted (`decision=block`, `action=abort`).
    BlockError,
    /// Additional context the hook wants surfaced (rarely shown).
    Context,
}

impl HookMessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::BlockError => "block_error",
            Self::Context => "context",
        }
    }
}

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
    /// A hook fire is starting or finishing (transient progress signal).
    HookProgress { event: String, state: HookState },
    /// A hook returned a user-visible message (persistent transcript line).
    HookMessage {
        event: String,
        kind: HookMessageKind,
        content: String,
    },
}

/// Send a `TurnEvent` if a sender is present. Ignores send failures
/// (e.g. receiver dropped).
pub async fn emit(tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>, event: TurnEvent) {
    if let Some(tx) = tx {
        let _ = tx.send(event).await;
    }
}

/// Fire a hook and emit `HookProgress` (running/completed) + `HookMessage` events
/// around the call. Skips emission when no hook is configured for `event`.
///
/// Centralized so every hook fire site reports identically without each caller
/// having to remember the running/completed/message dance.
pub async fn fire_hook_with_events(
    hook_manager: &oxicode_hooks::HookManager,
    event: oxicode_hooks::HookEvent,
    data: serde_json::Value,
    event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
) -> oxicode_hooks::HookResponse {
    if !hook_manager.has_hook(event) {
        return oxicode_hooks::HookResponse::Pass;
    }

    let event_name = event.as_str().to_string();

    emit(
        event_tx,
        TurnEvent::HookProgress {
            event: event_name.clone(),
            state: HookState::Running,
        },
    )
    .await;

    let resp = hook_manager.fire(event, data).await;

    emit(
        event_tx,
        TurnEvent::HookProgress {
            event: event_name.clone(),
            state: HookState::Completed,
        },
    )
    .await;

    match &resp {
        oxicode_hooks::HookResponse::Abort { reason } => {
            emit(
                event_tx,
                TurnEvent::HookMessage {
                    event: event_name,
                    kind: HookMessageKind::BlockError,
                    content: reason.clone(),
                },
            )
            .await;
        }
        oxicode_hooks::HookResponse::Message {
            system_message,
            block_reason,
            additional_context,
        } => {
            if let Some(msg) = system_message {
                emit(
                    event_tx,
                    TurnEvent::HookMessage {
                        event: event_name.clone(),
                        kind: HookMessageKind::System,
                        content: msg.clone(),
                    },
                )
                .await;
            }
            if let Some(reason) = block_reason {
                emit(
                    event_tx,
                    TurnEvent::HookMessage {
                        event: event_name.clone(),
                        kind: HookMessageKind::BlockError,
                        content: reason.clone(),
                    },
                )
                .await;
            }
            if let Some(ctx) = additional_context {
                emit(
                    event_tx,
                    TurnEvent::HookMessage {
                        event: event_name,
                        kind: HookMessageKind::Context,
                        content: ctx.clone(),
                    },
                )
                .await;
            }
        }
        _ => {}
    }

    resp
}
