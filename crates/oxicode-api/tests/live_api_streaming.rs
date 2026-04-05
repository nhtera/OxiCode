//! Live API streaming integration tests.
//!
//! All tests require `ANTHROPIC_AUTH_TOKEN` env var and are gated behind `#[ignore]`.
//! Run with: `cargo test -p oxicode-api --test live_api_streaming -- --ignored --nocapture`

use futures::StreamExt;
use oxicode_api::{AnthropicProvider, LlmProvider, MessageRequest, StreamEvent};
use oxicode_common::Message;

/// Helper: create a live provider from env vars.
fn make_live_provider() -> AnthropicProvider {
    let token =
        std::env::var("ANTHROPIC_AUTH_TOKEN").expect("ANTHROPIC_AUTH_TOKEN env var required");
    let base_url = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
    AnthropicProvider::new(token).with_base_url(base_url)
}

/// Helper: resolve model name from env var or use default.
fn model_name() -> String {
    std::env::var("ANTHROPIC_DEFAULT_SONNET_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string())
}

#[tokio::test]
#[ignore]
async fn test_live_api_simple_text_response() {
    let provider = make_live_provider();
    let request = MessageRequest::new(model_name(), vec![Message::user("Say hello in one word")])
        .with_max_tokens(100);

    let mut stream = provider
        .stream_message(request)
        .await
        .expect("Failed to start stream");

    let mut got_text = false;
    let mut got_stop = false;

    while let Some(event) = stream.next().await {
        let event = event.expect("Stream event error");
        match event {
            StreamEvent::TextDelta { ref text } => {
                assert!(!text.is_empty(), "TextDelta should not be empty");
                got_text = true;
            }
            StreamEvent::MessageStop { stop_reason } => {
                assert_eq!(
                    stop_reason,
                    oxicode_common::StopReason::EndTurn,
                    "Simple text should end with EndTurn"
                );
                got_stop = true;
            }
            StreamEvent::Error { message } => panic!("API error: {message}"),
            _ => {} // Ignore other events
        }
    }

    assert!(got_text, "Should have received at least one TextDelta");
    assert!(got_stop, "Should have received MessageStop");
}

#[tokio::test]
#[ignore]
async fn test_live_api_tool_use_response() {
    let provider = make_live_provider();

    // Provide a tool schema so the model can use it.
    let tool_schema = serde_json::json!({
        "name": "file_read",
        "description": "Read a file from the filesystem",
        "input_schema": {
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
                }
            },
            "required": ["file_path"]
        }
    });

    let mut request =
        MessageRequest::new(model_name(), vec![Message::user("Read the file /tmp/test.txt")])
            .with_max_tokens(500);
    request.tools = vec![tool_schema];

    let mut stream = provider
        .stream_message(request)
        .await
        .expect("Failed to start stream");

    let mut got_tool_start = false;
    let mut got_tool_input = false;
    let mut got_block_stop = false;
    let mut tool_name = String::new();

    while let Some(event) = stream.next().await {
        let event = event.expect("Stream event error");
        match event {
            StreamEvent::ToolUseStart { name, .. } => {
                tool_name = name;
                got_tool_start = true;
            }
            StreamEvent::ToolInputDelta { .. } => {
                got_tool_input = true;
            }
            StreamEvent::ContentBlockStop { .. } => {
                got_block_stop = true;
            }
            StreamEvent::Error { message } => panic!("API error: {message}"),
            _ => {}
        }
    }

    assert!(got_tool_start, "Should have received ToolUseStart");
    assert_eq!(tool_name, "file_read", "Tool name should be file_read");
    assert!(got_tool_input, "Should have received ToolInputDelta");
    assert!(got_block_stop, "Should have received ContentBlockStop");
}

#[tokio::test]
#[ignore]
async fn test_live_api_streaming_token_usage() {
    let provider = make_live_provider();
    let request = MessageRequest::new(model_name(), vec![Message::user("Say hi")])
        .with_max_tokens(50);

    let mut stream = provider
        .stream_message(request)
        .await
        .expect("Failed to start stream");

    let mut got_usage = false;
    let mut total_output = 0u32;

    while let Some(event) = stream.next().await {
        let event = event.expect("Stream event error");
        if let StreamEvent::UsageUpdate(usage) = event {
            got_usage = true;
            total_output += usage.output_tokens;
        }
    }

    assert!(got_usage, "Should have received at least one UsageUpdate");
    assert!(
        total_output > 0,
        "Output tokens should be > 0, got {total_output}"
    );
}

#[tokio::test]
#[ignore]
async fn test_live_api_custom_base_url() {
    // This test validates that a custom base URL works.
    let token =
        std::env::var("ANTHROPIC_AUTH_TOKEN").expect("ANTHROPIC_AUTH_TOKEN env var required");
    let base_url = std::env::var("ANTHROPIC_BASE_URL");

    // Only meaningful if a custom base URL is set.
    let base_url = match base_url {
        Ok(url) if !url.is_empty() => url,
        _ => {
            eprintln!("Skipping test_live_api_custom_base_url: no ANTHROPIC_BASE_URL set");
            return;
        }
    };

    let provider = AnthropicProvider::new(token).with_base_url(&base_url);
    let request = MessageRequest::new(model_name(), vec![Message::user("Say OK")])
        .with_max_tokens(20);

    let mut stream = provider
        .stream_message(request)
        .await
        .expect("Failed to connect to custom base URL");

    let mut got_any_event = false;
    while let Some(event) = stream.next().await {
        let event = event.expect("Stream error on custom base URL");
        match event {
            StreamEvent::Error { message } => panic!("Custom URL API error: {message}"),
            _ => got_any_event = true,
        }
    }

    assert!(
        got_any_event,
        "Should receive events from custom base URL at {base_url}"
    );
}

#[tokio::test]
#[ignore]
async fn test_live_api_multi_turn_messages() {
    let provider = make_live_provider();

    // Build a 2-turn conversation history.
    let messages = vec![
        Message::user("What is 2+2?"),
        {
            let mut m = Message::assistant();
            m.content
                .push(oxicode_common::ContentBlock::Text { text: "4".into() });
            m
        },
        Message::user("And what is that number times 3?"),
    ];

    let request = MessageRequest::new(model_name(), messages).with_max_tokens(100);

    let mut stream = provider
        .stream_message(request)
        .await
        .expect("Failed to stream multi-turn");

    let mut response_text = String::new();
    while let Some(event) = stream.next().await {
        let event = event.expect("Stream event error");
        if let StreamEvent::TextDelta { text } = event {
            response_text.push_str(&text);
        }
    }

    assert!(
        !response_text.is_empty(),
        "Multi-turn response should not be empty"
    );
    // The answer should contain "12" somewhere.
    assert!(
        response_text.contains("12"),
        "Expected '12' in response, got: {response_text}"
    );
}
