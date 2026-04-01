pub mod path_utils;
pub mod registry;
pub mod tool_trait;

// Tool implementations
pub mod bash;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob_tool;
pub mod grep_tool;

// Secondary tools
pub mod ask_user;
pub mod config_tool;
pub mod notebook_edit;
pub mod send_message;
pub mod stub_tools;
pub mod tool_search;

// Re-exports
pub use registry::ToolRegistry;
pub use tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Create a registry pre-loaded with all built-in tools.
pub fn default_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(file_read::FileReadTool));
    reg.register(Box::new(file_write::FileWriteTool));
    reg.register(Box::new(file_edit::FileEditTool));
    reg.register(Box::new(glob_tool::GlobTool));
    reg.register(Box::new(grep_tool::GrepTool));
    reg.register(Box::new(bash::BashTool));
    reg.register(Box::new(notebook_edit::NotebookEditTool));
    reg.register(Box::new(ask_user::AskUserTool));
    reg.register(Box::new(send_message::SendMessageTool));
    reg.register(Box::new(config_tool::ConfigTool));
    reg.register(Box::new(tool_search::ToolSearchTool));
    reg.register(Box::new(stub_tools::AgentTool));
    reg.register(Box::new(stub_tools::McpTool));
    reg.register(Box::new(stub_tools::WorktreeTool));
    reg
}
