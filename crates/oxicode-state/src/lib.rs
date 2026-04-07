pub mod cost_tracker;

use oxicode_common::{FeatureFlags, Message, RateLimitInfo, Usage};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

/// Snapshot of a managed subagent, serialisation-friendly.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentEntry {
    pub name: String,
    /// "running" | "completed" | "failed" | "killed"
    pub status: String,
    /// ISO-8601 timestamp string.
    pub started_at: String,
}

/// Snapshot of a background task, serialisation-friendly.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskEntry {
    pub id: String,
    /// "bash" | "agent" | "monitor"
    pub task_type: String,
    /// "pending" | "running" | "completed" | "failed"
    pub status: String,
    /// First ~30 chars of the command / prompt.
    pub command_preview: String,
}

/// Centralized application state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub is_streaming: bool,
    pub current_model: String,
    pub total_usage: Usage,
    /// Active subagents tracked by coordinator mode.
    pub active_agents: Vec<AgentEntry>,
    /// Skills currently active for this session.
    pub active_skills: Vec<String>,
    /// Background tasks registered in the task manager.
    pub background_tasks: Vec<TaskEntry>,
    /// Runtime feature flags.
    pub feature_flags: FeatureFlags,
    /// Last rate limit event (for /status display).
    pub last_rate_limit: Option<RateLimitSnapshot>,
    /// Auth status display label (e.g. "⚡ user@example.com" or "🔑 sk-...XXXX").
    pub auth_label: String,
    /// Ingress token for bridge session routing (HMAC-SHA256).
    pub session_ingress_token: Option<String>,
    /// Per-model cost tracking with persistence.
    pub cost_tracker: cost_tracker::CostTracker,
    /// Context window maximum tokens for the current model (0 = unknown).
    pub context_window_max: u32,
    /// Current permission mode label (e.g. "ask", "auto", "bypass").
    pub permission_mode: String,
    /// Current working directory (for status bar display).
    pub cwd: String,
}

/// Snapshot of a rate limit event for display in /status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitSnapshot {
    pub info: RateLimitInfo,
    /// ISO-8601 timestamp string.
    pub occurred_at: String,
    /// Which provider was rate limited.
    pub provider: String,
}

impl Default for AppState {
    fn default() -> Self {
        let session_id = Uuid::new_v4().to_string();
        Self {
            cost_tracker: cost_tracker::CostTracker::new(session_id.clone()),
            session_id,
            messages: Vec::new(),
            is_streaming: false,
            current_model: oxicode_common::constants::DEFAULT_MODEL.to_string(),
            total_usage: Usage::default(),
            active_agents: Vec::new(),
            active_skills: Vec::new(),
            background_tasks: Vec::new(),
            feature_flags: FeatureFlags::default(),
            last_rate_limit: None,
            auth_label: String::new(),
            session_ingress_token: None,
            context_window_max: 0,
            permission_mode: "ask".to_string(),
            cwd: String::new(),
        }
    }
}

/// State store with watch channel for broadcasting updates to subscribers.
pub struct StateStore {
    tx: watch::Sender<AppState>,
    rx: watch::Receiver<AppState>,
}

impl StateStore {
    pub fn new(initial: AppState) -> Self {
        let (tx, rx) = watch::channel(initial);
        Self { tx, rx }
    }

    /// Get a subscriber that receives state updates.
    pub fn subscribe(&self) -> watch::Receiver<AppState> {
        self.rx.clone()
    }

    /// Get current state snapshot.
    pub fn current(&self) -> AppState {
        self.rx.borrow().clone()
    }

    /// Update state via a closure. Broadcasts to all subscribers.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AppState),
    {
        self.tx.send_modify(f);
    }

    /// Add a message to the state.
    pub fn push_message(&self, message: Message) {
        self.update(|state| {
            state.messages.push(message);
        });
    }

    /// Set streaming flag.
    pub fn set_streaming(&self, streaming: bool) {
        self.update(|state| {
            state.is_streaming = streaming;
        });
    }

    /// Clear all messages.
    pub fn clear_messages(&self) {
        self.update(|state| {
            state.messages.clear();
        });
    }

    /// Replace all messages (used by /compact).
    pub fn replace_messages(&self, messages: Vec<Message>) {
        self.update(|state| {
            state.messages = messages;
        });
    }

