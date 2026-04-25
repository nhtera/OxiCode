use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use oxicode_api::{LlmProvider, MessageRequest, StreamEvent};
use oxicode_common::{ContentBlock, Message, OxiError, OxiResult, Role, StopReason};
use oxicode_context::BudgetManager;
use oxicode_hooks::{HookEvent, HookManager, HookResponse};
use oxicode_permissions::PermissionPipeline;
use oxicode_state::StateStore;
use oxicode_tools::{ToolContext, ToolRegistry};
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex;

use crate::conversation::Conversation;
use crate::turn_event::{emit, fire_hook_with_events, TurnEvent};

/// Maximum number of tool-use turns before forcing a stop.
const MAX_TOOL_TURNS: usize = 50;

/// Typical Claude model context window (used as default budget ceiling).
const DEFAULT_MODEL_MAX_TOKENS: usize = 200_000;

/// Maximum retries when a turn ends in `StopReason::MaxTokens`.
///
/// After this many retries, the truncated assistant message is returned with
/// stop_reason still `MaxTokens` so the caller can decide what to do.
pub const MAX_OUTPUT_TOKENS_RECOVERY: u8 = 3;

/// Synthetic user nudge appended to the conversation when continuing past a
/// `MaxTokens` cutoff. Sent verbatim — the model treats it as a directive.
pub const CONTINUATION_NUDGE: &str = "Continue from where you left off";

/// Multi-turn query engine with tool execution support.
pub struct QueryEngine {
    provider: Arc<dyn LlmProvider>,
    pub(crate) state_store: Arc<StateStore>,
    pub(crate) tool_registry: Arc<ToolRegistry>,
    pub(crate) permission_pipeline: Arc<PermissionPipeline>,
    pub(crate) tool_context: ToolContext,
    model: StdMutex<String>,
    max_tokens: u32,
    system_prompt: String,
    /// Context budget manager — wrapped in Mutex because check_budget needs &mut self.
    budget_manager: Mutex<BudgetManager>,
    /// Hook manager for firing lifecycle events.
    pub(crate) hook_manager: Arc<HookManager>,
}

