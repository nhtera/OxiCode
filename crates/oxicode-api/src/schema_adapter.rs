//! Schema adapter: converts between Claude tool format and OpenAI function format.
//!
//! Claude tools use `input_schema` at the top level, while OpenAI wraps
//! everything in a `function` object with `parameters`.

use serde_json::Value;

/// Convert a Claude tool definition to OpenAI function-calling format.
///
/// Claude: `{ "name": "bash", "description": "...", "input_schema": {...} }`
/// OpenAI: `{ "type": "function", "function": { "name": "bash", "description": "...", "parameters": {...} } }`
pub fn claude_tool_to_openai_function(tool: &Value) -> Value {
    let name = tool.get("name").cloned().unwrap_or(Value::Null);
    let description = tool.get("description").cloned().unwrap_or(Value::Null);
    let parameters = tool
        .get("input_schema")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));

    serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    })
}

/// Convert a batch of Claude tool definitions to OpenAI function format.
pub fn claude_tools_to_openai_functions(tools: &[Value]) -> Vec<Value> {
    tools.iter().map(claude_tool_to_openai_function).collect()
}

/// Parsed OpenAI tool call from a streaming or non-streaming response.
#[derive(Debug, Clone)]
pub struct OpenAiToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Convert OpenAI `finish_reason` string to our `StopReason`.
pub fn openai_finish_reason_to_stop_reason(
    reason: &str,
) -> oxicode_common::StopReason {
    match reason {
        "stop" => oxicode_common::StopReason::EndTurn,
        "tool_calls" => oxicode_common::StopReason::ToolUse,
        "length" => oxicode_common::StopReason::MaxTokens,
        "content_filter" => oxicode_common::StopReason::EndTurn,
        _ => oxicode_common::StopReason::EndTurn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_to_openai_tool() {
        let claude_tool = serde_json::json!({
            "name": "bash",
            "description": "Run bash command",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The command" }
                },
                "required": ["command"]
            }
        });

        let openai = claude_tool_to_openai_function(&claude_tool);
        assert_eq!(openai["type"], "function");
        assert_eq!(openai["function"]["name"], "bash");
        assert_eq!(openai["function"]["parameters"]["type"], "object");
        assert!(openai["function"]["parameters"]["properties"]["command"].is_object());
    }

    #[test]
    fn test_batch_conversion() {
        let tools = vec![
            serde_json::json!({"name": "read", "description": "Read file", "input_schema": {"type": "object"}}),
            serde_json::json!({"name": "write", "description": "Write file", "input_schema": {"type": "object"}}),
        ];
        let functions = claude_tools_to_openai_functions(&tools);
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0]["function"]["name"], "read");
        assert_eq!(functions[1]["function"]["name"], "write");
    }

    #[test]
    fn test_finish_reason_mapping() {
        assert_eq!(
            openai_finish_reason_to_stop_reason("stop"),
            oxicode_common::StopReason::EndTurn
        );
        assert_eq!(
            openai_finish_reason_to_stop_reason("tool_calls"),
            oxicode_common::StopReason::ToolUse
        );
        assert_eq!(
            openai_finish_reason_to_stop_reason("length"),
            oxicode_common::StopReason::MaxTokens
        );
    }

    #[test]
    fn test_missing_input_schema_defaults() {
        let tool = serde_json::json!({"name": "simple", "description": "No schema"});
        let openai = claude_tool_to_openai_function(&tool);
        assert_eq!(openai["function"]["parameters"]["type"], "object");
    }
}
