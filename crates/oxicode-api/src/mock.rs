//! Mock LLM provider for testing.
//!
//! Provides a configurable `MockLlmProvider` that returns predefined
//! `StreamEvent` sequences. Used by oxicode-core and integration tests
//! to test query engine, tool dispatch, and conversation flows
//! without making real API calls.

use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_stream::stream;
use futures::Stream;
use oxicode_common::{OxiResult, StopReason, Usage};

use crate::provider::{EventStream, LlmProvider, MessageRequest};
use crate::StreamEvent;

/// A mock LLM provider that returns pre-configured stream event sequences.
///
/// Each call to `stream_message` pops the next sequence from the queue.
/// If the queue is exhausted, returns a default text response.
#[derive(Clone)]
pub struct MockLlmProvider {
    /// Pre-configured response sequences — each inner Vec is one call.
    responses: Arc<Vec<Vec<StreamEvent>>>,
    /// Tracks which response to return next.
    call_index: Arc<AtomicUsize>,
    /// Provider name for logging.
    provider_name: String,
}

impl MockLlmProvider {
    /// Create a mock provider with a list of response sequences.
    ///
    /// Each `stream_message` call returns the next sequence in order.
    pub fn new(responses: Vec<Vec<StreamEvent>>) -> Self {
        Self {
            responses: Arc::new(responses),
            call_index: Arc::new(AtomicUsize::new(0)),
            provider_name: "mock".to_string(),
        }
    }

    /// Create a simple mock that always returns a single text response.
    pub fn with_text(text: &str) -> Self {
        let events = text_response_events(text);
        Self::new(vec![events])
    }

    /// Create a mock that returns a tool_use response followed by a text response.
    pub fn with_tool_then_text(
        tool_id: &str,
        tool_name: &str,
        tool_input: serde_json::Value,
        final_text: &str,
    ) -> Self {
        let tool_events = tool_use_response_events(tool_id, tool_name, &tool_input);
        let text_events = text_response_events(final_text);
        Self::new(vec![tool_events, text_events])
    }

