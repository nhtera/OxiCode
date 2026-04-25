//! Tool dispatch: permission checks, built-in/MCP routing, and permission dialog integration.

use oxicode_common::{ContentBlock, OxiError, PermissionResponse};
use oxicode_hooks::{HookEvent, HookResponse};
use oxicode_permissions::PermissionDecision;
use oxicode_tools::tool_search::search_tools;
use oxicode_tools::PermissionLevel;

use crate::query_engine::QueryEngine;
use crate::turn_event::{emit, fire_hook_with_events, TurnEvent};

impl QueryEngine {
    /// Collect MCP tool definitions grouped by server name.
    pub(crate) fn mcp_tool_schemas(&self) -> Vec<(String, Vec<oxicode_mcp::McpToolDef>)> {
        let mut by_server: std::collections::HashMap<String, Vec<oxicode_mcp::McpToolDef>> =
            std::collections::HashMap::new();
        for (server_name, server) in self.tool_context.mcp_manager.all_tools() {
            let sn = server_name
                .split("__")
                .next()
                .unwrap_or(&server_name)
                .to_string();
            by_server.entry(sn).or_default().push(server.clone());
        }
        by_server.into_iter().collect()
    }

    /// Try executing an MCP tool by its prefixed name (server__tool).
    async fn try_mcp_tool(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> Option<oxicode_tools::ToolResult> {
        let (server, tool) = oxicode_mcp::McpServerManager::resolve_tool_name(tool_name)?;
        match self
            .tool_context
            .mcp_manager
            .call_tool(server, tool, input.clone())
            .await
        {
            Ok(result) => {
                let text: String = result
                    .content
                    .iter()
                    .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
                let has_non_text = result.content.iter().any(|c| c.as_text().is_none());
                let mut output = if text.is_empty() {
                    "(no text content returned)".to_string()
                } else {
                    text
                };
                if has_non_text {
                    output.push_str(
                        "\n[Note: non-text content (images/resources) was returned but omitted]",
                    );
                }
                if result.is_error.unwrap_or(false) {
                    Some(oxicode_tools::ToolResult::error(output))
                } else {
                    Some(oxicode_tools::ToolResult::success(output))
                }
            }
            Err(e) => Some(oxicode_tools::ToolResult::error(format!(
                "MCP tool call failed: {e}"
            ))),
        }
    }

    /// Run a tool after permission is granted (built-in registry first, then MCP).
    async fn run_tool(
        &self,
        tool_use_id: &str,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> ContentBlock {
        // Intercept `tool_search` and fulfil it from the live registry so the
        // tool itself does not need direct access to its sibling tools.
        if tool_name == "tool_search" {
            let query = input["query"].as_str().unwrap_or("");
            let max = input["max_results"]
                .as_u64()
                .map_or(5, |n| n as usize);
            let results = search_tools(&self.tool_registry, query, max);
            let content = serde_json::to_string_pretty(&results)
                .unwrap_or_else(|_| "[]".to_string());
            return ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content,
                is_error: false,
            };
        }

        let result = if self.tool_registry.get(tool_name).is_some() {
            self.tool_registry
                .execute(tool_name, input.clone(), &self.tool_context)
                .await
        } else if let Some(mcp_result) = self.try_mcp_tool(tool_name, input).await {
            Ok(mcp_result)
        } else {
            Err(OxiError::Tool {
                name: tool_name.to_string(),
                message: format!("Tool '{tool_name}' not found"),
            })
        };

        match result {
            Ok(result) => ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: result.content,
                is_error: result.is_error,
            },
            Err(e) => ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: format!("Tool error: {e}"),
                is_error: true,
            },
        }
    }

    /// Execute a single tool, checking permissions first.
    ///
    /// Fires `PreToolUse` / `PostToolUse` (or `PostToolUseFailure` on error)
    /// hooks following Claude Code spec.
    /// When `event_tx` is `Some`, permission Ask decisions are sent to the TUI
    /// via oneshot channel and the engine blocks until the user responds (30s timeout).
    /// When `None`, Ask decisions are auto-denied.
    pub(crate) async fn execute_tool(
        &self,
        tool_use_id: &str,
        tool_name: &str,
        input: &serde_json::Value,
        event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
    ) -> ContentBlock {
        // Fire PreToolUse hook — abort if hook says so.
        let before_data = serde_json::json!({
            "tool_name": tool_name,
            "tool_input": input,
        });
        let before_resp = fire_hook_with_events(
            &self.hook_manager,
            HookEvent::PreToolUse,
            before_data,
            event_tx,
        )
        .await;
        if let HookResponse::Abort { reason } = before_resp {
            return ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: format!("Hook aborted: {reason}"),
                is_error: true,
            };
        }

        let tool_level = self.tool_registry.get(tool_name).map_or(
            oxicode_permissions::pipeline::ToolPermissionLevel::System,
            |t| match t.permission_level() {
                PermissionLevel::ReadOnly => {
                    oxicode_permissions::pipeline::ToolPermissionLevel::ReadOnly
                }
                PermissionLevel::FileWrite => {
                    oxicode_permissions::pipeline::ToolPermissionLevel::FileWrite
                }
                PermissionLevel::ShellExec => {
                    oxicode_permissions::pipeline::ToolPermissionLevel::ShellExec
                }
                PermissionLevel::System => {
                    oxicode_permissions::pipeline::ToolPermissionLevel::System
                }
            },
        );

        let decision = self.permission_pipeline.check(tool_name, tool_level, input);

        let block = match decision {
            PermissionDecision::Allow => self.run_tool(tool_use_id, tool_name, input).await,
            PermissionDecision::Deny(reason) => {
                self.permission_pipeline.record_denial(tool_name, &reason);
                ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.to_string(),
                    content: format!("Permission denied: {reason}"),
                    is_error: true,
                }
            }
            PermissionDecision::Ask(prompt) => {
                self.handle_permission_ask(tool_use_id, tool_name, input, &prompt, event_tx)
                    .await
            }
        };

        // Emit tool result event.
        if let ContentBlock::ToolResult {
            ref tool_use_id,
            ref content,
            is_error,
        } = block
        {
            // Fire PostToolUse (or PostToolUseFailure on error) hook.
            let after_data = serde_json::json!({
                "tool_name": tool_name,
                "tool_input": input,
                "tool_response": content,
                "is_error": is_error,
            });
            let after_event = if is_error {
                HookEvent::PostToolUseFailure
            } else {
                HookEvent::PostToolUse
            };
            fire_hook_with_events(&self.hook_manager, after_event, after_data, event_tx).await;

            emit(
                event_tx,
                TurnEvent::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error,
                },
            )
            .await;
        }

        block
    }

    /// Handle a PermissionDecision::Ask — send to TUI if available, else auto-deny.
    async fn handle_permission_ask(
        &self,
        tool_use_id: &str,
        tool_name: &str,
        input: &serde_json::Value,
        prompt: &str,
        event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
    ) -> ContentBlock {
        let Some(tx) = event_tx else {
            tracing::info!("Permission ask (auto-deny, no event channel): {}", prompt);
            return ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: format!("Permission required: {prompt}"),
                is_error: true,
            };
        };

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let input_summary = summarize_tool_input(tool_name, input);
        let _ = tx
            .send(TurnEvent::PermissionAsk {
                tool_name: tool_name.to_string(),
                input_summary,
                prompt: prompt.to_string(),
                reply_tx,
            })
            .await;

        // Wait with 30s timeout to prevent hanging on abandoned session.
        match tokio::time::timeout(std::time::Duration::from_secs(30), reply_rx).await {
            Ok(Ok(PermissionResponse::AllowOnce)) => {
                self.run_tool(tool_use_id, tool_name, input).await
            }
            Ok(Ok(PermissionResponse::AlwaysAllow)) => {
                self.permission_pipeline.add_session_allow(tool_name);
                self.run_tool(tool_use_id, tool_name, input).await
            }
            Ok(Ok(PermissionResponse::Deny)) => {
                self.permission_pipeline.record_denial(tool_name, prompt);
                ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.to_string(),
                    content: format!("Permission denied by user: {prompt}"),
                    is_error: true,
                }
            }
            Ok(Ok(PermissionResponse::AlwaysDeny)) => {
                self.permission_pipeline.add_session_deny(tool_name);
                self.permission_pipeline.record_denial(tool_name, prompt);
                ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.to_string(),
                    content: format!("Permission denied by user: {prompt}"),
                    is_error: true,
                }
            }
            Ok(Err(_)) => ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: "Permission dialog dismissed".to_string(),
                is_error: true,
            },
            Err(_) => ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: "Permission request timed out (30s)".to_string(),
                is_error: true,
            },
        }
    }
}

