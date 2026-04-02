//! Slash commands for inspecting agents, skills, and background tasks.
//!
//! Commands:
//!   /agent  — list active subagents and their status
//!   /skills — list active skills for this session
//!   /tasks  — list background tasks registered in the task manager

use super::{CommandContext, CommandOutput, SlashCommand};

// ── /agent ──────────────────────────────────────────────────────────────────

pub struct AgentCommand;

impl SlashCommand for AgentCommand {
    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "List active subagents and their status"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        if state.active_agents.is_empty() {
            return CommandOutput::Message("No active agents.".into());
        }
        let mut output = String::from("Active Agents:\n");
        for agent in &state.active_agents {
            output.push_str(&format!(
                "  {} [{}] — {}\n",
                agent.name, agent.status, agent.started_at
            ));
        }
        CommandOutput::Message(output)
    }
}

// ── /skills ──────────────────────────────────────────────────────────────────

pub struct SkillsCommand;

impl SlashCommand for SkillsCommand {
    fn name(&self) -> &str {
        "skills"
    }

    fn description(&self) -> &str {
        "List active skills for this session"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        if state.active_skills.is_empty() {
            return CommandOutput::Message(
                "No skills active. Skills activate automatically based on context.".into(),
            );
        }
        let mut output = String::from("Active Skills:\n");
        for skill in &state.active_skills {
            output.push_str(&format!("  - {skill}\n"));
        }
        CommandOutput::Message(output)
    }
}

// ── /tasks ───────────────────────────────────────────────────────────────────

pub struct TasksCommand;

impl SlashCommand for TasksCommand {
    fn name(&self) -> &str {
        "tasks"
    }

    fn description(&self) -> &str {
        "List background tasks registered in the task manager"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        if state.background_tasks.is_empty() {
            return CommandOutput::Message("No background tasks.".into());
        }
        let mut output = String::from("Background Tasks:\n");
        for task in &state.background_tasks {
            output.push_str(&format!(
                "  #{} [{}] {}: {}\n",
                task.id, task.status, task.task_type, task.command_preview
            ));
        }
        CommandOutput::Message(output)
    }
}
