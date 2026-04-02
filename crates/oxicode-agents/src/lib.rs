/// oxicode-agents — subagent spawning, coordinator mode, messaging, team management, and fork mode.
pub mod communication;
pub mod coordinator;
pub mod fork;
pub mod spawner;
pub mod team;

// Key type re-exports for convenience.
pub use communication::{AgentMessage, MessageBus};
pub use coordinator::{
    AgentInfo, AgentStatus, CoordinatorMode, CoordinatorState, COORDINATOR_TOOLS,
    filter_tools, is_coordinator_tool,
};
pub use spawner::{AgentConfig, AgentHandle, AgentResult, spawn_agent, spawn_agent_handle};
pub use fork::{ForkConfig, ForkResult, run_fork_agent};
pub use team::{Team, TeamManager};
