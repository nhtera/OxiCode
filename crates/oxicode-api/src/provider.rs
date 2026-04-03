use std::pin::Pin;

use futures::Stream;
use oxicode_common::{Message, OxiResult};

use crate::StreamEvent;

/// Extended thinking configuration.
#[derive(Debug, Clone, Default)]
pub struct ThinkingConfig {
    pub enabled: bool,
    pub budget_tokens: u32,
}

/// Request to send to an LLM provider.
#[derive(Debug, Clone)]
pub struct MessageRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub stream: bool,
    pub tools: Vec<serde_json::Value>,
    /// Enable prompt caching (Anthropic-specific).
    pub prompt_caching: bool,
    /// Extended thinking configuration.
    pub thinking: Option<ThinkingConfig>,
    /// Beta features to request (e.g., "prompt-caching-2024-07-31").
    pub beta_features: Vec<String>,
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
            prompt_caching: false,
            thinking: None,
            beta_features: Vec::new(),
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

    pub fn with_prompt_caching(mut self, enabled: bool) -> Self {
        self.prompt_caching = enabled;
        if enabled {
            self.beta_features
                .retain(|f| f != "prompt-caching-2024-07-31");
            self.beta_features
                .push("prompt-caching-2024-07-31".to_string());
        }
        self
    }

    /// Enable extended thinking with the given token budget.
    /// Budget is clamped to a minimum of 1024 (Anthropic API requirement).
    pub fn with_thinking(mut self, budget_tokens: u32) -> Self {
        self.thinking = Some(ThinkingConfig {
            enabled: true,
            budget_tokens: budget_tokens.max(1024),
        });
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
