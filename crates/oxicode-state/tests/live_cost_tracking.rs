//! Integration tests for cost tracking and state store usage accumulation.
//!
//! Tests the cost tracker's pricing, accumulation, persistence, and integration
//! with the StateStore's `add_usage()` method.
//!
//! Run with: `cargo test -p oxicode-state --test live_cost_tracking`

use oxicode_common::Usage;
use oxicode_state::cost_tracker::{calculate_cost, get_rate, save_costs, load_costs, CostTracker};
use oxicode_state::{AppState, StateStore};

// ── Helper ──────────────────────────────────────────────────────

fn make_usage(input: u32, output: u32) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
    }
}

fn make_usage_with_cache(input: u32, output: u32, cache_read: u32, cache_write: u32) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_input_tokens: if cache_read > 0 { Some(cache_read) } else { None },
        cache_creation_input_tokens: if cache_write > 0 { Some(cache_write) } else { None },
    }
}

// ── Rate Lookup ─────────────────────────────────────────────────

#[test]
fn test_sonnet_rate_correct() {
    let rate = get_rate("claude-sonnet-4-20250514");
    assert!((rate.input - 3.0).abs() < f64::EPSILON);
    assert!((rate.output - 15.0).abs() < f64::EPSILON);
}

#[test]
fn test_haiku_rate_correct() {
    let rate = get_rate("claude-haiku-4-20260101");
    assert!((rate.input - 1.0).abs() < f64::EPSILON);
    assert!((rate.output - 5.0).abs() < f64::EPSILON);
}

#[test]
fn test_opus_rate_correct() {
    let rate = get_rate("claude-opus-4-20260101");
    assert!((rate.input - 15.0).abs() < f64::EPSILON);
}

#[test]
fn test_unknown_model_uses_sonnet_fallback() {
    let rate = get_rate("totally-unknown-model");
    // Fallback is Sonnet pricing: $3/$15
    assert!((rate.input - 3.0).abs() < f64::EPSILON);
    assert!((rate.output - 15.0).abs() < f64::EPSILON);
}

// ── Cost Calculation ────────────────────────────────────────────

#[test]
fn test_calculate_cost_basic() {
    // 100K input at $3/Mtok + 50K output at $15/Mtok = $0.30 + $0.75 = $1.05
    let cost = calculate_cost("claude-sonnet-4-20250514", &make_usage(100_000, 50_000));
    assert!((cost - 1.05).abs() < 0.001, "Expected $1.05, got ${cost:.4}");
}

#[test]
fn test_calculate_cost_with_cache() {
    // 1M input + 500K output + 200K cache_read + 100K cache_write
    // = 3.0 + 7.5 + 0.06 + 0.375 = 10.935
    let usage = make_usage_with_cache(1_000_000, 500_000, 200_000, 100_000);
    let cost = calculate_cost("claude-sonnet-4-20250514", &usage);
    assert!((cost - 10.935).abs() < 0.001, "Expected $10.935, got ${cost:.4}");
}

#[test]
fn test_calculate_cost_zero_tokens() {
    let cost = calculate_cost("claude-sonnet-4-20250514", &make_usage(0, 0));
    assert!((cost).abs() < f64::EPSILON, "Zero tokens should cost $0");
}

// ── CostTracker Accumulation ────────────────────────────────────

#[test]
fn test_tracker_accumulates_across_records() {
    let mut tracker = CostTracker::new("sess-1".to_string());
    tracker.record("claude-sonnet-4-20250514", &make_usage(1000, 500));
    tracker.record("claude-sonnet-4-20250514", &make_usage(2000, 1000));

    let entry = &tracker.models["claude-sonnet-4-20250514"];
    assert_eq!(entry.input_tokens, 3000);
    assert_eq!(entry.output_tokens, 1500);
    assert!(entry.cost_usd > 0.0);
}

#[test]
fn test_tracker_multi_model_tracking() {
    let mut tracker = CostTracker::new("sess-1".to_string());
    tracker.record("claude-sonnet-4-20250514", &make_usage(1000, 500));
    tracker.record("claude-haiku-4-20260101", &make_usage(1000, 500));

    assert_eq!(tracker.models.len(), 2);
    let (total_input, total_output) = tracker.total_tokens();
    assert_eq!(total_input, 2000);
    assert_eq!(total_output, 1000);
}