impl QueryEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        state_store: Arc<StateStore>,
        tool_registry: Arc<ToolRegistry>,
        permission_pipeline: Arc<PermissionPipeline>,
        tool_context: ToolContext,
        model: String,
        max_tokens: u32,
        system_prompt: String,
    ) -> Self {
        // Use hook_manager from tool_context if available, otherwise noop.
        let hook_manager = tool_context
            .hook_manager
            .clone()
            .unwrap_or_else(|| Arc::new(HookManager::noop()));
        Self {
            provider,
            state_store,
            tool_registry,
            permission_pipeline,
            tool_context,
            model: StdMutex::new(model),
            max_tokens,
            system_prompt,
            budget_manager: Mutex::new(BudgetManager::new(DEFAULT_MODEL_MAX_TOKENS)),
            hook_manager,
        }
    }

    /// Get a reference to the LLM provider.
    pub fn provider_ref(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Get the system prompt.
    pub fn system_prompt_ref(&self) -> &str {
        &self.system_prompt
    }

    /// Get the current model name.
    pub fn model(&self) -> String {
        self.model.lock().unwrap().clone()
    }

    /// Switch the active model at runtime.
    pub fn set_model(&self, new_model: String) {
        *self.model.lock().unwrap() = new_model;
    }

    /// Get max tokens setting.
    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    /// Get a reference to the tool registry — used by slash command async
    /// dispatch in oxicode-cli to invoke built-in tools (cron, worktree, etc.).
    pub fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }

    /// Get a reference to the tool execution context.
    pub fn tool_context(&self) -> &ToolContext {
        &self.tool_context
    }

    /// Execute a multi-turn conversation loop.
    ///
    /// Sends messages to the LLM, executes any tool calls, appends results,
    /// and continues until the LLM returns EndTurn or MaxTokens.
    /// Execute a multi-turn conversation loop.
    ///
    /// When `event_tx` is `Some`, emits `TurnEvent`s so the TUI can render
    /// streaming text, tool calls, and permission dialogs in real-time.
    /// When `None` (single-prompt mode), behavior is unchanged.
    pub async fn execute_turn(
        &self,
        conversation: &mut Conversation,
        event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
    ) -> OxiResult<Message> {
        self.execute_turn_with_cancel(conversation, event_tx, None)
            .await
    }

    /// Execute a multi-turn conversation loop with optional cancel support.
    ///
    /// When `cancel_flag` is `Some`, the engine checks the flag between stream
    /// events and before each tool execution. When set, the turn is aborted
    /// and an `Interrupted` error is returned.
    #[allow(clippy::too_many_lines)]
    pub async fn execute_turn_with_cancel(
        &self,
        conversation: &mut Conversation,
        event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
        cancel_flag: Option<&Arc<AtomicBool>>,
    ) -> OxiResult<Message> {
        let mut turn_count = 0;

        loop {
            turn_count += 1;
            if turn_count > MAX_TOOL_TURNS {
                tracing::warn!("Max tool turns ({MAX_TOOL_TURNS}) reached, stopping");
                break;
            }

            // Check cancel flag before each turn.
            if let Some(flag) = cancel_flag {
                if flag.load(Ordering::SeqCst) {
                    flag.store(false, Ordering::SeqCst);
                    self.state_store.set_streaming(false);
                    emit(event_tx, TurnEvent::TurnEnd).await;
                    return Err(OxiError::Other("Interrupted by user".into()));
                }
            }

            // Context defense: apply budget management before API call.
            {
                let mut mgr = self.budget_manager.lock().await;
                let defended = mgr
                    .apply_defense_with_dir(
                        conversation.api_messages(),
                        self.provider.as_ref(),
                        &self.model(),
                        &self.tool_context.working_dir,
                    )
                    .await?;
                if defended.len() < conversation.len() {
                    tracing::info!(
                        before = conversation.len(),
                        after = defended.len(),
                        "context compacted before API call"
                    );
                    conversation.replace_messages(defended);
                }
            }

            // Fire UserPromptSubmit ONCE on the first turn (before the first
            // LLM call) — Claude Code spec: this hook fires when the user
            // submits, not per intermediate tool turn.
            if turn_count == 1 {
                let prompt_text = conversation.last_user_text().unwrap_or_default();
                let pre_data = serde_json::json!({
                    "prompt": prompt_text,
                });
                let pre_resp = fire_hook_with_events(
                    &self.hook_manager,
                    HookEvent::UserPromptSubmit,
                    pre_data,
                    event_tx,
                )
                .await;
                if let HookResponse::Abort { reason } = pre_resp {
                    return Err(OxiError::HookAbort(reason));
                }
            }

            let assistant_msg = self
                .stream_one_turn(conversation, event_tx, cancel_flag)
                .await;

            let mut assistant_msg = match assistant_msg {
                Ok(msg) => msg,
                Err(e) => {
                    // Errors flow through stderr / OxiResult; no Error hook
                    // event in Claude Code spec.
                    return Err(e);
                }
            };

            // Max-output-tokens recovery loop. When the API returns MaxTokens,
            // append a synthetic nudge and re-stream up to N times so long
            // code-gen turns don't silently truncate.
            self.max_tokens_recovery_loop(
                &mut assistant_msg,
                conversation,
                event_tx,
                cancel_flag,
            )
            .await?;

            // Fire Stop hook ONLY on final EndTurn (Claude Code spec —
            // not per intermediate tool turn). Runs AFTER recovery so the
            // hook sees the merged assistant message.
            let stop = assistant_msg.stop_reason.unwrap_or(StopReason::EndTurn);
            if matches!(stop, StopReason::EndTurn) {
                let post_data = serde_json::json!({
                    "stop_reason": format!("{stop:?}").to_lowercase(),
                });
                fire_hook_with_events(
                    &self.hook_manager,
                    HookEvent::Stop,
                    post_data,
                    event_tx,
                )
                .await;
            }
            let stop_reason = assistant_msg.stop_reason.unwrap_or(StopReason::EndTurn);

            // Extract tool use blocks from the assistant message.
            let tool_uses: Vec<_> = assistant_msg
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse { id, name, input } => {
                        Some((id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect();

            if tool_uses.is_empty() || stop_reason == StopReason::EndTurn {
                return Ok(assistant_msg);
            }

            // Execute each tool and build a user message with tool results.
            let mut tool_results = Vec::new();
            let mut interrupted = false;
            for (id, name, input) in &tool_uses {
                // Check cancel flag before each tool execution.
                if let Some(flag) = cancel_flag {
                    if flag.load(Ordering::SeqCst) {
                        flag.store(false, Ordering::SeqCst);
                        // Repair: the assistant message with ToolUse blocks is already in
                        // conversation; add synthetic results for all pending tools so the
                        // next API call is not rejected for missing tool_results.
                        self.commit_interrupt_results(
                            conversation,
                            &tool_uses,
                            std::mem::take(&mut tool_results),
                        );
                        self.state_store.set_streaming(false);
                        emit(event_tx, TurnEvent::TurnEnd).await;
                        return Err(OxiError::Other("Interrupted by user".into()));
                    }
                }

                // Race tool execution against cancel flag so Ctrl+C interrupts
                // long-running tools (e.g. bash). The bash child process has
                // kill_on_drop(true), so dropping the future kills the process.
                let result = if let Some(flag) = cancel_flag {
                    let tool_fut = self.execute_tool(id, name, input, event_tx);
                    tokio::pin!(tool_fut);

                    loop {
                        tokio::select! {
                            result = &mut tool_fut => break result,
                            () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                                if flag.load(Ordering::SeqCst) {
                                    flag.store(false, Ordering::SeqCst);
                                    // Drop tool_fut here kills the bash child process.
                                    interrupted = true;
                                    break ContentBlock::ToolResult {
                                        tool_use_id: id.clone(),
                                        content: "Interrupted by user".to_string(),
                                        is_error: true,
                                    };
                                }
                            }
                        }
                    }
                } else {
                    self.execute_tool(id, name, input, event_tx).await
                };

                tool_results.push(result);
                if interrupted {
                    // Emit interrupted tool result event and abort.
                    emit(
                        event_tx,
                        TurnEvent::ToolResult {
                            tool_use_id: id.clone(),
                            content: "Interrupted by user".to_string(),
                            is_error: true,
                        },
                    )
                    .await;
                    // Repair: add synthetic results for any remaining tools so the
                    // conversation invariant (tool_calls → tool_results) is maintained.
                    self.commit_interrupt_results(
                        conversation,
                        &tool_uses,
                        std::mem::take(&mut tool_results),
                    );
                    self.state_store.set_streaming(false);
                    emit(event_tx, TurnEvent::TurnEnd).await;
                    return Err(OxiError::Other("Interrupted by user".into()));
                }
            }

            // Add tool result message to conversation.
            let result_msg = Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: Role::User,
                content: tool_results,
                model: None,
                stop_reason: None,
                created_at: chrono::Utc::now(),
                usage: None,
            };

            self.state_store.push_message(result_msg.clone());
            conversation.push(result_msg);

            if stop_reason != StopReason::ToolUse {
                return Ok(assistant_msg);
            }
        }

        Err(OxiError::Other("Max tool turns exceeded".to_string()))
    }

    /// Recover from `StopReason::MaxTokens` by appending a continuation nudge
    /// and re-streaming up to `MAX_OUTPUT_TOKENS_RECOVERY` times.
    ///
    /// The truncated assistant message is already at the conversation tail
    /// (pushed by `stream_one_turn`). For each retry, this method:
    ///   1. Pushes a synthetic `Continue from where you left off` user message.
    ///   2. Calls `stream_one_turn` again — the new assistant turn auto-appends.
    ///   3. Strips the new assistant + nudge + truncated assistant from
    ///      conversation/state.
    ///   4. Merges the new assistant into `assistant_msg`.
    ///   5. Re-pushes the merged assistant so persisted history is clean.
    ///
    /// Cancellation short-circuits the loop — never retries a user-cancelled
    /// turn. On exit, `assistant_msg.stop_reason` is the last iteration's stop
    /// reason (`EndTurn` if recovery succeeded, `MaxTokens` if all attempts
    /// also truncated).
    async fn max_tokens_recovery_loop(
        &self,
        assistant_msg: &mut Message,
        conversation: &mut Conversation,
        event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
        cancel_flag: Option<&Arc<AtomicBool>>,
    ) -> OxiResult<()> {
        let mut attempt: u8 = 0;
        while assistant_msg.stop_reason == Some(StopReason::MaxTokens)
            && attempt < MAX_OUTPUT_TOKENS_RECOVERY
        {
            if let Some(flag) = cancel_flag {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
            }
            attempt += 1;

            emit(
                event_tx,
                TurnEvent::Retrying {
                    message: format!(
                        "Output truncated, continuing ({attempt}/{MAX_OUTPUT_TOKENS_RECOVERY})"
                    ),
                    attempt: u32::from(attempt),
                    max_retries: u32::from(MAX_OUTPUT_TOKENS_RECOVERY),
                    retry_in_secs: 0.0,
                },
            )
            .await;

            // Push synthetic user nudge so the next API call sees
            // [..., assistant(truncated), user(nudge)].
            let nudge = Message::user(CONTINUATION_NUDGE);
            self.state_store.push_message(nudge.clone());
            conversation.push(nudge);

            let new_assistant = self
                .stream_one_turn(conversation, event_tx, cancel_flag)
                .await?;

            // Strip the three transient messages we added during this attempt
            // (new_assistant, nudge, original truncated assistant) so the
            // re-pushed merged message replaces them in-place.
            conversation.messages.pop();
            conversation.messages.pop();
            conversation.messages.pop();
            self.state_store.pop_message();
            self.state_store.pop_message();
            self.state_store.pop_message();

            Self::merge_assistant_messages(assistant_msg, new_assistant);

            self.state_store.push_message(assistant_msg.clone());
            conversation.push(assistant_msg.clone());
        }
        Ok(())
    }

    /// Merge a continuation assistant message into the base.
    ///
    /// Concatenates trailing/leading text blocks (so the user sees one
    /// continuous response), appends remaining blocks verbatim, accumulates
    /// usage, and replaces stop_reason with the latest iteration's value.
    fn merge_assistant_messages(base: &mut Message, next: Message) {
        let mut next_iter = next.content.into_iter();

        // If both base ends with Text and next starts with Text, splice.
        if matches!(base.content.last(), Some(ContentBlock::Text { .. })) {
            if let Some(first) = next_iter.next() {
                match first {
                    ContentBlock::Text { text: next_text } => {
                        if let Some(ContentBlock::Text { text }) = base.content.last_mut() {
                            text.push_str(&next_text);
                        }
                    }
                    other => base.content.push(other),
                }
            }
        }
        for block in next_iter {
            base.content.push(block);
        }

        // Accumulate token usage so /cost stays accurate across retries.
        if let Some(next_usage) = next.usage {
            let base_usage = base.usage.get_or_insert_with(oxicode_common::Usage::default);
            base_usage.input_tokens = base_usage.input_tokens.saturating_add(next_usage.input_tokens);
            base_usage.output_tokens =
                base_usage.output_tokens.saturating_add(next_usage.output_tokens);
            base_usage.cache_read_input_tokens = sum_opt(
                base_usage.cache_read_input_tokens,
                next_usage.cache_read_input_tokens,
            );
            base_usage.cache_creation_input_tokens = sum_opt(
                base_usage.cache_creation_input_tokens,
                next_usage.cache_creation_input_tokens,
            );
        }

        base.stop_reason = next.stop_reason;
    }

    /// Repair the conversation after an interrupt that stopped tool execution.
    ///
    /// The assistant message with ToolUse blocks is already in conversation; without
    /// corresponding ToolResult blocks the next API call fails with a "tool_calls must
    /// be followed by tool_results" error. This method adds a user message with
    /// synthetic error results for any tools that were not yet executed.
    fn commit_interrupt_results(
        &self,
        conversation: &mut Conversation,
        tool_uses: &[(String, String, serde_json::Value)],
        partial: Vec<ContentBlock>,
    ) {
        let done: std::collections::HashSet<String> = partial
            .iter()
            .filter_map(|b| {
                if let ContentBlock::ToolResult { tool_use_id, .. } = b {
                    Some(tool_use_id.clone())
                } else {
                    None
                }
            })
            .collect();

        let mut results = partial;
        for (id, _, _) in tool_uses {
            if !done.contains(id) {
                results.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: "Interrupted by user".to_string(),
                    is_error: true,
                });
            }
        }

        if results.is_empty() {
            return;
        }

        let result_msg = Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: Role::User,
            content: results,
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        };
        self.state_store.push_message(result_msg.clone());
        conversation.push(result_msg);
    }

    /// Stream a single LLM turn, collecting the full assistant message.
    /// Cleanup helper: reset streaming state and emit error + end events.
    async fn abort_streaming(
        &self,
        event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
        error: &OxiError,
    ) {
        self.state_store.set_streaming(false);
        emit(event_tx, TurnEvent::Error(error.to_string())).await;
        emit(event_tx, TurnEvent::TurnEnd).await;
    }

    /// Merge any consecutive user-role messages into a single message.
    ///
    /// The Anthropic API requires strictly alternating user/assistant turns. Consecutive
    /// user messages can appear after an interrupted tool turn if the user sends a
    /// follow-up before the engine processes the interrupt (e.g. stall auto-recovery
    /// sets is_turn_active=false while the engine is still running). Merging here is a
    /// defensive layer that prevents spurious "roles must alternate" API rejections.
    fn coalesce_messages(messages: &[Message]) -> Vec<Message> {
        let mut out: Vec<Message> = Vec::with_capacity(messages.len());
        for msg in messages {
            if msg.role == Role::User {
                if let Some(last) = out.last_mut() {
                    if last.role == Role::User {
                        last.content.extend(msg.content.clone());
                        continue;
                    }
                }
            }
            out.push(msg.clone());
        }
        out
    }

    /// Build a MessageRequest with all tool schemas (built-in + MCP).
    ///
    /// Dynamically injects mode-specific system prompt sections (advisor, sandbox)
    /// and environment info based on current state.
    fn build_request(&self, conversation: &Conversation) -> MessageRequest {
        let model = self.model();
        let mut tool_schemas = self.tool_registry.schemas_json();
        for (server_name, server_tools) in self.mcp_tool_schemas() {
            tool_schemas.extend(oxicode_mcp::mcp_tools_to_schemas(
                &server_name,
                &server_tools,
            ));
        }

        // Append mode injection (advisor/sandbox) and env info to system prompt.
        let active_skills = self.state_store.current().active_skills;
        let mut effective_prompt = self.system_prompt.clone();

        // Inject environment info (working dir, platform, shell, date, model).
        let cwd_str = self.tool_context.working_dir.to_string_lossy();
        let env_info = crate::system_prompt::build_env_info_section(Some(&cwd_str), Some(&model));
        effective_prompt.push_str(&env_info);

        if let Some(modes) = crate::system_prompt::mode_injection_text(&active_skills) {
            effective_prompt.push_str(&modes);
        }

        let messages = Self::coalesce_messages(conversation.api_messages());
        let mut request = MessageRequest::new(&model, messages)
            .with_system(&effective_prompt)
            .with_max_tokens(self.max_tokens);
        request.tools = tool_schemas;
        request
    }

    #[allow(clippy::too_many_lines)] // core streaming loop with necessary event handling
    async fn stream_one_turn(
        &self,
        conversation: &mut Conversation,
        event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
        cancel_flag: Option<&Arc<AtomicBool>>,
    ) -> OxiResult<Message> {
        let request = self.build_request(conversation);

        self.state_store.set_streaming(true);
        emit(event_tx, TurnEvent::TurnStart).await;

        let mut stream = match self.provider.stream_message(request).await {
            Ok(s) => s,
            Err(e) => {
                self.abort_streaming(event_tx, &e).await;
                return Err(e);
            }
        };
        let mut assistant_msg = Message::assistant();
        assistant_msg.model = Some(self.model());

        let mut current_text = String::new();
        let mut current_thinking = String::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_input_json = String::new();

        loop {
            // Eager cancel check before blocking on stream. During rapid
            // SSE events, select! may always pick the stream branch, so
            // this ensures the flag is checked at least once per iteration.
            if let Some(flag) = cancel_flag {
                if flag.load(Ordering::SeqCst) {
                    flag.store(false, Ordering::SeqCst);
                    if !current_text.is_empty() {
                        assistant_msg.content.push(ContentBlock::Text {
                            text: std::mem::take(&mut current_text),
                        });
                    }
                    assistant_msg.stop_reason = Some(StopReason::EndTurn);
                    self.state_store.set_streaming(false);
                    self.state_store.push_message(assistant_msg.clone());
                    conversation.push(assistant_msg.clone());
                    emit(event_tx, TurnEvent::TurnEnd).await;
                    return Err(OxiError::Other("Interrupted by user".into()));
                }
            }

            // Race stream.next() against a 50ms cancel-flag poll so Esc
            // interrupts even when the server is slow to send events.
            let event_result = if let Some(flag) = cancel_flag {
                loop {
                    tokio::select! {
                        item = stream.next() => break item,
                        () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                            if flag.load(Ordering::SeqCst) {
                                flag.store(false, Ordering::SeqCst);
                                if !current_text.is_empty() {
                                    assistant_msg.content.push(ContentBlock::Text {
                                        text: std::mem::take(&mut current_text),
                                    });
                                }
                                assistant_msg.stop_reason = Some(StopReason::EndTurn);
                                self.state_store.set_streaming(false);
                                self.state_store.push_message(assistant_msg.clone());
                                conversation.push(assistant_msg.clone());
                                emit(event_tx, TurnEvent::TurnEnd).await;
                                return Err(OxiError::Other("Interrupted by user".into()));
                            }
                        }
                    }
                }
            } else {
                stream.next().await
            };

            let Some(event_result) = event_result else {
                break;
            };

            let event = match event_result {
                Ok(ev) => ev,
                Err(e) => {
                    self.abort_streaming(event_tx, &e).await;
                    return Err(e);
                }
            };

            match event {
                StreamEvent::TextDelta { text } => {
                    current_text.push_str(&text);
                    emit(event_tx, TurnEvent::TextDelta(text)).await;
                }
                StreamEvent::ThinkingDelta { thinking } => {
                    current_thinking.push_str(&thinking);
                    emit(event_tx, TurnEvent::ThinkingDelta(thinking)).await;
                }
                StreamEvent::ToolUseStart { id, name } => {
                    // Finalize any pending text block.
                    if !current_text.is_empty() {
                        assistant_msg.content.push(ContentBlock::Text {
                            text: std::mem::take(&mut current_text),
                        });
                    }
                    current_tool_id = id;
                    current_tool_name = name;
                    current_tool_input_json.clear();
                }
                StreamEvent::ToolInputDelta { partial_json } => {
                    current_tool_input_json.push_str(&partial_json);
                }
                StreamEvent::ContentBlockStop { .. } => {
                    // If we were accumulating a tool use, finalize it.
                    if !current_tool_id.is_empty() {
                        let input: serde_json::Value =
                            serde_json::from_str(&current_tool_input_json)
                                .unwrap_or(serde_json::Value::Object(serde_json::Map::default()));

                        let id = std::mem::take(&mut current_tool_id);
                        let name = std::mem::take(&mut current_tool_name);

                        emit(
                            event_tx,
                            TurnEvent::ToolUseStart {
                                id: id.clone(),
                                name: name.clone(),
                                input: input.clone(),
                            },
                        )
                        .await;

                        assistant_msg
                            .content
                            .push(ContentBlock::ToolUse { id, name, input });
                        current_tool_input_json.clear();
                    }
                }
                StreamEvent::UsageUpdate(usage) => {
                    self.state_store.add_usage(&usage, &self.model());
                    assistant_msg.usage = Some(usage);
                }
                StreamEvent::MessageStop { stop_reason } => {
                    // Finalize pending thinking block (before text so it appears first).
                    if !current_thinking.is_empty() {
                        assistant_msg.content.push(ContentBlock::Thinking {
                            thinking: std::mem::take(&mut current_thinking),
                        });
                    }
                    // Finalize pending text.
                    if !current_text.is_empty() {
                        assistant_msg.content.push(ContentBlock::Text {
                            text: std::mem::take(&mut current_text),
                        });
                    }
                    assistant_msg.stop_reason = Some(stop_reason);
                    emit(event_tx, TurnEvent::TurnEnd).await;

                    if stop_reason == StopReason::MaxTokens {
                        tracing::warn!("Response truncated — max tokens reached");
                    }
                    break;
                }
                StreamEvent::Error { message } => {
                    let err = OxiError::api(&message);
                    self.abort_streaming(event_tx, &err).await;
                    return Err(err);
                }
                StreamEvent::Retrying {
                    message,
                    attempt,
                    max_retries,
                    retry_in_secs,
                } => {
                    emit(
                        event_tx,
                        TurnEvent::Retrying {
                            message,
                            attempt,
                            max_retries,
                            retry_in_secs,
                        },
                    )
                    .await;
                }
                StreamEvent::Ping => {}
                StreamEvent::CacheBreakDetected(event) => {
                    tracing::warn!(
                        "Cache break detected: {:?} (diff: {} tokens)",
                        event.reason,
                        event.diff
                    );
                }
                StreamEvent::RateLimited {
                    info,
                    attempt,
                    max_retries,
                    retry_in_secs,
                } => {
                    // Record in state for /status display.
                    self.state_store
                        .record_rate_limit(info.clone(), self.provider.name().to_string());
                    emit(
                        event_tx,
                        TurnEvent::RateLimited {
                            message: info.message,
                            attempt,
                            max_retries,
                            retry_in_secs,
                        },
                    )
                    .await;
                }
                StreamEvent::ApiError {
                    status,
                    error_type,
                    message,
                } => {
                    tracing::warn!(
                        "API error: status={} type={:?} message={}",
                        status,
                        error_type,
                        message
                    );
                    // Not emitting TurnEvent::Retrying here — the real Retrying
                    // event follows immediately from the same error path in the
                    // provider, with correct attempt/max_retries values.
                }
            }
        }

        self.state_store.set_streaming(false);

        // Add assistant message to state and conversation.
        self.state_store.push_message(assistant_msg.clone());
        conversation.push(assistant_msg.clone());

        Ok(assistant_msg)
    }
}

/// Sum two `Option<u32>` values, treating `None` as zero. Returns `None`
/// only when both inputs are `None`.
fn sum_opt(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}
