//! Per-model cost tracking with session persistence.
//!
//! Tracks token usage (input, output, cache read, cache write) per model,
//! calculates USD cost from a rate table, and persists to disk for session
//! restore. Rates default to Anthropic pricing; unknown models use Sonnet rates.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use oxicode_common::Usage;
use serde::{Deserialize, Serialize};

/// Per-model token accumulation + USD cost.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: f64,
}

/// Per-million-token pricing for a model family.
#[derive(Debug, Clone, Copy)]
pub struct ModelRate {
    /// $/Mtok for input tokens.
    pub input: f64,
    /// $/Mtok for output tokens.
    pub output: f64,
    /// $/Mtok for cache read tokens.
    pub cache_read: f64,
    /// $/Mtok for cache write tokens.
    pub cache_write: f64,
}

/// Default rate table: model-prefix → pricing.
/// Source: <https://platform.claude.com/docs/en/about-claude/pricing>
fn default_rates() -> Vec<(&'static str, ModelRate)> {
    vec![
        // Haiku 3.5: $0.80 / $4
        (
            "claude-3-5-haiku",
            ModelRate {
                input: 0.80,
                output: 4.0,
                cache_read: 0.08,
                cache_write: 1.0,
            },
        ),
        // Haiku 4.5: $1 / $5
        (
            "claude-haiku-4",
            ModelRate {
                input: 1.0,
                output: 5.0,
                cache_read: 0.1,
                cache_write: 1.25,
            },
        ),
        // Sonnet family: $3 / $15
        (
            "claude-sonnet",
            ModelRate {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
            },
        ),
        (
            "claude-3-5-sonnet",
            ModelRate {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
            },
        ),
        (
            "claude-3-7-sonnet",
            ModelRate {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
            },
        ),
        // Opus 4/4.1: $15 / $75
        (
            "claude-opus-4",
            ModelRate {
                input: 15.0,
                output: 75.0,
                cache_read: 1.5,
                cache_write: 18.75,
            },
        ),
        // Opus 4.5+: $5 / $25
        (
            "claude-opus-4-5",
            ModelRate {
                input: 5.0,
                output: 25.0,
                cache_read: 0.5,
                cache_write: 6.25,
            },
        ),
        (
            "claude-opus-4-6",
            ModelRate {
                input: 5.0,
                output: 25.0,
                cache_read: 0.5,
                cache_write: 6.25,
            },
        ),
        // GPT-4o approx
        (
            "gpt-4o",
            ModelRate {
                input: 2.5,
                output: 10.0,
                cache_read: 1.25,
                cache_write: 2.5,
            },
        ),
        // Gemini 1.5 Pro approx
        (
            "gemini",
            ModelRate {
                input: 1.25,
                output: 5.0,
                cache_read: 0.3,
                cache_write: 1.0,
            },
        ),
    ]
}

/// Fallback: Sonnet pricing for unknown models.
const FALLBACK_RATE: ModelRate = ModelRate {
    input: 3.0,
    output: 15.0,
    cache_read: 0.3,
    cache_write: 3.75,
};

/// Look up rate by longest-prefix match on model name.
pub fn get_rate(model: &str) -> ModelRate {
    let lower = model.to_lowercase();
    let rates = default_rates();
    // Longest prefix wins (e.g. "claude-opus-4-5" over "claude-opus-4").
    rates
        .iter()
        .filter(|(prefix, _)| lower.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map_or(FALLBACK_RATE, |(_, rate)| *rate)
}

/// Calculate USD cost for a single usage event.
pub fn calculate_cost(model: &str, usage: &Usage) -> f64 {
    let rate = get_rate(model);
    let input = f64::from(usage.input_tokens) / 1_000_000.0 * rate.input;
    let output = f64::from(usage.output_tokens) / 1_000_000.0 * rate.output;
    let cache_read =
        f64::from(usage.cache_read_input_tokens.unwrap_or(0)) / 1_000_000.0 * rate.cache_read;
    let cache_write =
        f64::from(usage.cache_creation_input_tokens.unwrap_or(0)) / 1_000_000.0 * rate.cache_write;
    input + output + cache_read + cache_write
}

/// Persistent cost tracker: per-model usage + session ID for restore guard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostTracker {
    pub session_id: String,
    pub models: HashMap<String, ModelUsage>,
    /// Whether any unknown model was used (cost may be inaccurate).
    #[serde(default)]
    pub has_unknown_model: bool,
}

