//! Prompt cache break detection for Anthropic API.
//!
//! Detects when the Anthropic prompt cache invalidates by comparing
//! `cache_creation_input_tokens` across consecutive API calls.
//! A significant increase (>2,000 tokens re-cached) indicates a cache break.
//!
//! Tracks per-agent history to avoid false positives when switching subagents.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use oxicode_common::Usage;
use serde::{Deserialize, Serialize};

/// Threshold in tokens — cache creation exceeding this vs previous call = break.
const CACHE_BREAK_THRESHOLD: u32 = 2_000;

/// Maximum history entries per agent to prevent unbounded growth.
const MAX_HISTORY_PER_AGENT: usize = 10;

/// Snapshot of cache-relevant state taken before an API call.
#[derive(Debug, Clone)]
pub struct CacheSnapshot {
    /// Hash of the system prompt text.
    pub system_prompt_hash: u64,
    /// Hash of serialized tool schemas.
    pub tool_schemas_hash: u64,
    /// Model name.
    pub model: String,
    /// Agent ID to isolate subagent switches.
    pub agent_id: Option<String>,
}

/// Event emitted when a cache break is detected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheBreakEvent {
    /// Why the cache broke.
    pub reason: CacheBreakReason,
    /// Previous call's cache_creation_input_tokens.
    pub previous_cached: u32,
    /// Current call's cache_creation_input_tokens.
    pub current_cached: u32,
    /// Difference (current - previous).
    pub diff: i64,
}

/// Reason for the cache break.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CacheBreakReason {
    /// System prompt changed.
    SystemPromptChanged,
    /// Tool schemas changed.
    ToolSchemasChanged,
    /// Both system prompt and tools changed.
    SystemAndToolsChanged,
    /// Cache creation spiked without detectable schema change.
    UnknownCacheInvalidation,
}

/// Per-agent usage history entry.
#[derive(Debug, Clone)]
struct UsageEntry {
    snapshot: CacheSnapshot,
    cache_creation_tokens: u32,
}

/// Detects prompt cache breaks across API calls.
///
/// Maintains per-agent history to distinguish real cache breaks
/// from normal subagent context switching.
pub struct CacheDetector {
    /// Per-agent usage history: agent_id → recent entries.
    history: HashMap<String, Vec<UsageEntry>>,
}

impl CacheDetector {
    /// Create a new cache detector.
    pub fn new() -> Self {
        Self {
            history: HashMap::new(),
        }
    }

    /// Take a snapshot before an API call.
    ///
    /// Hashes the system prompt and tool schemas for later comparison.
    pub fn snapshot_before(
        system_prompt: Option<&str>,
        tool_schemas: &[serde_json::Value],
        model: &str,
        agent_id: Option<&str>,
    ) -> CacheSnapshot {
        let system_prompt_hash = system_prompt.map_or(0, hash_string);
        let tool_schemas_hash = hash_tool_schemas(tool_schemas);

        CacheSnapshot {
            system_prompt_hash,
            tool_schemas_hash,
            model: model.to_string(),
            agent_id: agent_id.map(String::from),
        }
    }

