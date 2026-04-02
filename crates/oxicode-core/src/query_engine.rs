use std::sync::Arc;

use futures::StreamExt;
use oxicode_api::{LlmProvider, MessageRequest, StreamEvent};
use oxicode_common::{ContentBlock, Message, OxiError, OxiResult, Role, StopReason};
use oxicode_context::BudgetManager;
use oxicode_permissions::{PermissionDecision, PermissionPipeline};
use oxicode_state::StateStore;
use oxicode_tools::{PermissionLevel, ToolContext, ToolRegistry};
use tokio::sync::Mutex;

use crate::conversation::Conversation;

/// Maximum number of tool-use turns before forcing a stop.
const MAX_TOOL_TURNS: usize = 50;

/// Typical Claude model context window (used as default budget ceiling).
const DEFAULT_MODEL_MAX_TOKENS: usize = 200_000;

/// Multi-turn query engine with tool execution support.
pub struct QueryEngine {
    provider: Arc<dyn LlmProvider>,
    state_store: Arc<StateStore>,
    tool_registry: Arc<ToolRegistry>,
    permission_pipeline: Arc<PermissionPipeline>,
    tool_context: ToolContext,
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
    pub async fn execute_turn(&self, conversation: &mut Conversation) -> OxiResult<Message> {
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
                // Only replace when messages were actually compacted.
                if defended.len() < conversation.len() {
                    tracing::info!(
                        before = conversation.len(),
                        after = defended.len(),
                        "context compacted before API call"
                    );
                    conversation.replace_messages(defended);
                }
            }

            let assistant_msg = self.stream_one_turn(conversation).await?;
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
                // No tool calls or LLM signaled end — we're done.
                return Ok(assistant_msg);
            }

            // Execute each tool and build a user message with tool results.
            let mut tool_results = Vec::new();
            for (id, name, input) in &tool_uses {
                let result = self.execute_tool(id, name, input).await;
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

            // If stop reason was ToolUse, loop back for the LLM to continue.
            if stop_reason != StopReason::ToolUse {
                return Ok(assistant_msg);
            }
        }

        Err(OxiError::Other("Max tool turns exceeded".to_string()))
    }

    /// Stream a single LLM turn, collecting the full assistant message.
    async fn stream_one_turn(&self, conversation: &mut Conversation) -> OxiResult<Message> {
        let tool_schemas = self.tool_registry.schemas_json();

        let mut request = MessageRequest::new(&self.model, conversation.api_messages().to_vec())
            .with_system(&self.system_prompt)
            .with_max_tokens(self.max_tokens);
        request.tools = tool_schemas;

        self.state_store.set_streaming(true);

        let mut stream = self.provider.stream_message(request).await?;
        let mut assistant_msg = Message::assistant();
        assistant_msg.model = Some(self.model.clone());

        let mut current_text = String::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_input_json = String::new();

        while let Some(event_result) = stream.next().await {
            let event = event_result?;

            match event {
                StreamEvent::TextDelta { text } => {
                    current_text.push_str(&text);
                }
                StreamEvent::ThinkingDelta { thinking } => {
                    tracing::debug!("Thinking: {}", &thinking[..thinking.len().min(50)]);
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

                        assistant_msg.content.push(ContentBlock::ToolUse {
                            id: std::mem::take(&mut current_tool_id),
                            name: std::mem::take(&mut current_tool_name),
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

                    if stop_reason == StopReason::MaxTokens {
                        tracing::warn!("Response truncated — max tokens reached");
                    }
                    break;
                }
                StreamEvent::Error { message } => {
                    self.state_store.set_streaming(false);
                    return Err(OxiError::api(message));
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

    /// Execute a single tool, checking permissions first.
    async fn execute_tool(
        &self,
        tool_use_id: &str,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> ContentBlock {
        // Map tool permission level to the permissions crate type.
        let tool_level = self.tool_registry.get(tool_name).map_or(
            oxicode_permissions::pipeline::ToolPermissionLevel::System,
            |t| match t.permission_level() {
                PermissionLevel::ReadOnly => {
                    oxicode_permissions::pipeline::ToolPermissionLevel::ReadOnly
                }
                PermissionLevel::FileWrite => {
                    oxicode_permissions::pipeline::ToolPermissionLevel::FileWrite
                }
                PermissionLevel::ShellExec => {
                    oxicode_permissions::pipeline::ToolPermissionLevel::ShellExec
                }
                PermissionLevel::System => {
                    oxicode_permissions::pipeline::ToolPermissionLevel::System
                }
            },
        );

        // Check permissions.
        let decision = self.permission_pipeline.check(tool_name, tool_level, input);

        match decision {
            PermissionDecision::Allow => {
                // Execute the tool.
                match self
                    .tool_registry
                    .execute(tool_name, input.clone(), &self.tool_context)
                    .await
                {
                    Ok(result) => ContentBlock::ToolResult {
                        tool_use_id: tool_use_id.to_string(),
                        content: result.content,
                        is_error: result.is_error,
                    },
                    Err(e) => ContentBlock::ToolResult {
                        tool_use_id: tool_use_id.to_string(),
                        content: format!("Tool error: {e}"),
                        is_error: true,
                    },
                }
            }
            PermissionDecision::Deny(reason) => {
                self.permission_pipeline.record_denial(tool_name, &reason);
                ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.to_string(),
                    content: format!("Permission denied: {reason}"),
                    is_error: true,
                }
            }
            PermissionDecision::Ask(prompt) => {
                // TODO: In Phase 2 TUI integration, this will send a permission
                // dialog event and wait for user response. For now, auto-deny.
                tracing::info!("Permission ask (auto-deny for now): {}", prompt);
                ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.to_string(),
                    content: format!("Permission required: {prompt}"),
                    is_error: true,
                }
            }
        }
    }
}
