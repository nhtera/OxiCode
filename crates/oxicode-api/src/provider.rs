use std::pin::Pin;

use futures::Stream;
use oxicode_common::{Message, OxiResult};

use crate::StreamEvent;

/// Request to send to an LLM provider.
#[derive(Debug, Clone)]
pub struct MessageRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub stream: bool,
    pub tools: Vec<serde_json::Value>,
}

impl MessageRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            system: None,
            max_tokens: oxicode_common::constants::DEFAULT_MAX_TOKENS,
            stream: true,
            tools: Vec::new(),
        }
    }

    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

/// A boxed async stream of `StreamEvents`.
pub type EventStream = Pin<Box<dyn Stream<Item = OxiResult<StreamEvent>> + Send>>;

/// Trait for LLM providers (Anthropic, `OpenAI`, etc.).
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream a message response as SSE events.
    async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream>;

    /// Provider name for logging/routing.
    fn name(&self) -> &str;
}