    /// Detect a cache break after receiving usage data.
    ///
    /// Compares current usage against the last call for the same agent.
    /// Returns `Some(CacheBreakEvent)` if a cache break is detected.
    pub fn detect_break(
        &mut self,
        snapshot: &CacheSnapshot,
        usage: &Usage,
    ) -> Option<CacheBreakEvent> {
        let agent_key = snapshot.agent_id.clone().unwrap_or_default();
        let current_creation = usage.cache_creation_input_tokens.unwrap_or(0);

        let result = if let Some(history) = self.history.get(&agent_key) {
            if let Some(last) = history.last() {
                // Compare against previous call for same agent.
                let prev_creation = last.cache_creation_tokens;
                let diff = i64::from(current_creation) - i64::from(prev_creation);

                if diff > i64::from(CACHE_BREAK_THRESHOLD) {
                    // Determine reason by comparing hashes.
                    let sys_changed =
                        snapshot.system_prompt_hash != last.snapshot.system_prompt_hash;
                    let tools_changed =
                        snapshot.tool_schemas_hash != last.snapshot.tool_schemas_hash;

                    let reason = match (sys_changed, tools_changed) {
                        (true, true) => CacheBreakReason::SystemAndToolsChanged,
                        (true, false) => CacheBreakReason::SystemPromptChanged,
                        (false, true) => CacheBreakReason::ToolSchemasChanged,
                        (false, false) => CacheBreakReason::UnknownCacheInvalidation,
                    };

                    Some(CacheBreakEvent {
                        reason,
                        previous_cached: prev_creation,
                        current_cached: current_creation,
                        diff,
                    })
                } else {
                    None
                }
            } else {
                None // First call for this agent.
            }
        } else {
            None // First call for this agent.
        };

        // Record this call in history.
        let entry = UsageEntry {
            snapshot: snapshot.clone(),
            cache_creation_tokens: current_creation,
        };
        let history = self.history.entry(agent_key).or_default();
        history.push(entry);

        // Trim history to prevent unbounded growth.
        if history.len() > MAX_HISTORY_PER_AGENT {
            history.drain(..history.len() - MAX_HISTORY_PER_AGENT);
        }

        result
    }

    /// Clear history for a specific agent (e.g., when agent is terminated).
    pub fn clear_agent(&mut self, agent_id: &str) {
        self.history.remove(agent_id);
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.history.clear();
    }
}