    /// Accumulate usage from a response, including cache tokens and cost tracking.
    pub fn add_usage(&self, usage: &Usage, model: &str) {
        self.update(|state| {
            state.total_usage.input_tokens += usage.input_tokens;
            state.total_usage.output_tokens += usage.output_tokens;
            // Accumulate cache tokens (Phase 3 fix).
            state.total_usage.cache_read_input_tokens = Some(
                state.total_usage.cache_read_input_tokens.unwrap_or(0)
                    + usage.cache_read_input_tokens.unwrap_or(0),
            );
            state.total_usage.cache_creation_input_tokens = Some(
                state.total_usage.cache_creation_input_tokens.unwrap_or(0)
                    + usage.cache_creation_input_tokens.unwrap_or(0),
            );
            // Update per-model cost tracker.
            state.cost_tracker.record(model, usage);
        });
    }

    /// Replace the active skills list.
    pub fn set_active_skills(&self, skills: Vec<String>) {
        self.update(|state| {
            state.active_skills = skills;
        });
    }

    /// Replace the active agents snapshot.
    pub fn update_agents(&self, agents: Vec<AgentEntry>) {
        self.update(|state| {
            state.active_agents = agents;
        });
    }

    /// Replace the background tasks snapshot.
    pub fn update_tasks(&self, tasks: Vec<TaskEntry>) {
        self.update(|state| {
            state.background_tasks = tasks;
        });
    }

    /// Check if a runtime feature flag is enabled.
    pub fn is_feature_enabled(&self, name: &str) -> bool {
        self.rx.borrow().feature_flags.is_enabled(name)
    }

    /// Toggle a runtime feature flag. Returns the new value.
    pub fn toggle_feature(&self, name: &str) -> Option<bool> {
        let mut result = None;
        self.update(|state| {
            result = state.feature_flags.toggle(name);
        });
        result
    }

    /// Replace the entire feature flags set (e.g. from config reload).
    pub fn set_feature_flags(&self, flags: FeatureFlags) {
        self.update(|state| {
            state.feature_flags = flags;
        });
    }

    /// Record a rate limit event.
    pub fn record_rate_limit(&self, info: RateLimitInfo, provider: String) {
        self.update(|state| {
            state.last_rate_limit = Some(RateLimitSnapshot {
                info,
                occurred_at: chrono::Utc::now().to_rfc3339(),
                provider,
            });
        });
    }
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new(AppState::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_store_update() {
        let store = StateStore::default();
        assert!(store.current().messages.is_empty());

        store.push_message(Message::user("hello"));
        assert_eq!(store.current().messages.len(), 1);
    }

    #[test]
    fn test_state_store_subscribe() {
        let store = StateStore::default();
        let rx = store.subscribe();

        store.set_streaming(true);
        assert!(rx.borrow().is_streaming);

        store.set_streaming(false);
        assert!(!rx.borrow().is_streaming);
    }

    #[test]
    fn test_add_usage() {
        let store = StateStore::default();
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: Some(20),
            cache_creation_input_tokens: Some(10),
        };
        store.add_usage(&usage, "claude-sonnet-4-20250514");
        let state = store.current();
        assert_eq!(state.total_usage.input_tokens, 100);
        assert_eq!(state.total_usage.output_tokens, 50);
        assert_eq!(state.total_usage.cache_read_input_tokens, Some(20));
        assert_eq!(state.total_usage.cache_creation_input_tokens, Some(10));
        assert!(state.cost_tracker.total_cost() > 0.0);
    }

    #[test]
    fn test_appstate_default_new_fields_empty() {
        let state = AppState::default();
        assert!(state.active_agents.is_empty());
        assert!(state.background_tasks.is_empty());
        assert!(state.active_skills.is_empty());
    }

    #[test]
    fn test_set_active_skills() {
        let store = StateStore::default();
        assert!(store.current().active_skills.is_empty());
        store.set_active_skills(vec!["sequential-thinking".to_string()]);
        assert_eq!(store.current().active_skills.len(), 1);
        assert_eq!(store.current().active_skills[0], "sequential-thinking");
    }

    #[test]
    fn test_update_agents() {
        let store = StateStore::default();
        let agent = AgentEntry {
            name: "researcher".to_string(),
            status: "running".to_string(),
            started_at: "2026-04-02T00:00:00Z".to_string(),
        };
        store.update_agents(vec![agent]);
        assert_eq!(store.current().active_agents.len(), 1);
        assert_eq!(store.current().active_agents[0].name, "researcher");
    }

