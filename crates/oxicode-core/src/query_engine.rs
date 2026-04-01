use std::sync::Arc;

use futures::StreamExt;
use oxicode_api::{LlmProvider, MessageRequest, StreamEvent};
use oxicode_common::{ContentBlock, Message, OxiError, OxiResult, StopReason};
use oxicode_state::StateStore;

use crate::conversation::Conversation;

/// Single-turn query engine: takes user input, streams LLM response.
pub struct QueryEngine {
    provider: Arc<dyn LlmProvider>,
    state_store: Arc<StateStore>,
    model: String,
    max_tokens: u32,
    system_prompt: String,
}

impl QueryEngine {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        state_store: Arc<StateStore>,
        model: String,
        max_tokens: u32,
        system_prompt: String,
    ) -> Self {
        Self {
            provider,
            state_store,
            model,
            max_tokens,
            system_prompt,
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

    /// Execute a single conversation turn.
    /// Sends all messages to the LLM and streams the response.
    /// Returns the completed assistant message.
    pub async fn execute_turn(&self, conversation: &mut Conversation) -> OxiResult<Message> {
        let request = MessageRequest::new(&self.model, conversation.api_messages().to_vec())
            .with_system(&self.system_prompt)
            .with_max_tokens(self.max_tokens);

        self.state_store.set_streaming(true);

        let mut stream = self.provider.stream_message(request).await?;
        let mut assistant_msg = Message::assistant();
        assistant_msg.model = Some(self.model.clone());
        let mut current_text = String::new();

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
                    // Finalize any pending text block
                    if !current_text.is_empty() {
                        assistant_msg.content.push(ContentBlock::Text {
                            text: current_text.clone(),
                        });
                        current_text.clear();
                    }
                    tracing::debug!("Tool use start: {} ({})", name, id);
                }
                StreamEvent::UsageUpdate(usage) => {
                    self.state_store.add_usage(&usage);
                    assistant_msg.usage = Some(usage);
                }
                StreamEvent::MessageStop { stop_reason } => {
                    // Finalize pending text
                    if !current_text.is_empty() {
                        assistant_msg.content.push(ContentBlock::Text {
                            text: current_text.clone(),
                        });
                        current_text.clear();
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
                StreamEvent::Ping
                | StreamEvent::ToolInputDelta { .. }
                | StreamEvent::ContentBlockStop { .. } => {}
            }
        }

        self.state_store.set_streaming(false);

        // Add assistant message to state
        self.state_store.push_message(assistant_msg.clone());
        conversation.push(assistant_msg.clone());

        Ok(assistant_msg)
    }
}
