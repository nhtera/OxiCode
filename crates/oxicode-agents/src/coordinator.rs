/// Coordinator mode — restricts tool access and tracks active subagents.
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use tracing::{debug, info};

use crate::spawner::AgentHandle;

/// Tools available exclusively in coordinator mode.
pub const COORDINATOR_TOOLS: &[&str] = &["team_create", "team_delete", "send_message", "output"];

/// Returns true if `name` is a valid coordinator tool.
pub fn is_coordinator_tool(name: &str) -> bool {
    COORDINATOR_TOOLS.contains(&name)
}

/// Filters `tool_names`, returning only those allowed in coordinator mode.
pub fn filter_tools(tool_names: &[String]) -> Vec<String> {
    tool_names
        .iter()
        .filter(|n| is_coordinator_tool(n.as_str()))
        .cloned()
        .collect()
}

/// Lifecycle status of a managed subagent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

/// Lightweight snapshot of a managed agent (no handle ownership).
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub id: String,
    pub status: AgentStatus,
    pub started_at: DateTime<Utc>,
}

/// Entry stored per agent: the live handle plus metadata.
struct AgentEntry {
    handle: AgentHandle,
    info: AgentInfo,
}

/// Tracks active subagents spawned by coordinator mode.
pub struct CoordinatorState {
    agents: HashMap<String, AgentEntry>,
}

impl std::fmt::Debug for CoordinatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoordinatorState")
            .field("agent_count", &self.agents.len())
            .finish()
    }
}

impl CoordinatorState {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Register a new agent handle under `name`.
    pub fn add_agent(&mut self, name: impl Into<String>, handle: AgentHandle) {
        let name = name.into();
        let info = AgentInfo {
            name: name.clone(),
            id: handle.id.clone(),
            status: AgentStatus::Running,
            started_at: Utc::now(),
        };
        debug!(agent = %name, id = %info.id, "coordinator registered agent");
        self.agents.insert(name, AgentEntry { handle, info });
    }

    /// Remove and return the handle for `name`, if present.
    pub fn remove_agent(&mut self, name: &str) -> Option<AgentHandle> {
        self.agents.remove(name).map(|e| {
            info!(agent = %name, "coordinator removed agent");
            e.handle
        })
    }

    /// Immutable borrow of the handle for `name`.
    pub fn get_agent(&self, name: &str) -> Option<&AgentHandle> {
        self.agents.get(name).map(|e| &e.handle)
    }

    /// Mutable borrow of the handle for `name`.
    pub fn get_agent_mut(&mut self, name: &str) -> Option<&mut AgentHandle> {
        self.agents.get_mut(name).map(|e| &mut e.handle)
    }

    /// Snapshot of all agent metadata (no handle access).
    pub fn list_agents(&self) -> Vec<AgentInfo> {
        self.agents.values().map(|e| e.info.clone()).collect()
    }

    /// Update the status of an agent by name.
    pub fn set_status(&mut self, name: &str, status: AgentStatus) {
        if let Some(entry) = self.agents.get_mut(name) {
            entry.info.status = status;
        }
    }
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self::new()
    }
}

/// Marker struct representing coordinator mode activation context.
#[derive(Debug, Default)]
pub struct CoordinatorMode {
    pub state: CoordinatorState,
}

impl CoordinatorMode {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_coordinator_tool() {
        assert!(is_coordinator_tool("team_create"));
        assert!(is_coordinator_tool("send_message"));
        assert!(!is_coordinator_tool("bash"));
        assert!(!is_coordinator_tool("read_file"));
    }

    #[test]
    fn test_filter_tools() {
        let names = vec![
            "bash".to_string(),
            "team_create".to_string(),
            "output".to_string(),
            "read_file".to_string(),
        ];
        let filtered = filter_tools(&names);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"team_create".to_string()));
        assert!(filtered.contains(&"output".to_string()));
    }

    #[test]
    fn test_coordinator_state_list_empty() {
        let state = CoordinatorState::new();
        assert!(state.list_agents().is_empty());
    }

    #[test]
    fn test_coordinator_tools_const() {
        assert_eq!(COORDINATOR_TOOLS.len(), 4);
    }
}