    #[test]
    fn test_update_tasks() {
        let store = StateStore::default();
        let task = TaskEntry {
            id: "t1".to_string(),
            task_type: "bash".to_string(),
            status: "pending".to_string(),
            command_preview: "cargo build".to_string(),
        };
        store.update_tasks(vec![task]);
        assert_eq!(store.current().background_tasks.len(), 1);
        assert_eq!(store.current().background_tasks[0].id, "t1");
    }

    #[test]
    fn test_feature_flags_default_in_state() {
        let store = StateStore::default();
        assert!(store.is_feature_enabled("extended_thinking"));
        assert!(!store.is_feature_enabled("proactive_agents"));
    }

    #[test]
    fn test_toggle_feature() {
        let store = StateStore::default();
        assert!(!store.is_feature_enabled("proactive_agents"));
        let new_val = store.toggle_feature("proactive_agents");
        assert_eq!(new_val, Some(true));
        assert!(store.is_feature_enabled("proactive_agents"));
    }

    #[test]
    fn test_set_feature_flags() {
        let store = StateStore::default();
        let mut flags = FeatureFlags::default();
        flags.set("vim_mode", true);
        store.set_feature_flags(flags);
        assert!(store.is_feature_enabled("vim_mode"));
    }

    #[test]
    fn test_record_rate_limit() {
        use oxicode_common::RateLimitType;

        let store = StateStore::default();
        assert!(store.current().last_rate_limit.is_none());

        let info = RateLimitInfo {
            retry_after_secs: Some(60.0),
            limit_type: RateLimitType::TokensPerMinute,
            remaining: Some(0),
            message: "Rate limited (tokens/min)".to_string(),
            ..Default::default()
        };

        store.record_rate_limit(info.clone(), "anthropic".to_string());

        let current = store.current();
        assert!(current.last_rate_limit.is_some());

        let snapshot = current.last_rate_limit.unwrap();
        assert_eq!(snapshot.provider, "anthropic");
        assert_eq!(snapshot.info.retry_after_secs, Some(60.0));
        assert_eq!(snapshot.info.limit_type, RateLimitType::TokensPerMinute);
        // Verify that occurred_at is set to a valid ISO-8601 timestamp.
        assert!(!snapshot.occurred_at.is_empty());
    }

    #[test]
    fn test_record_rate_limit_overwrites_previous() {
        use oxicode_common::RateLimitType;

        let store = StateStore::default();

        let info1 = RateLimitInfo {
            limit_type: RateLimitType::TokensPerMinute,
            retry_after_secs: Some(10.0),
            ..Default::default()
        };
        store.record_rate_limit(info1, "anthropic".to_string());

        let snapshot1 = store.current().last_rate_limit.unwrap();
        assert_eq!(snapshot1.info.retry_after_secs, Some(10.0));

        let info2 = RateLimitInfo {
            limit_type: RateLimitType::RequestsPerMinute,
            retry_after_secs: Some(30.0),
            ..Default::default()
        };
        store.record_rate_limit(info2, "openai".to_string());

        let snapshot2 = store.current().last_rate_limit.unwrap();
        assert_eq!(snapshot2.provider, "openai");
        assert_eq!(snapshot2.info.retry_after_secs, Some(30.0));
        assert_eq!(snapshot2.info.limit_type, RateLimitType::RequestsPerMinute);
    }

    #[test]
    fn test_rate_limit_snapshot_serde() {
        use oxicode_common::RateLimitType;

        let info = RateLimitInfo {
            retry_after_secs: Some(45.0),
            limit_type: RateLimitType::TokensPerDay,
            remaining: Some(100),
            message: "Rate limited".to_string(),
            ..Default::default()
        };

        let snapshot = RateLimitSnapshot {
            info,
            occurred_at: "2026-04-04T12:00:00Z".to_string(),
            provider: "anthropic".to_string(),
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: RateLimitSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.provider, "anthropic");
        assert_eq!(parsed.info.retry_after_secs, Some(45.0));
        assert_eq!(parsed.info.limit_type, RateLimitType::TokensPerDay);
        assert_eq!(parsed.occurred_at, "2026-04-04T12:00:00Z");
    }
}
