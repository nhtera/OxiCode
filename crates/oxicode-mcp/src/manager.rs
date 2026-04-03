//! McpServerManager: lifecycle management for MCP server connections.
//!
//! Starts configured servers, discovers tools/resources/prompts, handles shutdown.

use std::collections::HashMap;

use oxicode_common::{OxiError, OxiResult};

use crate::config::{McpConfig, McpServerConfig, McpTransportType};
use crate::protocol::{McpToolDef, McpToolResult, ServerCapabilities};
use crate::sse_transport::SseTransport;
use crate::stdio_transport::StdioTransport;
use crate::websocket_transport::WebSocketTransport;

/// Active transport for a running MCP server.
enum ActiveTransport {
    Stdio(StdioTransport),
    Sse(SseTransport),
    WebSocket(WebSocketTransport),
}

impl ActiveTransport {
    async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> OxiResult<serde_json::Value> {
        match self {
            Self::Stdio(t) => t.request(method, params).await,
            Self::Sse(t) => t.request(method, params).await,
            Self::WebSocket(t) => t.request(method, params).await,
        }
    }
}

/// A running MCP server with its discovered capabilities.
struct RunningServer {
    transport: ActiveTransport,
    #[allow(dead_code)]
    capabilities: ServerCapabilities,
    tools: Vec<McpToolDef>,
    /// Original config for channel permission filtering.
    config: McpServerConfig,
}

/// Manages multiple MCP server connections.
pub struct McpServerManager {
    servers: HashMap<String, RunningServer>,
}

impl McpServerManager {
    /// Create an empty manager (no servers started yet).
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    /// Start all enabled servers from config.
    pub async fn start_from_config(&mut self, config: &McpConfig) -> Vec<String> {
        let mut started = Vec::new();

        for (name, server_config) in config.enabled_servers() {
            let transport = match &server_config.transport {
                McpTransportType::Stdio => {
                    let Some(command) = &server_config.command else {
                        tracing::warn!("MCP server '{name}' has no command");
                        continue;
                    };
                    let env: Vec<(String, String)> = server_config
                        .env
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();

                    match StdioTransport::spawn(command, &server_config.args, &env) {
                        Ok(t) => ActiveTransport::Stdio(t),
                        Err(e) => {
                            tracing::error!("Failed to start MCP server '{name}': {e}");
                            continue;
                        }
                    }
                }
                McpTransportType::Sse => {
                    let Some(url) = &server_config.url else {
                        tracing::warn!("MCP server '{name}' has no URL");
                        continue;
                    };
                    ActiveTransport::Sse(SseTransport::new(url))
                }
                McpTransportType::WebSocket => {
                    let Some(url) = &server_config.url else {
                        tracing::warn!("MCP server '{name}' has no WebSocket URL");
                        continue;
                    };
                    match WebSocketTransport::connect(url).await {
                        Ok(t) => ActiveTransport::WebSocket(t),
                        Err(e) => {
                            tracing::error!("Failed to connect WebSocket MCP server '{name}': {e}");
                            continue;
                        }
                    }
                }
            };

            // Initialize the server.
            match self.initialize_server(name, transport, server_config.clone()).await {
                Ok(()) => started.push(name.to_string()),
                Err(e) => tracing::error!("Failed to initialize MCP server '{name}': {e}"),
            }
        }

        started
    }

