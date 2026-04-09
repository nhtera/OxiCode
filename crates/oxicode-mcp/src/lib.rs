pub mod config;
pub mod doctor;
pub mod elicitation;
pub mod env_expansion;
pub mod manager;
pub mod oauth;
pub mod server;
pub mod types;

// ---------------------------------------------------------------------------
// Bridge debug modules (feature = "bridge_debug")
// ---------------------------------------------------------------------------

#[cfg(feature = "bridge_debug")]
pub mod bridge_debug_logger;
#[cfg(feature = "bridge_debug")]
pub mod bridge_diagnostics;
#[cfg(feature = "bridge_debug")]
pub mod bridge_event_tap;
#[cfg(feature = "bridge_debug")]
pub mod bridge_health_check;
#[cfg(feature = "bridge_debug")]
pub mod bridge_message_inspector;
#[cfg(feature = "bridge_debug")]
pub mod bridge_status_tracker;

// ---------------------------------------------------------------------------
// Bridge UI modules (always available)
// ---------------------------------------------------------------------------

pub mod bridge_ui_config_dialog;
pub mod bridge_ui_notification;
pub mod bridge_ui_permission_dialog;

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
