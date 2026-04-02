use oxicode_api::{LlmProvider, MessageRequest};
use oxicode_common::{Message, OxiError, OxiResult};

/// System prompt sent to the LLM when compacting a conversation.
const COMPACT_SYSTEM_PROMPT: &str = "\
Summarize this conversation concisely. Preserve: key decisions, file paths \
mentioned, current task state, important code snippets. \
Output as a structured summary.";

/// Tokens reserved for the summary output.
const SUMMARY_MAX_TOKENS: u32 = 4096;

/// Layer-3 defense: use the LLM itself to summarize the conversation.
pub struct AutoCompactor;

impl AutoCompactor {
    /// Compact `messages` into a single User message containing an LLM-generated summary.
    ///
    /// On LLM failure returns `OxiError::Other`.
    pub async fn compact(
        messages: &[Message],
        provider: &dyn LlmProvider,
        model: &str,
    ) -> OxiResult<Message> {
        tracing::info!(
            model,
            message_count = messages.len(),
            "L3: auto-compact via LLM"
        );

        let request = MessageRequest {
            model: model.to_string(),
            messages: messages.to_vec(),
            system: Some(COMPACT_SYSTEM_PROMPT.to_string()),
            max_tokens: SUMMARY_MAX_TOKENS,
            stream: false,
            tools: Vec::new(),
        };

        let stream = provider
            .stream_message(request)
            .await
            .map_err(|e| OxiError::Other(format!("L3 compact failed: {e}")))?;

        let summary = collect_stream_text(stream).await?;

        if summary.is_empty() {
            return Err(OxiError::Other(
                "L3 compact returned empty summary".to_string(),
            ));
        }

        tracing::info!(summary_len = summary.len(), "L3: summary received");

        Ok(Message::user(format!(
            "[Conversation summary]\n{summary}"
        )))
    }
}

/// Drain an `EventStream` and concatenate all text deltas into a single string.
async fn collect_stream_text(
    mut stream: oxicode_api::EventStream,
) -> OxiResult<String> {
    use futures::StreamExt;
    use oxicode_api::StreamEvent;

    let mut buf = String::new();

    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::TextDelta { text } => buf.push_str(&text),
            StreamEvent::MessageStop { .. } => break,
            _ => {}
        }
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the system prompt is non-empty and mentions key preservation terms.
    #[test]
    fn compact_system_prompt_content() {
        assert!(COMPACT_SYSTEM_PROMPT.contains("Summarize"));
        assert!(COMPACT_SYSTEM_PROMPT.contains("file paths"));
        assert!(COMPACT_SYSTEM_PROMPT.contains("code snippets"));
    }

    /// Verify summary max tokens is reasonable.
    #[test]
    fn summary_max_tokens_reasonable() {
        assert!(SUMMARY_MAX_TOKENS >= 1024);
        assert!(SUMMARY_MAX_TOKENS <= 8192);
    }
}