    /// Initialize a server: send initialize, list tools.
    async fn initialize_server(
        &mut self,
        name: &str,
        transport: ActiveTransport,
        server_config: McpServerConfig,
    ) -> OxiResult<()> {
        let init_params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "oxicode",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });

        let result = transport.request("initialize", Some(init_params)).await?;

        let capabilities: ServerCapabilities =
            serde_json::from_value(result.get("capabilities").cloned().unwrap_or_default())
                .unwrap_or_default();

        // Send initialized notification (stdio only).
        if let ActiveTransport::Stdio(ref stdio) = transport {
            let _ = stdio.notify("notifications/initialized", None).await;
        }

        // Discover tools.
        let tools = if capabilities.tools.is_some() {
            match transport.request("tools/list", None).await {
                Ok(result) => {
                    let tools_array = result.get("tools").cloned().unwrap_or_default();
                    serde_json::from_value::<Vec<McpToolDef>>(tools_array).unwrap_or_default()
                }
                Err(e) => {
                    tracing::warn!("Failed to list tools for MCP server '{name}': {e}");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        tracing::info!("MCP server '{name}' initialized with {} tools", tools.len());

        self.servers.insert(
            name.to_string(),
            RunningServer {
                transport,
                capabilities,
                tools,
                config: server_config,
            },
        );

        Ok(())
    }

    /// Get all discovered tools across all servers, prefixed with server name.
    /// Tools are filtered by per-server channel permissions (allow/block lists).
    pub fn all_tools(&self) -> Vec<(String, &McpToolDef)> {
        let mut tools = Vec::new();
        for (server_name, server) in &self.servers {
            for tool in &server.tools {
                // Apply channel permission filtering.
                if !server.config.is_tool_allowed(&tool.name) {
                    continue;
                }
                // Prefix tool name: "server_name__tool_name"
                let prefixed = format!("{server_name}__{}", tool.name);
                tools.push((prefixed, tool));
            }
        }
        tools
    }

    /// Call a tool on a specific server.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> OxiResult<McpToolResult> {
        let server = self
            .servers
            .get(server_name)
            .ok_or_else(|| OxiError::Other(format!("MCP server '{server_name}' not found")))?;

        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        });

        let result = server.transport.request("tools/call", Some(params)).await?;
        serde_json::from_value(result)
            .map_err(|e| OxiError::Other(format!("Failed to parse tool result: {e}")))
    }

    /// Resolve a prefixed tool name ("server__tool") to (server_name, tool_name).
    pub fn resolve_tool_name(prefixed: &str) -> Option<(&str, &str)> {
        prefixed.split_once("__")
    }

    /// List connected server names.
    pub fn server_names(&self) -> Vec<&str> {
        self.servers.keys().map(String::as_str).collect()
    }

    /// Number of connected servers.
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    /// List resources from all connected servers that advertise resource capability.
    ///
    /// Returns `(server_name, Vec<McpResource>)` pairs.
    pub async fn list_resources(&self) -> Vec<(String, Vec<crate::protocol::McpResource>)> {
        let mut results = Vec::new();
        for (name, server) in &self.servers {
            if server.capabilities.resources.is_none() {
                continue;
            }
            match server.transport.request("resources/list", None).await {
                Ok(result) => {
                    let arr = result.get("resources").cloned().unwrap_or_default();
                    let resources: Vec<crate::protocol::McpResource> =
                        serde_json::from_value(arr).unwrap_or_default();
                    if !resources.is_empty() {
                        results.push((name.clone(), resources));
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to list resources from MCP server '{name}': {e}");
                }
            }
        }
        results
    }

    /// Read a resource by URI from a specific server.
    pub async fn read_resource(
        &self,
        server_name: &str,
        uri: &str,
    ) -> OxiResult<Vec<crate::protocol::McpContent>> {
        let server = self
            .servers
            .get(server_name)
            .ok_or_else(|| OxiError::Other(format!("MCP server '{server_name}' not found")))?;

        let params = serde_json::json!({ "uri": uri });
        let result = server
            .transport
            .request("resources/read", Some(params))
            .await?;

        let contents_arr = result.get("contents").cloned().unwrap_or_default();
        serde_json::from_value(contents_arr)
            .map_err(|e| OxiError::Other(format!("Failed to parse resource contents: {e}")))
    }

    /// Shut down all servers gracefully.
    pub async fn shutdown_all(&self) {
        for (name, server) in &self.servers {
            tracing::info!("Shutting down MCP server '{name}'");
            match &server.transport {
                ActiveTransport::Stdio(stdio) => stdio.shutdown().await,
                ActiveTransport::WebSocket(ws) => ws.close().await,
                ActiveTransport::Sse(_) => {} // SSE has no shutdown protocol.
            }
        }
    }
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}
