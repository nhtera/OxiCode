//! LSP tool: Language Server Protocol integration for code intelligence.
//!
//! Supports: goToDefinition, findReferences, hover, documentSymbol,
//! workspaceSymbol, goToImplementation. Communicates with language servers
//! via JSON-RPC over stdio.

use std::path::Path;

use async_trait::async_trait;
use oxicode_common::OxiResult;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Detect language server command from file extension.
fn detect_lsp_command(file_path: &str) -> Option<Vec<String>> {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => Some(vec!["rust-analyzer".into()]),
        "ts" | "tsx" | "js" | "jsx" => Some(vec![
            "typescript-language-server".into(),
            "--stdio".into(),
        ]),
        "py" => Some(vec!["pylsp".into()]),
        "go" => Some(vec!["gopls".into(), "serve".into()]),
        "java" => Some(vec!["jdtls".into()]),
        "c" | "cpp" | "h" | "hpp" => Some(vec!["clangd".into()]),
        _ => None,
    }
}

/// Map operation name to LSP method string.
fn lsp_method(operation: &str) -> Option<&'static str> {
    match operation {
        "goToDefinition" => Some("textDocument/definition"),
        "findReferences" => Some("textDocument/references"),
        "hover" => Some("textDocument/hover"),
        "documentSymbol" => Some("textDocument/documentSymbol"),
        "workspaceSymbol" => Some("workspace/symbol"),
        "goToImplementation" => Some("textDocument/implementation"),
        _ => None,
    }
}

/// Build LSP request params for a position-based operation.
fn position_params(file_uri: &str, line: u64, character: u64) -> serde_json::Value {
    json!({
        "textDocument": { "uri": file_uri },
        "position": { "line": line, "character": character }
    })
}

/// Encode a JSON-RPC message with Content-Length header.
fn encode_jsonrpc(msg: &serde_json::Value) -> String {
    let body = serde_json::to_string(msg).unwrap_or_default();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

pub struct LspTool;

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "LSP"
    }
    fn description(&self) -> &str {
        "Query language servers for code intelligence: definitions, references, hover info, symbols."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "enum": ["goToDefinition", "findReferences", "hover",
                                 "documentSymbol", "workspaceSymbol", "goToImplementation"],
                        "description": "The LSP operation to perform"
                    },
                    "filePath": {
                        "type": "string",
                        "description": "Absolute path to the file"
                    },
                    "line": {
                        "type": "integer",
                        "description": "1-based line number"
                    },
                    "character": {
                        "type": "integer",
                        "description": "1-based character offset"
                    }
                },
                "required": ["operation", "filePath"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(operation) = input.get("operation").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("operation is required"));
        };
        let Some(file_path) = input.get("filePath").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("filePath is required"));
        };

        let abs_path = if Path::new(file_path).is_absolute() {
            file_path.to_string()
        } else {
            ctx.working_dir.join(file_path).to_string_lossy().to_string()
        };

        if !Path::new(&abs_path).exists() {
            return Ok(ToolResult::error(format!("File not found: {abs_path}")));
        }

        let Some(method) = lsp_method(operation) else {
            return Ok(ToolResult::error(format!("Unknown operation: {operation}")));
        };

        let Some(cmd_parts) = detect_lsp_command(&abs_path) else {
            return Ok(ToolResult::error(format!(
                "No language server available for file type: {abs_path}"
            )));
        };

        let line = input.get("line").and_then(serde_json::Value::as_u64).unwrap_or(1).saturating_sub(1);
        let character = input.get("character").and_then(serde_json::Value::as_u64).unwrap_or(1).saturating_sub(1);

        run_lsp_query(operation, &abs_path, method, &cmd_parts, line, character, &input, ctx).await
    }
}

