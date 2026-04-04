//! MCP elicitation handler: MCP servers can request user input during tool execution.
//!
//! Handles the `elicitation/create` MCP notification by forwarding the request
//! to a user-facing callback (TUI dialog, CLI prompt, etc.).

use serde::{Deserialize, Serialize};

/// An elicitation request from an MCP server asking the user for input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationRequest {
    /// Unique request ID for correlating response.
    pub id: String,
    /// Message to display to the user.
    pub message: String,
    /// Type of input requested.
    #[serde(default)]
    pub input_type: ElicitationInputType,
    /// Predefined choices (for `Select` input type).
    #[serde(default)]
    pub choices: Vec<String>,
    /// Default value to pre-fill.
    #[serde(default)]
    pub default_value: Option<String>,
}

/// Type of user input an MCP server is requesting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElicitationInputType {
    /// Free-form text input.
    #[default]
    Text,
    /// Yes/No confirmation.
    Confirm,
    /// Selection from predefined choices.
    Select,
    /// Secret input (e.g., password — not echoed).
    Secret,
}

/// The user's response to an elicitation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResponse {
    /// Matches the request ID.
    pub id: String,
    /// Whether the user approved/provided input (false = cancelled/denied).
    pub approved: bool,
    /// The user's input value (empty if cancelled).
    #[serde(default)]
    pub value: String,
}

/// Trait for handling elicitation requests.
///
/// Implementors decide how to present the request to the user:
/// - TUI: show a dialog overlay
/// - CLI: prompt on stdin
/// - Headless: auto-deny or use defaults
pub trait ElicitationHandler: Send + Sync {
    /// Handle an elicitation request, returning the user's response.
    fn handle(&self, request: &ElicitationRequest) -> ElicitationResponse;
}

/// Default handler that auto-denies all elicitation requests.
/// Used when no interactive handler is configured.
pub struct DenyAllHandler;

impl ElicitationHandler for DenyAllHandler {
    fn handle(&self, request: &ElicitationRequest) -> ElicitationResponse {
        tracing::info!(
            "Auto-denying elicitation request '{}': {}",
            request.id,
            request.message
        );
        ElicitationResponse {
            id: request.id.clone(),
            approved: false,
            value: String::new(),
        }
    }
}

/// Handler that uses the default value if available, otherwise denies.
pub struct DefaultValueHandler;

impl ElicitationHandler for DefaultValueHandler {
    fn handle(&self, request: &ElicitationRequest) -> ElicitationResponse {
        if let Some(ref default) = request.default_value {
            tracing::info!(
                "Auto-accepting elicitation '{}' with default: {default}",
                request.id
            );
            ElicitationResponse {
                id: request.id.clone(),
                approved: true,
                value: default.clone(),
            }
        } else {
            tracing::info!("No default for elicitation '{}', denying", request.id);
            ElicitationResponse {
                id: request.id.clone(),
                approved: false,
                value: String::new(),
            }
        }
    }
}

/// Parse an elicitation request from a JSON-RPC notification params.
pub fn parse_elicitation(params: &serde_json::Value) -> Option<ElicitationRequest> {
    serde_json::from_value(params.clone()).ok()
}

/// Build the JSON-RPC response to send back to the MCP server.
pub fn build_elicitation_response(response: &ElicitationResponse) -> serde_json::Value {
    serde_json::json!({
        "id": response.id,
        "approved": response.approved,
        "value": response.value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_elicitation_text() {
        let params = serde_json::json!({
            "id": "req-1",
            "message": "Enter your API key",
            "input_type": "text",
        });
        let req = parse_elicitation(&params).unwrap();
        assert_eq!(req.id, "req-1");
        assert_eq!(req.message, "Enter your API key");
        assert!(matches!(req.input_type, ElicitationInputType::Text));
    }

    #[test]
    fn test_parse_elicitation_confirm() {
        let params = serde_json::json!({
            "id": "req-2",
            "message": "Allow file access?",
            "input_type": "confirm",
        });
        let req = parse_elicitation(&params).unwrap();
        assert!(matches!(req.input_type, ElicitationInputType::Confirm));
    }

    #[test]
    fn test_parse_elicitation_select() {
        let params = serde_json::json!({
            "id": "req-3",
            "message": "Choose a model",
            "input_type": "select",
            "choices": ["gpt-4", "claude-3", "gemini"],
        });
        let req = parse_elicitation(&params).unwrap();
        assert!(matches!(req.input_type, ElicitationInputType::Select));
        assert_eq!(req.choices.len(), 3);
    }

    #[test]
    fn test_deny_all_handler() {
        let handler = DenyAllHandler;
        let req = ElicitationRequest {
            id: "test-1".to_string(),
            message: "Do something?".to_string(),
            input_type: ElicitationInputType::Confirm,
            choices: vec![],
            default_value: None,
        };
        let resp = handler.handle(&req);
        assert_eq!(resp.id, "test-1");
        assert!(!resp.approved);
    }

    #[test]
    fn test_default_value_handler_with_default() {
        let handler = DefaultValueHandler;
        let req = ElicitationRequest {
            id: "test-2".to_string(),
            message: "Enter name".to_string(),
            input_type: ElicitationInputType::Text,
            choices: vec![],
            default_value: Some("default-name".to_string()),
        };
        let resp = handler.handle(&req);
        assert!(resp.approved);
        assert_eq!(resp.value, "default-name");
    }

    #[test]
    fn test_default_value_handler_without_default() {
        let handler = DefaultValueHandler;
        let req = ElicitationRequest {
            id: "test-3".to_string(),
            message: "Enter secret".to_string(),
            input_type: ElicitationInputType::Secret,
            choices: vec![],
            default_value: None,
        };
        let resp = handler.handle(&req);
        assert!(!resp.approved);
    }

    #[test]
    fn test_build_elicitation_response() {
        let resp = ElicitationResponse {
            id: "req-1".to_string(),
            approved: true,
            value: "my-value".to_string(),
        };
        let json = build_elicitation_response(&resp);
        assert_eq!(json["id"], "req-1");
        assert_eq!(json["approved"], true);
        assert_eq!(json["value"], "my-value");
    }
}