    /// Get number of calls made so far.
    pub fn call_count(&self) -> usize {
        self.call_index.load(Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockLlmProvider {
    async fn stream_message(&self, _request: MessageRequest) -> OxiResult<EventStream> {
        let idx = self.call_index.fetch_add(1, Ordering::Relaxed);
        let events = if idx < self.responses.len() {
            self.responses[idx].clone()
        } else {
            // Default: simple text response.
            text_response_events("(mock default response)")
        };

        let stream = stream! {
            for event in events {
                yield Ok(event);
            }
        };

        Ok(Box::pin(stream)
            as Pin<
                Box<dyn Stream<Item = OxiResult<StreamEvent>> + Send>,
            >)
    }

    fn name(&self) -> &str {
        &self.provider_name
    }
}

/// Build a stream event sequence for a simple text response.
pub fn text_response_events(text: &str) -> Vec<StreamEvent> {
    vec![
        StreamEvent::TextDelta {
            text: text.to_string(),
        },
        StreamEvent::ContentBlockStop { index: 0 },
        StreamEvent::UsageUpdate(Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
        StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        },
    ]
}

/// Build a stream event sequence for a tool use response.
pub fn tool_use_response_events(
    tool_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
) -> Vec<StreamEvent> {
    let input_json = serde_json::to_string(input).unwrap_or_default();
    vec![
        StreamEvent::ToolUseStart {
            id: tool_id.to_string(),
            name: tool_name.to_string(),
        },
        StreamEvent::ToolInputDelta {
            partial_json: input_json,
        },
        StreamEvent::ContentBlockStop { index: 0 },
        StreamEvent::UsageUpdate(Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
        StreamEvent::MessageStop {
            stop_reason: StopReason::ToolUse,
        },
    ]
}

/// Build a stream event sequence that returns an error.
pub fn error_response_events(message: &str) -> Vec<StreamEvent> {
    vec![StreamEvent::Error {
        message: message.to_string(),
    }]
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;

    #[tokio::test]
    async fn test_mock_text_response() {
        let provider = MockLlmProvider::with_text("Hello, world!");
        let request = MessageRequest::new("test-model", vec![]);
        let mut stream = provider.stream_message(request).await.unwrap();

        let mut text = String::new();
        while let Some(Ok(event)) = stream.next().await {
            if let StreamEvent::TextDelta { text: t } = event {
                text.push_str(&t);
            }
        }
        assert_eq!(text, "Hello, world!");
        assert_eq!(provider.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_tool_response() {
        let provider = MockLlmProvider::with_tool_then_text(
            "t1",
            "bash",
            serde_json::json!({"command": "ls"}),
            "Done!",
        );

        let request = MessageRequest::new("test-model", vec![]);

        // First call: tool_use.
        let mut stream = provider.stream_message(request.clone()).await.unwrap();
        let mut saw_tool_start = false;
        let mut stop = StopReason::EndTurn;
        while let Some(Ok(event)) = stream.next().await {
            match event {
                StreamEvent::ToolUseStart { name, .. } => {
                    assert_eq!(name, "bash");
                    saw_tool_start = true;
                }
                StreamEvent::MessageStop { stop_reason } => stop = stop_reason,
                _ => {}
            }
        }
        assert!(saw_tool_start);
        assert_eq!(stop, StopReason::ToolUse);

        // Second call: text.
        let mut stream = provider.stream_message(request).await.unwrap();
        let mut text = String::new();
        while let Some(Ok(event)) = stream.next().await {
            if let StreamEvent::TextDelta { text: t } = event {
                text.push_str(&t);
            }
        }
        assert_eq!(text, "Done!");
        assert_eq!(provider.call_count(), 2);
    }

    #[tokio::test]
    async fn test_mock_error_response() {
        let events = error_response_events("API error");
        let provider = MockLlmProvider::new(vec![events]);
        let request = MessageRequest::new("test-model", vec![]);
        let mut stream = provider.stream_message(request).await.unwrap();

        let mut got_error = false;
        while let Some(Ok(event)) = stream.next().await {
            if let StreamEvent::Error { message } = event {
                assert_eq!(message, "API error");
                got_error = true;
            }
        }
        assert!(got_error);
    }

    #[tokio::test]
    async fn test_mock_default_after_exhaustion() {
        let provider = MockLlmProvider::new(vec![text_response_events("first")]);
        let request = MessageRequest::new("test-model", vec![]);

        // First call: configured response.
        let _ = provider.stream_message(request.clone()).await.unwrap();

        // Second call: default fallback.
        let mut stream = provider.stream_message(request).await.unwrap();
        let mut text = String::new();
        while let Some(Ok(event)) = stream.next().await {
            if let StreamEvent::TextDelta { text: t } = event {
                text.push_str(&t);
            }
        }
        assert_eq!(text, "(mock default response)");
        assert_eq!(provider.call_count(), 2);
    }

    #[tokio::test]
    async fn test_mock_name() {
        let provider = MockLlmProvider::with_text("test");
        assert_eq!(provider.name(), "mock");
    }

    #[tokio::test]
    async fn test_text_response_events_structure() {
        let events = text_response_events("hello");
        assert_eq!(events.len(), 4);
        assert!(matches!(&events[0], StreamEvent::TextDelta { text } if text == "hello"));
        assert!(matches!(
            &events[1],
            StreamEvent::ContentBlockStop { index: 0 }
        ));
        assert!(matches!(&events[2], StreamEvent::UsageUpdate(_)));
        assert!(matches!(
            &events[3],
            StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn
            }
        ));
    }

    #[tokio::test]
    async fn test_tool_use_events_structure() {
        let events =
            tool_use_response_events("t1", "file_read", &serde_json::json!({"path": "/tmp"}));
        assert_eq!(events.len(), 5);
        assert!(
            matches!(&events[0], StreamEvent::ToolUseStart { id, name } if id == "t1" && name == "file_read")
        );
        assert!(matches!(&events[1], StreamEvent::ToolInputDelta { .. }));
        assert!(matches!(
            &events[4],
            StreamEvent::MessageStop {
                stop_reason: StopReason::ToolUse
            }
        ));
    }
}