impl Default for CacheDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash a string using DefaultHasher (fast, stable within a process).
fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Hash tool schemas by serializing to a canonical JSON string.
fn hash_tool_schemas(schemas: &[serde_json::Value]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for schema in schemas {
        // Use to_string for deterministic output (serde_json sorts keys).
        let s = serde_json::to_string(schema).unwrap_or_default();
        s.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_usage(cache_creation: u32, cache_read: u32) -> Usage {
        Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(cache_creation),
            cache_read_input_tokens: Some(cache_read),
        }
    }

    fn make_snapshot(system: &str, tools: &[&str], agent_id: Option<&str>) -> CacheSnapshot {
        let tool_schemas: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| serde_json::json!({"name": t}))
            .collect();
        CacheDetector::snapshot_before(
            Some(system),
            &tool_schemas,
            "claude-sonnet-4-20250514",
            agent_id,
        )
    }

    #[test]
    fn test_no_break_on_first_call() {
        let mut detector = CacheDetector::new();
        let snapshot = make_snapshot("You are helpful.", &["read", "write"], None);
        let usage = make_usage(5000, 0);

        let result = detector.detect_break(&snapshot, &usage);
        assert!(result.is_none(), "First call should never be a break");
    }

    #[test]
    fn test_no_break_same_prompt_twice() {
        let mut detector = CacheDetector::new();
        let snapshot = make_snapshot("You are helpful.", &["read", "write"], None);

        // First call — creates cache.
        let usage1 = make_usage(5000, 0);
        detector.detect_break(&snapshot, &usage1);

        // Second call — reads from cache, low creation.
        let usage2 = make_usage(0, 5000);
        let result = detector.detect_break(&snapshot, &usage2);
        assert!(result.is_none(), "Same prompt should not trigger break");
    }

    #[test]
    fn test_break_on_system_prompt_change() {
        let mut detector = CacheDetector::new();
        let tools = &["read", "write"];

        // First call.
        let snap1 = make_snapshot("You are helpful.", tools, None);
        detector.detect_break(&snap1, &make_usage(5000, 0));

        // Second call — cache read (normal).
        detector.detect_break(&snap1, &make_usage(0, 5000));

        // Third call — system prompt changed, re-caches.
        let snap2 = make_snapshot("You are a coding assistant.", tools, None);
        let result = detector.detect_break(&snap2, &make_usage(6000, 0));

        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.reason, CacheBreakReason::SystemPromptChanged);
        assert!(event.diff > i64::from(CACHE_BREAK_THRESHOLD));
    }

    #[test]
    fn test_break_on_tools_change() {
        let mut detector = CacheDetector::new();
        let prompt = "You are helpful.";

        // First call.
        let snap1 = make_snapshot(prompt, &["read", "write"], None);
        detector.detect_break(&snap1, &make_usage(5000, 0));

        // Second call — tools changed, cache re-created with larger token count.
        let snap2 = make_snapshot(prompt, &["read", "write", "execute"], None);
        let result = detector.detect_break(&snap2, &make_usage(7500, 0));

        assert!(result.is_some());
        assert_eq!(result.unwrap().reason, CacheBreakReason::ToolSchemasChanged);
    }

    #[test]
    fn test_no_false_positive_on_agent_switch() {
        let mut detector = CacheDetector::new();
        let prompt = "You are helpful.";
        let tools = &["read"];

        // Agent A calls.
        let snap_a = make_snapshot(prompt, tools, Some("agent-a"));
        detector.detect_break(&snap_a, &make_usage(5000, 0));
        detector.detect_break(&snap_a, &make_usage(0, 5000));

        // Agent B calls — different agent, should NOT compare against A.
        let snap_b = make_snapshot("Different prompt for B", tools, Some("agent-b"));
        let result = detector.detect_break(&snap_b, &make_usage(8000, 0));

        assert!(
            result.is_none(),
            "First call for agent-b should not be flagged as break"
        );
    }

    #[test]
    fn test_break_within_same_agent() {
        let mut detector = CacheDetector::new();
        let tools = &["read"];

        // Agent A call 1.
        let snap1 = make_snapshot("Prompt v1", tools, Some("agent-a"));
        detector.detect_break(&snap1, &make_usage(5000, 0));

        // Agent A call 2 — same prompt, cache read.
        detector.detect_break(&snap1, &make_usage(0, 5000));

        // Agent A call 3 — prompt changed.
        let snap2 = make_snapshot("Prompt v2 completely different", tools, Some("agent-a"));
        let result = detector.detect_break(&snap2, &make_usage(6000, 0));

        assert!(result.is_some());
        assert_eq!(
            result.unwrap().reason,
            CacheBreakReason::SystemPromptChanged
        );
    }

    #[test]
    fn test_below_threshold_no_break() {
        let mut detector = CacheDetector::new();
        let tools = &["read"];

        // First call.
        let snap = make_snapshot("Prompt", tools, None);
        detector.detect_break(&snap, &make_usage(5000, 0));

        // Second call — slight increase below threshold.
        let snap2 = make_snapshot("Prompt slightly changed", tools, None);
        let result = detector.detect_break(&snap2, &make_usage(6500, 0));

        // diff = 1500, below 2000 threshold.
        assert!(result.is_none());
    }

    #[test]
    fn test_clear_agent_history() {
        let mut detector = CacheDetector::new();
        let snap = make_snapshot("Prompt", &["read"], Some("agent-x"));
        detector.detect_break(&snap, &make_usage(5000, 0));

        detector.clear_agent("agent-x");

        // After clearing, next call is treated as first — no break.
        let result = detector.detect_break(&snap, &make_usage(8000, 0));
        assert!(result.is_none());
    }

    #[test]
    fn test_snapshot_hashing_deterministic() {
        let snap1 = make_snapshot("Hello", &["a", "b"], None);
        let snap2 = make_snapshot("Hello", &["a", "b"], None);
        assert_eq!(snap1.system_prompt_hash, snap2.system_prompt_hash);
        assert_eq!(snap1.tool_schemas_hash, snap2.tool_schemas_hash);
    }

    #[test]
    fn test_cache_break_event_serialization() {
        let event = CacheBreakEvent {
            reason: CacheBreakReason::SystemPromptChanged,
            previous_cached: 0,
            current_cached: 5000,
            diff: 5000,
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: CacheBreakEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.reason, CacheBreakReason::SystemPromptChanged);
        assert_eq!(parsed.current_cached, 5000);
    }
}
