//! Agent hook executor — runs hooks via LLM call with structured output.
//!
//! Agent hooks call an LLM provider (e.g., Haiku for speed) with the hook event
//! as context, expecting a structured JSON response (`pass`, `modify`, `abort`).
//!
//! - 60s timeout (configurable via `AgentHookConfig`)
//! - Fail-open: any error or timeout → `HookResponse::Pass`

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::events::{HookPayload, HookResponse};

/// Configuration specific to agent-type hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHookConfig {
    /// System prompt / instructions for the LLM call.
    pub instructions: String,

    /// Model to use (default: cheapest/fastest available).
    #[serde(default = "default_agent_model")]
    pub model: String,

    /// Timeout in seconds (default 60).
    #[serde(default = "default_agent_timeout_secs")]
    pub timeout_secs: u64,

    /// Max tokens for the LLM response.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_agent_model() -> String {
    "claude-haiku".to_string()
}
fn default_agent_timeout_secs() -> u64 {
    60
}
fn default_max_tokens() -> u32 {
    256
}

impl Default for AgentHookConfig {
    fn default() -> Self {
        Self {
            instructions: String::new(),
            model: default_agent_model(),
            timeout_secs: default_agent_timeout_secs(),
            max_tokens: default_max_tokens(),
        }
    }
}

/// Structured response expected from the LLM agent hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentHookResponse {
    /// Allow the operation to proceed.
    Pass,
    /// Modify the prompt with injected text.
    Modify { text: String },
    /// Abort the operation.
    Abort { reason: String },
}

impl From<AgentHookResponse> for HookResponse {
    fn from(resp: AgentHookResponse) -> Self {
        match resp {
            AgentHookResponse::Pass => HookResponse::Pass,
            AgentHookResponse::Modify { text } => HookResponse::ModifyPrompt { text },
            AgentHookResponse::Abort { reason } => HookResponse::Abort { reason },
        }
    }
}

/// Execute an agent hook by calling the LLM with structured output.
///
/// This function builds the prompt from the hook config instructions + event payload,
/// calls the LLM provider, and parses the structured response.
///
/// On any failure (network, parse, timeout), returns `HookResponse::Pass` (fail-open).
pub async fn execute_agent_hook(payload: &HookPayload, config: &AgentHookConfig) -> HookResponse {
    let timeout = Duration::from_secs(config.timeout_secs);

    let result = tokio::time::timeout(timeout, call_agent(payload, config)).await;

    match result {
        Ok(Ok(response)) => response.into(),
        Ok(Err(e)) => {
            tracing::warn!("Agent hook error: {e}");
            HookResponse::Pass
        }
        Err(_) => {
            tracing::warn!("Agent hook timed out after {}s", config.timeout_secs);
            HookResponse::Pass
        }
    }
}

/// Build prompt and call LLM provider.
///
/// Currently a stub that returns Pass — real implementation requires passing
/// an `LlmProvider` trait object from `oxicode-api`. The wiring path:
///   1. Add `oxicode-api` dep to `oxicode-hooks/Cargo.toml`
///   2. Accept `provider: &dyn LlmProvider` in `execute_agent_hook()`
///   3. Build `MessageRequest` from `system_prompt` + `user_message`
///   4. Stream response, collect text, parse via `parse_agent_response()`
///   5. Timeout via `tokio::time::timeout` (already in caller)
#[allow(clippy::unused_async)] // Will use async when wired to LLM provider.
async fn call_agent(
    payload: &HookPayload,
    config: &AgentHookConfig,
) -> Result<AgentHookResponse, String> {
    // Build the prompt combining instructions + payload context.
    let _system_prompt = build_system_prompt(config);
    let _user_message = build_user_message(payload)?;

    // TODO: Wire into oxicode-api provider trait.
    // For now, agent hooks pass through (fail-open by design).
    //
    // Future implementation:
    // let provider = get_provider(&config.model)?;
    // let response = provider.complete(system_prompt, user_message, structured_schema).await?;
    // parse_agent_response(&response)

    tracing::debug!(
        "Agent hook stub: model={}, event={}, instructions_len={}",
        config.model,
        payload.hook_event_name.as_str(),
        config.instructions.len()
    );

    Ok(AgentHookResponse::Pass)
}

/// Build system prompt for the agent hook LLM call.
fn build_system_prompt(config: &AgentHookConfig) -> String {
    format!(
        "You are a hook agent that evaluates events and returns a structured JSON response.\n\
         \n\
         Instructions: {}\n\
         \n\
         You MUST respond with exactly one of these JSON objects:\n\
         - {{\"action\": \"pass\"}} — allow the operation\n\
         - {{\"action\": \"modify\", \"text\": \"...\"}} — inject text into the prompt\n\
         - {{\"action\": \"abort\", \"reason\": \"...\"}} — cancel the operation\n\
         \n\
         Respond with ONLY the JSON object, no other text.",
        config.instructions
    )
}

/// Build user message from the hook event payload.
fn build_user_message(payload: &HookPayload) -> Result<String, String> {
    serde_json::to_string_pretty(payload).map_err(|e| format!("Failed to serialize payload: {e}"))
}

