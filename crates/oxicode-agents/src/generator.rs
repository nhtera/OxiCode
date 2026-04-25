//! AI-powered agent generator — calls an LLM to produce a coherent agent
//! configuration (identifier, when-to-use, system prompt) from a free-form
//! user description, mirroring openclaude's `generateAgent` flow.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use oxicode_api::{MessageRequest, ProviderRouter, StreamEvent};
use oxicode_common::Message;

const SYSTEM_PROMPT: &str = "You are an elite AI agent architect specializing in crafting high-performance agent configurations. Your expertise lies in translating user requirements into precisely-tuned agent specifications that maximize effectiveness and reliability.

When a user describes what they want an agent to do, you will:

1. **Extract Core Intent**: Identify the fundamental purpose, key responsibilities, and success criteria for the agent.
2. **Design Expert Persona**: Create a compelling expert identity that embodies deep domain knowledge relevant to the task.
3. **Architect Comprehensive Instructions**: Develop a system prompt that establishes clear behavioral boundaries, provides specific methodologies, anticipates edge cases, and defines output expectations.
4. **Optimize for Performance**: Include decision-making frameworks, quality control mechanisms, and clear escalation strategies.
5. **Create Identifier**: Design a concise, descriptive identifier using lowercase letters, numbers, and hyphens only (typically 2-4 words). Avoid generic terms like \"helper\" or \"assistant\".

Your output must be a valid JSON object with exactly these fields:
{
  \"identifier\": \"A unique, descriptive kebab-case identifier (e.g., 'test-runner', 'api-docs-writer')\",
  \"whenToUse\": \"A precise description starting with 'Use this agent when...' that defines triggering conditions and use cases.\",
  \"systemPrompt\": \"The complete system prompt for the agent, written in second person ('You are...', 'You will...').\"
}

Key principles:
- Be specific rather than generic.
- Include concrete examples when useful.
- Build in self-correction and quality assurance.
- Make agents proactive in seeking clarification when needed.

Return ONLY the JSON object, no other text.";

/// Result of a successful generation.
#[derive(Debug, Clone)]
pub struct GeneratedAgent {
    pub identifier: String,
    pub when_to_use: String,
    pub system_prompt: String,
}

/// Errors surfaced from agent generation.
#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("agent generation cancelled")]
    Cancelled,
    #[error("provider error: {0}")]
    Provider(String),
    #[error("invalid model response: {0}")]
    Parse(String),
    #[error("no LLM provider configured")]
    NoProvider,
}

/// Generate an agent configuration from a free-form user description.
///
/// `user_prompt` is what the agent should do (e.g. "review my Rust code for safety").
/// `model` is a model alias understood by [`ProviderRouter::resolve`] (e.g. "sonnet").
/// `existing_identifiers` are already-taken slugs the model must avoid.
/// `cancel` lets the caller abort the in-flight request.
pub async fn generate_agent(
    user_prompt: &str,
    model: &str,
    existing_identifiers: &[String],
    cancel: Arc<AtomicBool>,
) -> Result<GeneratedAgent, GenerateError> {
    let router = ProviderRouter::from_env();
    let resolved = router
        .resolve(model)
        .map_err(|_| GenerateError::NoProvider)?;

    let existing_list = if existing_identifiers.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nIMPORTANT: The following identifiers already exist and must NOT be used: {}",
            existing_identifiers.join(", ")
        )
    };

    let user_message = Message::user(format!(
        "Create an agent configuration based on this request: \"{user_prompt}\".{existing_list}\n  Return ONLY the JSON object, no other text."
    ));

    // 2048 was too small — long agent specs (whenToUse + examples + multi-paragraph
    // systemPrompt) commonly exceed it and get truncated mid-JSON, breaking parse.
    let request = MessageRequest::new(resolved.model.clone(), vec![user_message])
        .with_system(SYSTEM_PROMPT)
        .with_max_tokens(oxicode_common::constants::DEFAULT_MAX_TOKENS);

    let mut stream = resolved
        .provider
        .stream_message(request)
        .await
        .map_err(|e| GenerateError::Provider(e.to_string()))?;

    let mut buffer = String::new();
    while let Some(event) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            return Err(GenerateError::Cancelled);
        }
        match event {
            Ok(StreamEvent::TextDelta { text }) => buffer.push_str(&text),
            Ok(StreamEvent::MessageStop { .. }) => break,
            Ok(StreamEvent::Error { message } | StreamEvent::ApiError { message, .. }) => {
                return Err(GenerateError::Provider(message));
            }
            Err(e) => return Err(GenerateError::Provider(e.to_string())),
            _ => {}
        }
    }

    parse_response(&buffer)
}

fn parse_response(text: &str) -> Result<GeneratedAgent, GenerateError> {
    let trimmed = text.trim();
    let json_str = if trimmed.starts_with('{') && trimmed.ends_with('}') {
        trimmed.to_string()
    } else if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if end > start {
            trimmed[start..=end].to_string()
        } else {
            return Err(GenerateError::Parse(
                "no JSON object in response".to_string(),
            ));
        }
    } else {
        return Err(GenerateError::Parse(
            "no JSON object in response".to_string(),
        ));
    };

    let value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| GenerateError::Parse(format!("JSON parse error: {e}")))?;

    let identifier = value
        .get("identifier")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| GenerateError::Parse("missing 'identifier' field".to_string()))?
        .to_string();

    let when_to_use = value
        .get("whenToUse")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| GenerateError::Parse("missing 'whenToUse' field".to_string()))?
        .to_string();

    let system_prompt = value
        .get("systemPrompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| GenerateError::Parse("missing 'systemPrompt' field".to_string()))?
        .to_string();

    Ok(GeneratedAgent {
        identifier,
        when_to_use,
        system_prompt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json_object() {
        let raw = r#"{"identifier":"test-runner","whenToUse":"Use this agent when running tests","systemPrompt":"You are a test runner."}"#;
        let g = parse_response(raw).unwrap();
        assert_eq!(g.identifier, "test-runner");
        assert!(g.when_to_use.starts_with("Use this agent"));
        assert_eq!(g.system_prompt, "You are a test runner.");
    }

    #[test]
    fn extracts_json_from_surrounding_text() {
        let raw = r#"Here is the JSON:
{
  "identifier": "doc-writer",
  "whenToUse": "Use this agent when writing docs",
  "systemPrompt": "You are a docs expert."
}
That's it."#;
        let g = parse_response(raw).unwrap();
        assert_eq!(g.identifier, "doc-writer");
    }

    #[test]
    fn rejects_response_without_required_fields() {
        let raw = r#"{"identifier":"x","whenToUse":""}"#;
        assert!(matches!(parse_response(raw), Err(GenerateError::Parse(_))));
    }

    #[test]
    fn rejects_non_json() {
        assert!(matches!(
            parse_response("nothing here"),
            Err(GenerateError::Parse(_))
        ));
    }
}
