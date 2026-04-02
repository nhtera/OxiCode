/// Team management — groups of named agents sharing a message bus.
use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, info};

use oxicode_common::{OxiError, OxiResult};

use crate::communication::MessageBus;
use crate::coordinator::CoordinatorState;
use crate::spawner::{spawn_agent_handle, AgentConfig, AgentHandle};

/// A named group of agents that share a `MessageBus`.
pub struct Team {
    pub name: String,
    pub agents: HashMap<String, AgentHandle>,
    pub bus: Arc<MessageBus>,
    pub coordinator: Option<CoordinatorState>,
}

impl std::fmt::Debug for Team {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Team")
            .field("name", &self.name)
            .field("agent_count", &self.agents.len())
            .finish_non_exhaustive()
    }
}

impl Team {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            agents: HashMap::new(),
            bus: Arc::new(MessageBus::new()),
            coordinator: None,
        }
    }
}

/// Top-level manager for all teams and their agents.
#[derive(Debug, Default)]
pub struct TeamManager {
    teams: HashMap<String, Team>,
}

impl TeamManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new empty team. Errors if the name already exists.
    pub fn create_team(&mut self, name: &str) -> OxiResult<()> {
        if self.teams.contains_key(name) {
            return Err(OxiError::Other(format!("team '{name}' already exists")));
        }
        info!(team = %name, "team created");
        self.teams.insert(name.to_string(), Team::new(name));
        Ok(())
    }

    /// Delete a team and all its agents. Errors if the team does not exist.
    pub fn delete_team(&mut self, name: &str) -> OxiResult<()> {
        self.teams
            .remove(name)
            .ok_or_else(|| OxiError::Other(format!("team '{name}' not found")))?;
        info!(team = %name, "team deleted");
        Ok(())
    }

    /// Immutable reference to a team.
    pub fn get_team(&self, name: &str) -> Option<&Team> {
        self.teams.get(name)
    }

    /// Names of all registered teams.
    pub fn list_teams(&self) -> Vec<String> {
        self.teams.keys().cloned().collect()
    }

    /// Spawn a new agent inside `team_name`, returning the agent id.
    pub fn spawn_in_team(&mut self, team_name: &str, config: AgentConfig) -> OxiResult<String> {
        let team = self
            .teams
            .get_mut(team_name)
            .ok_or_else(|| OxiError::Other(format!("team '{team_name}' not found")))?;

        let handle = spawn_agent_handle(&config)?;
        let agent_id = handle.id.clone();
        let agent_name = config.name.clone();

        debug!(team = %team_name, agent = %agent_name, id = %agent_id, "agent spawned in team");
        team.agents.insert(agent_name, handle);
        Ok(agent_id)
    }

    /// Send a message to a named agent within a team via the team bus.
    pub async fn send_to_agent(
        &self,
        team_name: &str,
        agent_name: &str,
        content: &str,
    ) -> OxiResult<()> {
        let team = self
            .teams
            .get(team_name)
            .ok_or_else(|| OxiError::Other(format!("team '{team_name}' not found")))?;

        if !team.agents.contains_key(agent_name) {
            return Err(OxiError::Other(format!(
                "agent '{agent_name}' not found in team '{team_name}'"
            )));
        }

        let msg = crate::communication::AgentMessage::new("coordinator", agent_name, content);
        team.bus.send(msg).await;
        debug!(team = %team_name, agent = %agent_name, "message sent to agent");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawner::AgentConfig;
    use std::path::PathBuf;
    use std::time::Duration;

    #[allow(dead_code)]
    fn dummy_config(name: &str) -> AgentConfig {
        AgentConfig {
            name: name.to_string(),
            prompt: "test".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            working_dir: PathBuf::from("/tmp"),
            permission_mode: "default".to_string(),
            timeout: Duration::from_secs(5),
            inherit_env: false,
        }
    }

    #[test]
    fn test_create_and_list_teams() {
        let mut mgr = TeamManager::new();
        mgr.create_team("alpha").unwrap();
        mgr.create_team("beta").unwrap();

        let mut teams = mgr.list_teams();
        teams.sort();
        assert_eq!(teams, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_create_duplicate_team_errors() {
        let mut mgr = TeamManager::new();
        mgr.create_team("dup").unwrap();
        assert!(mgr.create_team("dup").is_err());
    }

    #[test]
    fn test_delete_team() {
        let mut mgr = TeamManager::new();
        mgr.create_team("to-delete").unwrap();
        mgr.delete_team("to-delete").unwrap();
        assert!(mgr.get_team("to-delete").is_none());
    }

    #[test]
    fn test_delete_nonexistent_team_errors() {
        let mut mgr = TeamManager::new();
        assert!(mgr.delete_team("ghost").is_err());
    }

    #[test]
    fn test_send_to_missing_team_errors() {
        let mgr = TeamManager::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(mgr.send_to_agent("no-team", "nobody", "hi"));
        assert!(result.is_err());
    }
}
