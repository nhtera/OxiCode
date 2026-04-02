pub mod file_state_tracker;
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

// Task tools
pub mod task_tools;

// Web tools
pub mod web_fetch;
pub mod web_search;

// Phase 5: New tools
pub mod brief;
pub mod cron;
pub mod plan_mode;
pub mod remote_trigger;
pub mod sleep;
pub mod structured_output;
pub mod worktree;

// Re-exports
pub use registry::ToolRegistry;
pub use tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Create a registry pre-loaded with all built-in tools.
pub fn default_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    // Core tools
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

    // Phase 5: Workflow tools
    reg.register(Box::new(plan_mode::EnterPlanModeTool));
    reg.register(Box::new(plan_mode::ExitPlanModeTool));
    reg.register(Box::new(worktree::EnterWorktreeTool));
    reg.register(Box::new(worktree::ExitWorktreeTool));

    // Phase 5: Agent + Dev tools
    reg.register(Box::new(sleep::SleepTool));
    reg.register(Box::new(remote_trigger::RemoteTriggerTool));
    reg.register(Box::new(brief::BriefTool));
    reg.register(Box::new(structured_output::StructuredOutputTool));
    reg.register(Box::new(cron::CronCreateTool));
    reg.register(Box::new(cron::CronDeleteTool));
    reg.register(Box::new(cron::CronListTool));

    // Task tools
    reg.register(Box::new(task_tools::TaskCreateTool));
    reg.register(Box::new(task_tools::TaskGetTool));
    reg.register(Box::new(task_tools::TaskListTool));
    reg.register(Box::new(task_tools::TaskUpdateTool));
    reg.register(Box::new(task_tools::TaskStopTool));
    reg.register(Box::new(task_tools::TaskOutputTool));

    // Web tools
    reg.register(Box::new(web_fetch::WebFetchTool));
    reg.register(Box::new(web_search::WebSearchTool));
    reg
}