/// Summarize tool input for display in permission dialogs.
fn summarize_tool_input(tool_name: &str, input: &serde_json::Value) -> String {
    let summary = match tool_name {
        "bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(String::from),
        "file_read" | "file_write" | "file_edit" => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(String::from),
        "grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            Some(format!("{pattern} in {path}"))
        }
        "glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => None,
    };

    let s = summary
        .unwrap_or_else(|| serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string()));

    // Truncate to 80 chars (char-safe).
    if let Some((idx, _)) = s.char_indices().nth(80) {
        format!("{}...", &s[..idx])
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- summarize_tool_input tests --

    #[test]
    fn test_summarize_bash() {
        let input = serde_json::json!({"command": "cargo test"});
        let result = summarize_tool_input("bash", &input);
        assert_eq!(result, "cargo test");
    }

    #[test]
    fn test_summarize_file_read() {
        let input = serde_json::json!({"file_path": "/tmp/test.rs"});
        let result = summarize_tool_input("file_read", &input);
        assert_eq!(result, "/tmp/test.rs");
    }

    #[test]
    fn test_summarize_file_write() {
        let input = serde_json::json!({"file_path": "/tmp/out.txt"});
        let result = summarize_tool_input("file_write", &input);
        assert_eq!(result, "/tmp/out.txt");
    }

    #[test]
    fn test_summarize_file_edit() {
        let input = serde_json::json!({"file_path": "/src/main.rs"});
        let result = summarize_tool_input("file_edit", &input);
        assert_eq!(result, "/src/main.rs");
    }

    #[test]
    fn test_summarize_grep() {
        let input = serde_json::json!({"pattern": "TODO", "path": "src/"});
        let result = summarize_tool_input("grep", &input);
        assert_eq!(result, "TODO in src/");
    }

    #[test]
    fn test_summarize_grep_defaults() {
        let input = serde_json::json!({});
        let result = summarize_tool_input("grep", &input);
        assert_eq!(result, "? in .");
    }

    #[test]
    fn test_summarize_glob() {
        let input = serde_json::json!({"pattern": "**/*.rs"});
        let result = summarize_tool_input("glob", &input);
        assert_eq!(result, "**/*.rs");
    }

    #[test]
    fn test_summarize_unknown_tool() {
        let input = serde_json::json!({"url": "https://example.com"});
        let result = summarize_tool_input("web_fetch", &input);
        // web_fetch is not a recognized tool, so falls through to generic serde_json output.
        assert!(result.contains("url"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn test_summarize_unknown_empty_input() {
        let input = serde_json::json!({});
        let result = summarize_tool_input("unknown_tool", &input);
        assert_eq!(result, "{}");
    }

    #[test]
    fn test_summarize_truncates_long_input() {
        let long_cmd = "a".repeat(100);
        let input = serde_json::json!({"command": long_cmd});
        let result = summarize_tool_input("bash", &input);
        assert!(result.len() < 100);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_summarize_bash_missing_command() {
        let input = serde_json::json!({"timeout": 30});
        let result = summarize_tool_input("bash", &input);
        // Falls through to serde_json::to_string since "command" key missing.
        assert!(result.contains("timeout"));
    }

    #[test]
    fn test_summarize_non_object_input() {
        let input = serde_json::json!("just a string");
        let result = summarize_tool_input("unknown", &input);
        assert!(result.contains("just a string"));
    }
}