impl CostTracker {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            models: HashMap::new(),
            has_unknown_model: false,
        }
    }

    /// Record a usage event from an API response.
    pub fn record(&mut self, model: &str, usage: &Usage) {
        let cost = calculate_cost(model, usage);
        // Check if model has known rate.
        let lower = model.to_lowercase();
        let known = default_rates().iter().any(|(p, _)| lower.starts_with(p));
        if !known {
            self.has_unknown_model = true;
        }

        let entry = self.models.entry(model.to_string()).or_default();
        entry.input_tokens += u64::from(usage.input_tokens);
        entry.output_tokens += u64::from(usage.output_tokens);
        entry.cache_read_tokens += u64::from(usage.cache_read_input_tokens.unwrap_or(0));
        entry.cache_write_tokens += u64::from(usage.cache_creation_input_tokens.unwrap_or(0));
        entry.cost_usd += cost;
    }

    /// Total cost across all models.
    pub fn total_cost(&self) -> f64 {
        self.models.values().map(|m| m.cost_usd).sum()
    }

    /// Total tokens across all models.
    pub fn total_tokens(&self) -> (u64, u64) {
        let input: u64 = self.models.values().map(|m| m.input_tokens).sum();
        let output: u64 = self.models.values().map(|m| m.output_tokens).sum();
        (input, output)
    }

    /// Per-model summary sorted by cost descending.
    pub fn summary(&self) -> Vec<(&str, &ModelUsage)> {
        let mut entries: Vec<_> = self.models.iter().map(|(k, v)| (k.as_str(), v)).collect();
        entries.sort_by(|a, b| {
            b.1.cost_usd
                .partial_cmp(&a.1.cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries
    }

    /// Format cost for display: >$0.50 shows 2 decimals, else 4.
    pub fn format_cost(cost: f64) -> String {
        if cost > 0.50 {
            format!("${cost:.2}")
        } else {
            format!("${cost:.4}")
        }
    }
}

// ── Persistence ──

/// Default persistence path.
pub fn default_cost_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicode/costs.json")
}

/// Load cost tracker from disk. Only restores if session_id matches.
pub fn load_costs(path: &Path, session_id: &str) -> Option<CostTracker> {
    let data = std::fs::read_to_string(path).ok()?;
    let tracker: CostTracker = serde_json::from_str(&data).ok()?;
    if tracker.session_id == session_id {
        Some(tracker)
    } else {
        None
    }
}

/// Save cost tracker to disk (sync — caller should use tokio::task::spawn_blocking).
pub fn save_costs(path: &Path, tracker: &CostTracker) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(tracker).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_usage(input: u32, output: u32, cache_read: u32, cache_write: u32) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            cache_read_input_tokens: if cache_read > 0 {
                Some(cache_read)
            } else {
                None
            },
            cache_creation_input_tokens: if cache_write > 0 {
                Some(cache_write)
            } else {
                None
            },
        }
    }

    #[test]
    fn sonnet_rate_lookup() {
        let rate = get_rate("claude-sonnet-4-20250514");
        assert!((rate.input - 3.0).abs() < f64::EPSILON);
        assert!((rate.output - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn opus_rate_lookup() {
        let rate = get_rate("claude-opus-4-20260101");
        assert!((rate.input - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn opus_45_rate_lookup() {
        let rate = get_rate("claude-opus-4-5-20260101");
        assert!((rate.input - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn haiku_rate_lookup() {
        let rate = get_rate("claude-3-5-haiku-20260101");
        assert!((rate.input - 0.80).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_model_uses_fallback() {
        let rate = get_rate("some-random-model");
        assert!((rate.input - FALLBACK_RATE.input).abs() < f64::EPSILON);
    }

    #[test]
    fn calculate_cost_sonnet() {
        let usage = make_usage(1_000_000, 500_000, 200_000, 100_000);
        let cost = calculate_cost("claude-sonnet-4-20250514", &usage);
        // 1M * 3/M + 500K * 15/M + 200K * 0.3/M + 100K * 3.75/M
        // = 3.0 + 7.5 + 0.06 + 0.375 = 10.935
        assert!((cost - 10.935).abs() < 0.001);
    }

    #[test]
    fn calculate_cost_no_cache() {
        let usage = make_usage(100_000, 50_000, 0, 0);
        let cost = calculate_cost("claude-sonnet-4-20250514", &usage);
        // 100K * 3/M + 50K * 15/M = 0.3 + 0.75 = 1.05
        assert!((cost - 1.05).abs() < 0.001);
    }

    #[test]
    fn tracker_record_accumulates() {
        let mut tracker = CostTracker::new("sess1".to_string());
        tracker.record("claude-sonnet-4-20250514", &make_usage(1000, 500, 0, 0));
        tracker.record("claude-sonnet-4-20250514", &make_usage(2000, 1000, 0, 0));

        let entry = &tracker.models["claude-sonnet-4-20250514"];
        assert_eq!(entry.input_tokens, 3000);
        assert_eq!(entry.output_tokens, 1500);
    }

    #[test]
    fn tracker_multi_model() {
        let mut tracker = CostTracker::new("sess1".to_string());
        tracker.record("claude-sonnet-4-20250514", &make_usage(1000, 500, 0, 0));
        tracker.record("claude-opus-4-20260101", &make_usage(1000, 500, 0, 0));

        assert_eq!(tracker.models.len(), 2);
        // Opus costs more
        let sonnet_cost = tracker.models["claude-sonnet-4-20250514"].cost_usd;
        let opus_cost = tracker.models["claude-opus-4-20260101"].cost_usd;
        assert!(opus_cost > sonnet_cost);
    }

    #[test]
    fn tracker_total_cost() {
        let mut tracker = CostTracker::new("sess1".to_string());
        tracker.record("claude-sonnet-4-20250514", &make_usage(1_000_000, 0, 0, 0));
        // 1M input at $3/Mtok = $3.00
        assert!((tracker.total_cost() - 3.0).abs() < 0.001);
    }

    #[test]
    fn tracker_total_tokens() {
        let mut tracker = CostTracker::new("sess1".to_string());
        tracker.record("model-a", &make_usage(100, 50, 0, 0));
        tracker.record("model-b", &make_usage(200, 100, 0, 0));
        let (input, output) = tracker.total_tokens();
        assert_eq!(input, 300);
        assert_eq!(output, 150);
    }

    #[test]
    fn tracker_summary_sorted() {
        let mut tracker = CostTracker::new("sess1".to_string());
        tracker.record("cheap-model", &make_usage(100, 50, 0, 0));
        tracker.record(
            "claude-opus-4-20260101",
            &make_usage(1_000_000, 500_000, 0, 0),
        );
        let summary = tracker.summary();
        assert_eq!(summary.len(), 2);
        // Opus should be first (most expensive)
        assert!(summary[0].0.contains("opus"));
    }

    #[test]
    fn tracker_unknown_model_flag() {
        let mut tracker = CostTracker::new("sess1".to_string());
        assert!(!tracker.has_unknown_model);
        tracker.record("totally-unknown-model", &make_usage(100, 50, 0, 0));
        assert!(tracker.has_unknown_model);
    }

    #[test]
    fn format_cost_small() {
        assert_eq!(CostTracker::format_cost(0.0012), "$0.0012");
    }

    #[test]
    fn format_cost_large() {
        assert_eq!(CostTracker::format_cost(1.5), "$1.50");
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = std::env::temp_dir().join(format!("oxi-cost-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("costs.json");

        let mut tracker = CostTracker::new("sess_abc".to_string());
        tracker.record("claude-sonnet-4-20250514", &make_usage(1000, 500, 100, 50));

        save_costs(&path, &tracker).unwrap();
        let loaded = load_costs(&path, "sess_abc").expect("should load");
        assert_eq!(loaded.session_id, "sess_abc");
        assert_eq!(loaded.models.len(), 1);
        assert!((loaded.total_cost() - tracker.total_cost()).abs() < f64::EPSILON);

        // Different session_id → None
        assert!(load_costs(&path, "other_session").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persistence_missing_file() {
        assert!(load_costs(Path::new("/nonexistent/costs.json"), "any").is_none());
    }

    #[test]
    fn cache_token_accumulation() {
        let mut tracker = CostTracker::new("sess1".to_string());
        tracker.record("claude-sonnet-4-20250514", &make_usage(0, 0, 500, 200));
        tracker.record("claude-sonnet-4-20250514", &make_usage(0, 0, 300, 100));

        let entry = &tracker.models["claude-sonnet-4-20250514"];
        assert_eq!(entry.cache_read_tokens, 800);
        assert_eq!(entry.cache_write_tokens, 300);
    }
}
