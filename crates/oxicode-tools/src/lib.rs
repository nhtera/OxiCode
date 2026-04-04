pub mod file_state_tracker;
pub mod path_utils;
pub mod registry;
pub mod tool_trait;

// Tool implementations
pub mod bash;
pub mod bash_background;
pub mod bash_result_mapper;
pub mod bash_sandbox;
pub mod bash_security;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob_tool;
pub mod grep_tool;

// Secondary tools
pub mod agent_tool;
pub mod ask_user;
pub mod config_tool;
pub mod notebook_edit;
pub mod send_message;
pub mod tool_search;

// Task tools
pub mod task_tools;

// Web tools
pub mod web_fetch;
pub mod web_search;

// MCP resource + Skill tools
pub mod mcp_resource_tools;
pub mod skill_tool;

// Phase 5: New tools
pub mod brief;
pub mod cron;
pub mod plan_mode;
pub mod remote_trigger;
pub mod sleep;
pub mod structured_output;
pub mod worktree;

// Phase 3 gap-closure: Missing tools
pub mod lsp_tool;
pub mod mcp_auth;
pub mod powershell;
pub mod repl_tool;
pub mod suggest_background_pr;
pub mod synthetic_output;
pub mod team_tools;
pub mod todo_write;
pub mod verify_plan_execution;
pub mod workflow_tool;

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
    reg.register(Box::new(agent_tool::AgentTool));

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

    // MCP resource tools
    reg.register(Box::new(mcp_resource_tools::ListMcpResourcesTool));
    reg.register(Box::new(mcp_resource_tools::ReadMcpResourceTool));

    // Skill tool
    reg.register(Box::new(skill_tool::SkillTool));

    // Phase 3 gap-closure: Missing tools
    reg.register(Box::new(todo_write::TodoWriteTool));
    reg.register(Box::new(team_tools::TeamCreateTool));
    reg.register(Box::new(team_tools::TeamDeleteTool));
    reg.register(Box::new(lsp_tool::LspTool));
    reg.register(Box::new(powershell::PowerShellTool));
    reg.register(Box::new(repl_tool::ReplTool));
    reg.register(Box::new(mcp_auth::McpAuthTool));
    reg.register(Box::new(suggest_background_pr::SuggestBackgroundPrTool));
    reg.register(Box::new(synthetic_output::SyntheticOutputTool));
    reg.register(Box::new(verify_plan_execution::VerifyPlanExecutionTool));
    reg.register(Box::new(workflow_tool::WorkflowTool));

    reg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_tools() {
        let reg = default_registry();
        assert!(
            reg.len() > 30,
            "Should have at least 30 tools, got {}",
            reg.len()
        );
    }

    #[test]
    fn default_registry_has_core_tools() {
        let reg = default_registry();
        let core_tools = [
            "file_read",
            "file_write",
            "file_edit",
            "glob",
            "grep",
            "bash",
            "ask_user",
            "tool_search",
            "web_fetch",
            "web_search",
        ];
        for name in core_tools {
            assert!(reg.get(name).is_some(), "Missing core tool: {name}");
        }
    }

    #[test]
    fn default_registry_schemas_not_empty() {
        let reg = default_registry();
        let schemas = reg.schemas_json();
        assert!(!schemas.is_empty());
        // Every schema should have name, description, input_schema.
        for s in &schemas {
            assert!(s["name"].is_string(), "Schema missing name");
            assert!(s["description"].is_string(), "Schema missing description");
            assert!(s["input_schema"].is_object(), "Schema missing input_schema");
        }
    }

    #[test]
    fn default_registry_no_duplicate_names() {
        let reg = default_registry();
        let names = reg.names();
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(
            names.len(),
            unique.len(),
            "Duplicate tool names in registry"
        );
    }
}
