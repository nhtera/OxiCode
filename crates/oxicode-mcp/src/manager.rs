//! McpServerManager: lifecycle management for MCP server connections.
//!
//! Uses the rmcp crate for protocol handling — no manual JSON-RPC.
//! Manages multiple MCP server connections, tool discovery, and execution.

use std::collections::HashMap;

use oxicode_common::{OxiError, OxiResult};
use rmcp::model::{CallToolRequestParams, CallToolResult, GetPromptRequestParams, Prompt, Tool};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};

use crate::config::{McpConfig, McpServerConfig, McpTransportType};

/// Type-erased rmcp client for storing heterogeneous transports in a HashMap.
type DynClient = RunningService<RoleClient, Box<dyn rmcp::service::DynService<RoleClient>>>;

/// A running MCP server client with cached tool list.
struct ManagedClient {
    client: DynClient,
    tools: Vec<Tool>,
    /// Original config for channel permission filtering.
    config: McpServerConfig,
}

/// Manages multiple MCP server connections via rmcp.
///
/// Handles lifecycle (start/shutdown), tool discovery, and tool execution.
/// All clients are type-erased via `DynService` for uniform storage.
pub struct McpServerManager {
    clients: HashMap<String, ManagedClient>,
}

impl McpServerManager {
    /// Create an empty manager (no servers started yet).
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    /// Start all enabled servers from config. Returns names of successfully started servers.
    pub async fn start_from_config(&mut self, config: &McpConfig) -> Vec<String> {
        let mut started = Vec::new();

        for (name, server_config) in config.enabled_servers() {
            match self.start_server(name, server_config).await {
                Ok(()) => started.push(name.to_string()),
                Err(e) => tracing::error!("Failed to start MCP server '{name}': {e}"),
            }
        }

        started
    }

    /// Start a single server: create transport, initialize via rmcp, discover tools.
    async fn start_server(&mut self, name: &str, config: &McpServerConfig) -> OxiResult<()> {
        let client: DynClient =
            match &config.transport {
                McpTransportType::Stdio => {
                    let command = config.command.as_deref().ok_or_else(|| {
                        OxiError::Other(format!("MCP server '{name}' has no command"))
                    })?;

                    let mut cmd = tokio::process::Command::new(command);
                    cmd.args(&config.args);
                    for (k, v) in &config.env {
                        cmd.env(k, v);
                    }

                    let transport = TokioChildProcess::new(cmd)
                        .map_err(|e| OxiError::Other(format!("Failed to spawn '{name}': {e}")))?;

                    ().into_dyn().serve(transport).await.map_err(|e| {
                        OxiError::Other(format!("Failed to initialize '{name}': {e}"))
                    })?
                }
                McpTransportType::Http | McpTransportType::Sse => {
                    // StreamableHttpClientTransport handles both SSE and Streamable HTTP.
                    let url = config.url.as_deref().ok_or_else(|| {
                        OxiError::Other(format!("MCP server '{name}' has no URL"))
                    })?;

                    let transport = rmcp::transport::StreamableHttpClientTransport::from_uri(url);

                    ().into_dyn().serve(transport).await.map_err(|e| {
                        OxiError::Other(format!("Failed to initialize '{name}': {e}"))
                    })?
                }
            };

        // Discover tools (rmcp handles capabilities check + auto-pagination).
        let tools = match client.list_all_tools().await {
            Ok(tools) => tools,
            Err(e) => {
                tracing::warn!("Failed to list tools for '{name}': {e}");
                Vec::new()
            }
        };

        tracing::info!("MCP server '{name}' initialized with {} tools", tools.len());

        self.clients.insert(
            name.to_string(),
            ManagedClient {
                client,
                tools,
                config: config.clone(),
            },
        );

        Ok(())
    }