/// Execute the LSP server lifecycle: spawn → init → open doc → query → shutdown.
#[allow(clippy::too_many_arguments)]
async fn run_lsp_query(
    operation: &str,
    abs_path: &str,
    method: &str,
    cmd_parts: &[String],
    line: u64,
    character: u64,
    input: &serde_json::Value,
    ctx: &ToolContext,
) -> OxiResult<ToolResult> {
    let file_uri = format!("file://{abs_path}");
    let program = &cmd_parts[0];
    let args = &cmd_parts[1..];

    let mut child = match Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "Failed to start language server '{program}': {e}. Is it installed?"
            )));
        }
    };

    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");
    let mut reader = BufReader::new(stdout);

    // Initialize handshake
    let init_req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootUri": format!("file://{}", ctx.working_dir.display()),
            "capabilities": {}
        }
    });
    if let Err(e) = stdin.write_all(encode_jsonrpc(&init_req).as_bytes()).await {
        return Ok(ToolResult::error(format!("Failed to send init: {e}")));
    }
    if let Err(e) = read_jsonrpc_response(&mut reader).await {
        return Ok(ToolResult::error(format!("LSP init failed: {e}")));
    }

    let init_notif = json!({"jsonrpc": "2.0", "method": "initialized", "params": {}});
    let _ = stdin.write_all(encode_jsonrpc(&init_notif).as_bytes()).await;

    // Open document
    let file_content = tokio::fs::read_to_string(abs_path).await.unwrap_or_default();
    let open_notif = json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": &file_uri, "languageId": detect_language_id(abs_path),
            "version": 1, "text": file_content,
        }}
    });
    let _ = stdin.write_all(encode_jsonrpc(&open_notif).as_bytes()).await;

    // Build request params
    let params = match method {
        "workspace/symbol" => {
            json!({"query": input.get("query").and_then(|v| v.as_str()).unwrap_or("")})
        }
        "textDocument/documentSymbol" => json!({"textDocument": {"uri": &file_uri}}),
        "textDocument/references" => {
            let mut p = position_params(&file_uri, line, character);
            p["context"] = json!({"includeDeclaration": true});
            p
        }
        _ => position_params(&file_uri, line, character),
    };

    let req = json!({"jsonrpc": "2.0", "id": 2, "method": method, "params": params});
    if let Err(e) = stdin.write_all(encode_jsonrpc(&req).as_bytes()).await {
        return Ok(ToolResult::error(format!("Failed to send request: {e}")));
    }

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        read_jsonrpc_response(&mut reader),
    )
    .await;

    // Shutdown
    let shutdown = json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown", "params": null});
    let _ = stdin.write_all(encode_jsonrpc(&shutdown).as_bytes()).await;
    let exit_notif = json!({"jsonrpc": "2.0", "method": "exit", "params": null});
    let _ = stdin.write_all(encode_jsonrpc(&exit_notif).as_bytes()).await;
    drop(stdin);

    match response {
        Ok(Ok(resp)) => {
            let result_data = resp.get("result").cloned().unwrap_or(json!(null));
            let output = json!({
                "operation": operation, "filePath": abs_path, "result": result_data,
            });
            Ok(ToolResult::success(
                serde_json::to_string_pretty(&output).unwrap_or_default(),
            ))
        }
        Ok(Err(e)) => Ok(ToolResult::error(format!("LSP response error: {e}"))),
        Err(_) => Ok(ToolResult::error("LSP request timed out after 30s")),
    }
}

/// Maximum Content-Length we accept from an LSP server (10 MB).
const MAX_LSP_BODY: usize = 10 * 1024 * 1024;

/// Read a single JSON-RPC message from a Content-Length delimited stream.
async fn read_jsonrpc_message(
    reader: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<serde_json::Value, String> {
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await.map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
            content_length = len_str.trim().parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
        }
    }

    if content_length == 0 {
        return Err("No Content-Length header".into());
    }
    if content_length > MAX_LSP_BODY {
        return Err(format!("Content-Length {content_length} exceeds {MAX_LSP_BODY} byte limit"));
    }

    let mut body = vec![0u8; content_length];
    tokio::io::AsyncReadExt::read_exact(reader, &mut body)
        .await
        .map_err(|e| e.to_string())?;

    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

/// Read JSON-RPC response, skipping notifications (messages without "id").
async fn read_jsonrpc_response(
    reader: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<serde_json::Value, String> {
    // LSP servers send interleaved notifications; skip until we get a response.
    for _ in 0..50 {
        let msg = read_jsonrpc_message(reader).await?;
        if msg.get("id").is_some() {
            return Ok(msg);
        }
        // Notification — skip and read next message
    }
    Err("Gave up after 50 messages without a response".into())
}

/// Detect LSP language ID from file extension.
fn detect_language_id(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => "rust",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" => "javascript",
        "jsx" => "javascriptreact",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        _ => "plaintext",
    }
}
