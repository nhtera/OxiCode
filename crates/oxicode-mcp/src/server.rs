//! MCP Server mode: expose OxiCode tools to external MCP clients via rmcp.
//!
//! Uses rmcp's `ServerHandler` trait for automatic JSON-RPC handling.
//! Runs over stdio — spawned as subprocess by MCP clients.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServiceExt};

use oxicode_common::OxiResult;

/// Async tool handler function signature for dynamic tool dispatch.
pub type AsyncToolHandler = Arc<
    dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = CallToolResult> + Send>> + Send + Sync,
>;

/// Tool registration entry for the MCP server.
struct RegisteredTool {
    def: Tool,
    handler: AsyncToolHandler,
}

/// MCP server exposing OxiCode tools over stdio.
///
/// Constructed via `OxiMcpServerBuilder`, then passed to `run_mcp_server()`.
#[derive(Clone)]
pub struct OxiMcpServer {
    tools: Arc<HashMap<String, RegisteredTool>>,
}

impl OxiMcpServer {
    /// Create an empty server (no tools registered).
    pub fn new() -> Self {
        Self {
            tools: Arc::new(HashMap::new()),
        }
    }
}

impl Default for OxiMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing an `OxiMcpServer` with registered tools.
pub struct OxiMcpServerBuilder {
    tools: HashMap<String, RegisteredTool>,
}

impl OxiMcpServerBuilder {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool with an async handler.
    pub fn add_tool(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: AsyncToolHandler,
    ) -> Self {
        let name = name.into();
        let schema: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(input_schema).unwrap_or_default();
        let tool_def = Tool::new(name.clone(), description.into(), Arc::new(schema));
        self.tools.insert(
            name,
            RegisteredTool {
                def: tool_def,
                handler,
            },
        );
        self
    }

    /// Build the server.
    pub fn build(self) -> OxiMcpServer {
        OxiMcpServer {
            tools: Arc::new(self.tools),
        }
    }
}

impl Default for OxiMcpServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::manual_async_fn)]
impl ServerHandler for OxiMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("OxiCode MCP server — exposes coding tools".to_string())
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            let tools: Vec<Tool> = self.tools.values().map(|t| t.def.clone()).collect();
            Ok(ListToolsResult::with_all_items(tools))
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            let tool = self.tools.get(request.name.as_ref()).ok_or_else(|| {
                McpError::invalid_params(format!("Unknown tool: {}", request.name), None)
            })?;

            let args = request
                .arguments
                .map_or(serde_json::Value::Null, serde_json::Value::Object);

            let result = (tool.handler)(args).await;
            Ok(result)
        }
    }
}

/// Run OxiCode as an MCP server over stdio.
///
/// Blocks until the client disconnects.
pub async fn run_mcp_server(server: OxiMcpServer) -> OxiResult<()> {
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| oxicode_common::OxiError::Other(format!("Failed to start MCP server: {e}")))?;

    service
        .waiting()
        .await
        .map_err(|e| oxicode_common::OxiError::Other(format!("MCP server error: {e}")))?;

    Ok(())
}