    /// Get all discovered tools across all servers, prefixed with server name.
    /// Tools are filtered by per-server channel permissions (allow/block lists).
    pub fn all_tools(&self) -> Vec<(String, &Tool)> {
        let mut tools = Vec::new();
        for (server_name, managed) in &self.clients {
            for tool in &managed.tools {
                if !managed.config.is_tool_allowed(&tool.name) {
                    continue;
                }
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
    ) -> OxiResult<CallToolResult> {
        let managed = self
            .clients
            .get(server_name)
            .ok_or_else(|| OxiError::Other(format!("MCP server '{server_name}' not found")))?;

        let args_map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(arguments)
                .map_err(|e| OxiError::Other(format!("Invalid tool arguments: {e}")))?;

        let params = CallToolRequestParams::new(tool_name.to_string()).with_arguments(args_map);

        managed
            .client
            .call_tool(params)
            .await
            .map_err(|e| OxiError::Other(format!("Tool call failed: {e}")))
    }

    /// Resolve a prefixed tool name ("server__tool") to (server_name, tool_name).
    pub fn resolve_tool_name(prefixed: &str) -> Option<(&str, &str)> {
        prefixed.split_once("__")
    }

    /// List connected server names.
    pub fn server_names(&self) -> Vec<&str> {
        self.clients.keys().map(String::as_str).collect()
    }

    /// Number of connected servers.
    pub fn server_count(&self) -> usize {
        self.clients.len()
    }

    /// List resources from all connected servers.
    pub async fn list_resources(&self) -> Vec<(String, Vec<rmcp::model::Resource>)> {
        let mut results = Vec::new();
        for (name, managed) in &self.clients {
            match managed.client.list_all_resources().await {
                Ok(resources) if !resources.is_empty() => {
                    results.push((name.clone(), resources));
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Failed to list resources from '{name}': {e}");
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
    ) -> OxiResult<rmcp::model::ReadResourceResult> {
        let managed = self
            .clients
            .get(server_name)
            .ok_or_else(|| OxiError::Other(format!("MCP server '{server_name}' not found")))?;

        let params = rmcp::model::ReadResourceRequestParams::new(uri);

        managed
            .client
            .read_resource(params)
            .await
            .map_err(|e| OxiError::Other(format!("Failed to read resource: {e}")))
    }

    /// List prompts from all connected servers.
    pub async fn list_prompts(&self) -> Vec<(String, Vec<Prompt>)> {
        let mut results = Vec::new();
        for (name, managed) in &self.clients {
            match managed.client.list_all_prompts().await {
                Ok(prompts) if !prompts.is_empty() => {
                    results.push((name.clone(), prompts));
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Failed to list prompts from '{name}': {e}");
                }
            }
        }
        results
    }

    /// Get a specific prompt with optional arguments from a server.
    pub async fn get_prompt(
        &self,
        server_name: &str,
        prompt_name: &str,
        arguments: Option<HashMap<String, String>>,
    ) -> OxiResult<rmcp::model::GetPromptResult> {
        let managed = self
            .clients
            .get(server_name)
            .ok_or_else(|| OxiError::Other(format!("MCP server '{server_name}' not found")))?;

        let mut params = GetPromptRequestParams::new(prompt_name);
        if let Some(args) = arguments {
            let map: serde_json::Map<String, serde_json::Value> = args
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect();
            params = params.with_arguments(map);
        }

        managed
            .client
            .get_prompt(params)
            .await
            .map_err(|e| OxiError::Other(format!("Failed to get prompt: {e}")))
    }

    /// Shut down all servers gracefully via cancellation tokens.
    ///
    /// Uses cancellation tokens so it works through `&self` (no need for mutable borrow).
    /// Preserves API compatibility with callers using `Arc<McpServerManager>`.
    #[allow(clippy::unused_async)]
    pub async fn shutdown_all(&self) {
        for (name, managed) in &self.clients {
            tracing::info!("Shutting down MCP server '{name}'");
            managed.client.cancellation_token().cancel();
        }
    }
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}
