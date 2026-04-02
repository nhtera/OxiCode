//! Tool dispatch: permission checks, built-in/MCP routing, and permission dialog integration.

use oxicode_common::{ContentBlock, OxiError, PermissionResponse};
use oxicode_permissions::PermissionDecision;
use oxicode_tools::PermissionLevel;

use crate::query_engine::QueryEngine;
use crate::turn_event::{emit, TurnEvent};

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
                    .filter_map(|c| c.as_text())
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
                if result.is_error {
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
            Ok(Ok(PermissionResponse::AllowOnce | PermissionResponse::AlwaysAllow)) => {
                self.run_tool(tool_use_id, tool_name, input).await
            }
            Ok(Ok(PermissionResponse::Deny | PermissionResponse::AlwaysDeny)) => {
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
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
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
