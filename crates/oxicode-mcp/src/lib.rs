pub mod config;
pub mod doctor;
pub mod elicitation;
pub mod env_expansion;
pub mod manager;
pub mod oauth;
pub mod server;
pub mod types;

pub use config::{McpAuthConfig, McpConfig, McpServerConfig};
pub use doctor::{diagnose_all, diagnose_server, DiagResult, DiagStatus};
pub use elicitation::{ElicitationHandler, ElicitationRequest, ElicitationResponse};
pub use env_expansion::expand_env;
pub use manager::McpServerManager;
pub use oauth::McpOAuth;
pub use server::{run_mcp_server, OxiMcpServer, OxiMcpServerBuilder};
pub use types::{
    mcp_tool_to_schema, mcp_tools_to_schemas, McpContent, McpGetPromptResult, McpPrompt,
    McpPromptMessage, McpResource, McpResourceContents, McpRoot, McpToolDef, McpToolResult,
};
