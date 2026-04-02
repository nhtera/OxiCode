use std::sync::Arc;

use futures::StreamExt;
use oxicode_api::{LlmProvider, MessageRequest, StreamEvent};
use oxicode_common::{ContentBlock, Message, OxiError, OxiResult, Role, StopReason};
use oxicode_context::BudgetManager;
use oxicode_permissions::PermissionPipeline;
use oxicode_state::StateStore;
use oxicode_tools::{ToolContext, ToolRegistry};
use tokio::sync::Mutex;

use crate::conversation::Conversation;
use crate::turn_event::{emit, TurnEvent};

/// Maximum number of tool-use turns before forcing a stop.
const MAX_TOOL_TURNS: usize = 50;

/// Typical Claude model context window (used as default budget ceiling).
const DEFAULT_MODEL_MAX_TOKENS: usize = 200_000;

/// Multi-turn query engine with tool execution support.
pub struct QueryEngine {
    provider: Arc<dyn LlmProvider>,
    pub(crate) state_store: Arc<StateStore>,
    pub(crate) tool_registry: Arc<ToolRegistry>,
    pub(crate) permission_pipeline: Arc<PermissionPipeline>,
    pub(crate) tool_context: ToolContext,
    model: String,
    max_tokens: u32,
    system_prompt: String,
    /// Context budget manager — wrapped in Mutex because check_budget needs &mut self.
    budget_manager: Mutex<BudgetManager>,
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
        Self {
            provider,
            state_store,
            tool_registry,
            permission_pipeline,
            tool_context,
            model,
            max_tokens,
            system_prompt,
            budget_manager: Mutex::new(BudgetManager::new(DEFAULT_MODEL_MAX_TOKENS)),
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

    /// Get max tokens setting.
    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
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
        let mut turn_count = 0;

        loop {
            turn_count += 1;
            if turn_count > MAX_TOOL_TURNS {
                tracing::warn!("Max tool turns ({MAX_TOOL_TURNS}) reached, stopping");
                break;
            }

            // Context defense: apply budget management before API call.
            {
                let mut mgr = self.budget_manager.lock().await;
                let defended = mgr
                    .apply_defense_with_dir(
                        conversation.api_messages(),
                        self.provider.as_ref(),
                        &self.model,
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

            let assistant_msg = self.stream_one_turn(conversation, event_tx).await?;
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
            for (id, name, input) in &tool_uses {
                let result = self.execute_tool(id, name, input, event_tx).await;
                tool_results.push(result);
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

    /// Build a MessageRequest with all tool schemas (built-in + MCP).
    fn build_request(&self, conversation: &Conversation) -> MessageRequest {
        let mut tool_schemas = self.tool_registry.schemas_json();
        for (server_name, server_tools) in self.mcp_tool_schemas() {
            tool_schemas.extend(oxicode_mcp::mcp_tools_to_schemas(&server_name, &server_tools));
        }
        let mut request = MessageRequest::new(&self.model, conversation.api_messages().to_vec())
            .with_system(&self.system_prompt)
            .with_max_tokens(self.max_tokens);
        request.tools = tool_schemas;
        request
    }

    async fn stream_one_turn(
        &self,
        conversation: &mut Conversation,
        event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
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
        assistant_msg.model = Some(self.model.clone());

        let mut current_text = String::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_input_json = String::new();

        while let Some(event_result) = stream.next().await {
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
                    let preview: String = thinking.chars().take(50).collect();
                    tracing::debug!("Thinking: {}", preview);
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

                        emit(event_tx, TurnEvent::ToolUseStart {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        }).await;

                        assistant_msg.content.push(ContentBlock::ToolUse {
                            id,
                            name,
                            input,
                        });
                        current_tool_input_json.clear();
                    }
                }
                StreamEvent::UsageUpdate(usage) => {
                    self.state_store.add_usage(&usage);
                    assistant_msg.usage = Some(usage);
                }
                StreamEvent::MessageStop { stop_reason } => {
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
                StreamEvent::Ping => {}
            }
        }

        self.state_store.set_streaming(false);

        // Add assistant message to state and conversation.
        self.state_store.push_message(assistant_msg.clone());
        conversation.push(assistant_msg.clone());

        Ok(assistant_msg)
    }

}