/// Parse LLM response text into a structured agent hook response.
pub fn parse_agent_response(text: &str) -> Result<AgentHookResponse, String> {
    let trimmed = text.trim();

    // Try direct parse.
    if let Ok(resp) = serde_json::from_str::<AgentHookResponse>(trimmed) {
        return Ok(resp);
    }

    // Try extracting JSON from markdown code block.
    if let Some(json_str) = extract_json_block(trimmed) {
        if let Ok(resp) = serde_json::from_str::<AgentHookResponse>(json_str) {
            return Ok(resp);
        }
    }

    Err(format!("Could not parse agent response: {trimmed}"))
}

/// Extract JSON from a markdown code block (```json ... ``` or ``` ... ```).
fn extract_json_block(text: &str) -> Option<&str> {
    let start = text.find("```")?;
    let after_fence = &text[start + 3..];
    // Skip optional language tag on the same line.
    let content_start = after_fence.find('\n')? + 1;
    let content = &after_fence[content_start..];
    let end = content.find("```")?;
    Some(content[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::HookEvent;

    fn test_payload() -> HookPayload {
        let mut p = HookPayload::new(HookEvent::UserPromptSubmit, "sess_1");
        p.prompt = Some("test prompt".to_string());
        p.model = Some("claude-sonnet-4".to_string());
        p
    }

    #[test]
    fn test_default_config() {
        let config = AgentHookConfig::default();
        assert_eq!(config.model, "claude-haiku");
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.max_tokens, 256);
        assert!(config.instructions.is_empty());
    }

    #[test]
    fn test_config_serde() {
        let config = AgentHookConfig {
            instructions: "Check for PII".to_string(),
            model: "claude-haiku".to_string(),
            timeout_secs: 30,
            max_tokens: 128,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AgentHookConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.instructions, "Check for PII");
        assert_eq!(parsed.timeout_secs, 30);
    }

    #[test]
    fn test_parse_pass() {
        let resp = parse_agent_response(r#"{"action":"pass"}"#).unwrap();
        assert!(matches!(resp, AgentHookResponse::Pass));
    }

    #[test]
    fn test_parse_modify() {
        let resp = parse_agent_response(r#"{"action":"modify","text":"added context"}"#).unwrap();
        assert!(matches!(resp, AgentHookResponse::Modify { text } if text == "added context"));
    }

    #[test]
    fn test_parse_abort() {
        let resp = parse_agent_response(r#"{"action":"abort","reason":"contains PII"}"#).unwrap();
        assert!(matches!(resp, AgentHookResponse::Abort { reason } if reason == "contains PII"));
    }

    #[test]
    fn test_parse_from_code_block() {
        let text = "Here is my response:\n```json\n{\"action\":\"pass\"}\n```";
        let resp = parse_agent_response(text).unwrap();
        assert!(matches!(resp, AgentHookResponse::Pass));
    }

    #[test]
    fn test_parse_invalid() {
        let result = parse_agent_response("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_agent_response_to_hook_response() {
        assert!(matches!(
            HookResponse::from(AgentHookResponse::Pass),
            HookResponse::Pass
        ));
        assert!(matches!(
            HookResponse::from(AgentHookResponse::Modify { text: "x".into() }),
            HookResponse::ModifyPrompt { text } if text == "x"
        ));
        assert!(matches!(
            HookResponse::from(AgentHookResponse::Abort { reason: "y".into() }),
            HookResponse::Abort { reason } if reason == "y"
        ));
    }

    #[test]
    fn test_build_system_prompt() {
        let config = AgentHookConfig {
            instructions: "Reject harmful queries".to_string(),
            ..Default::default()
        };
        let prompt = build_system_prompt(&config);
        assert!(prompt.contains("Reject harmful queries"));
        assert!(prompt.contains("action"));
    }

    #[test]
    fn test_build_user_message() {
        let payload = test_payload();
        let msg = build_user_message(&payload).unwrap();
        assert!(msg.contains("UserPromptSubmit"));
        assert!(msg.contains("test prompt"));
    }

    #[tokio::test]
    async fn test_execute_agent_hook_stub_returns_pass() {
        let payload = test_payload();
        let config = AgentHookConfig {
            instructions: "Test instructions".to_string(),
            ..Default::default()
        };
        // Stub implementation always returns Pass.
        let response = execute_agent_hook(&payload, &config).await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[test]
    fn test_extract_json_block() {
        let text = "```json\n{\"action\":\"pass\"}\n```";
        assert_eq!(extract_json_block(text), Some("{\"action\":\"pass\"}"));

        let text2 = "```\n{\"key\":\"val\"}\n```";
        assert_eq!(extract_json_block(text2), Some("{\"key\":\"val\"}"));

        assert!(extract_json_block("no code block here").is_none());
    }

    #[test]
    fn test_parse_with_whitespace() {
        let resp = parse_agent_response("  \n  {\"action\":\"pass\"}  \n  ").unwrap();
        assert!(matches!(resp, AgentHookResponse::Pass));
    }
}