#[test]
fn test_tracker_flags_unknown_model() {
    let mut tracker = CostTracker::new("sess-1".to_string());
    assert!(!tracker.has_unknown_model);
    tracker.record("some-random-llm", &make_usage(100, 50));
    assert!(tracker.has_unknown_model);
}

#[test]
fn test_tracker_total_cost_sums_models() {
    let mut tracker = CostTracker::new("sess-1".to_string());
    tracker.record("claude-sonnet-4-20250514", &make_usage(1_000_000, 0));
    tracker.record("claude-haiku-4-20260101", &make_usage(1_000_000, 0));

    // Sonnet: 1M × $3/Mtok = $3.00, Haiku: 1M × $1/Mtok = $1.00
    let total = tracker.total_cost();
    assert!((total - 4.0).abs() < 0.01, "Expected ~$4.00, got ${total:.4}");
}

// ── CostTracker Persistence ─────────────────────────────────────

#[test]
fn test_tracker_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("costs.json");

    let mut tracker = CostTracker::new("sess-roundtrip".to_string());
    tracker.record("claude-sonnet-4-20250514", &make_usage_with_cache(5000, 2000, 1000, 500));

    save_costs(&path, &tracker).unwrap();
    let loaded = load_costs(&path, "sess-roundtrip").expect("Should load with matching session ID");

    assert_eq!(loaded.session_id, "sess-roundtrip");
    assert_eq!(loaded.models.len(), 1);
    assert!((loaded.total_cost() - tracker.total_cost()).abs() < f64::EPSILON);

    let entry = &loaded.models["claude-sonnet-4-20250514"];
    assert_eq!(entry.input_tokens, 5000);
    assert_eq!(entry.output_tokens, 2000);
    assert_eq!(entry.cache_read_tokens, 1000);
    assert_eq!(entry.cache_write_tokens, 500);
}

#[test]
fn test_tracker_load_wrong_session_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("costs.json");

    let tracker = CostTracker::new("session-A".to_string());
    save_costs(&path, &tracker).unwrap();

    assert!(
        load_costs(&path, "session-B").is_none(),
        "Should not load costs for a different session ID"
    );
}

#[test]
fn test_tracker_load_missing_file_returns_none() {
    assert!(load_costs(std::path::Path::new("/nonexistent/costs.json"), "any").is_none());
}

// ── StateStore Integration ──────────────────────────────────────

#[test]
fn test_state_store_add_usage_updates_cost_tracker() {
    let store = StateStore::new(AppState::default());
    let usage = make_usage(10_000, 5_000);

    store.add_usage(&usage, "claude-sonnet-4-20250514");

    let state = store.current();
    assert_eq!(state.total_usage.input_tokens, 10_000);
    assert_eq!(state.total_usage.output_tokens, 5_000);
    assert!(state.cost_tracker.total_cost() > 0.0, "Cost should be tracked");
    assert_eq!(state.cost_tracker.models.len(), 1);
}

#[test]
fn test_state_store_add_usage_accumulates() {
    let store = StateStore::new(AppState::default());

    store.add_usage(&make_usage(1000, 500), "claude-sonnet-4-20250514");
    store.add_usage(&make_usage(2000, 1000), "claude-sonnet-4-20250514");
    store.add_usage(&make_usage(500, 250), "claude-haiku-4-20260101");

    let state = store.current();
    assert_eq!(state.total_usage.input_tokens, 3500);
    assert_eq!(state.total_usage.output_tokens, 1750);
    assert_eq!(state.cost_tracker.models.len(), 2);
}

#[test]
fn test_state_store_add_usage_with_cache_tokens() {
    let store = StateStore::new(AppState::default());
    let usage = make_usage_with_cache(1000, 500, 300, 100);

    store.add_usage(&usage, "claude-sonnet-4-20250514");

    let state = store.current();
    assert_eq!(state.total_usage.cache_read_input_tokens, Some(300));
    assert_eq!(state.total_usage.cache_creation_input_tokens, Some(100));
}

// ── Cost Formatting ─────────────────────────────────────────────

#[test]
fn test_format_cost_small_amount() {
    assert_eq!(CostTracker::format_cost(0.0012), "$0.0012");
    assert_eq!(CostTracker::format_cost(0.0001), "$0.0001");
}

#[test]
fn test_format_cost_large_amount() {
    assert_eq!(CostTracker::format_cost(1.5), "$1.50");
    assert_eq!(CostTracker::format_cost(10.935), "$10.94");
}
