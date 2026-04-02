pub mod config;
pub mod manager;
pub mod protocol;
pub mod sse_transport;
pub mod stdio_transport;
pub mod tool_adapter;
pub mod websocket_transport;

pub use config::{McpConfig, McpServerConfig};
pub use manager::McpServerManager;
pub use protocol::{McpContent, McpPrompt, McpResource, McpToolDef, McpToolResult};
pub use tool_adapter::{mcp_tool_to_schema, mcp_tools_to_schemas};
