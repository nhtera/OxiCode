/// oxicode-agents — subagent spawning, coordinator mode, messaging, team management, and fork mode.
pub mod communication;
pub mod coordinator;
pub mod fork;
pub mod spawner;
pub mod team;

// Key type re-exports for convenience.
pub use communication::{AgentMessage, MessageBus};
pub use coordinator::{
    filter_tools, is_coordinator_tool, AgentInfo, AgentStatus, CoordinatorMode, CoordinatorState,
    COORDINATOR_TOOLS,
};
pub use fork::{run_fork_agent, ForkConfig, ForkResult};
pub use spawner::{spawn_agent, spawn_agent_handle, AgentConfig, AgentHandle, AgentResult};
pub use team::{Team, TeamManager};
