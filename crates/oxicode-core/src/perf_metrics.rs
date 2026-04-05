//! Performance metrics — records per-tool call durations and computes summary statistics.
//!
//! Intended for lightweight, in-process instrumentation. Not a substitute for a
//! full observability stack, but useful for surfacing slow tools during development.

use std::collections::HashMap;
use std::time::Duration;

/// Accumulated timing data for a single tool or operation name.
#[derive(Debug, Clone, Default)]
pub struct PerfMetrics {
    /// Raw duration recordings keyed by tool name.
    recordings: HashMap<String, Vec<Duration>>,
}

/// Summary statistics for one tool's recorded calls.
#[derive(Debug, Clone)]
pub struct PerfSummary {
    /// Name of the tool or operation.
    pub tool_name: String,
    /// Total number of recorded calls.
    pub call_count: usize,
    /// Mean duration across all calls.
    pub avg: Duration,
    /// 50th-percentile (median) duration.
    pub p50: Duration,
    /// 95th-percentile duration.
    pub p95: Duration,
    /// Single slowest call duration.
    pub max: Duration,
}

impl PerfMetrics {
    /// Create a new, empty `PerfMetrics` instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a single timing observation for `tool`.
    pub fn record(&mut self, tool: &str, duration: Duration) {
        self.recordings
            .entry(tool.to_string())
            .or_default()
            .push(duration);
    }

    /// Compute summary statistics for every tracked tool.
    ///
    /// Results are sorted by total accumulated time, descending (slowest tools first).
    pub fn summary(&self) -> Vec<PerfSummary> {
        let mut summaries: Vec<PerfSummary> = self
            .recordings
            .iter()
            .filter_map(|(name, durations)| {
                if durations.is_empty() {
                    return None;
                }
                let mut sorted = durations.clone();
                sorted.sort_unstable();

                let call_count = sorted.len();
                let total_nanos: u128 = sorted.iter().map(|d| d.as_nanos()).sum();
                let avg = Duration::from_nanos((total_nanos / call_count as u128) as u64);
                let p50 = percentile(&sorted, 50);
                let p95 = percentile(&sorted, 95);
                let max = *sorted.last().unwrap_or(&Duration::ZERO);

                Some(PerfSummary {
                    tool_name: name.clone(),
                    call_count,
                    avg,
                    p50,
                    p95,
                    max,
                })
            })
            .collect();

        // Sort by total time descending so the heaviest tools appear first.
        summaries.sort_by(|a, b| {
            let total_a = a.avg.as_nanos() * a.call_count as u128;
            let total_b = b.avg.as_nanos() * b.call_count as u128;
            total_b.cmp(&total_a)
        });

        summaries
    }

    /// Remove all recorded data, resetting the instance to an empty state.
    pub fn clear(&mut self) {
        self.recordings.clear();
    }
}

/// Compute the Nth percentile of a **sorted** slice of durations.
///
/// Uses nearest-rank method. Returns `Duration::ZERO` if the slice is empty.
fn percentile(sorted: &[Duration], n: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let index = ((n * sorted.len()).saturating_sub(1)) / 100;
    sorted[index.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_summary_basic() {
        let mut m = PerfMetrics::new();
        m.record("read_file", Duration::from_millis(10));
        m.record("read_file", Duration::from_millis(20));
        m.record("read_file", Duration::from_millis(30));

        let summaries = m.summary();
        assert_eq!(summaries.len(), 1);

        let s = &summaries[0];
        assert_eq!(s.tool_name, "read_file");
        assert_eq!(s.call_count, 3);
        assert_eq!(s.avg, Duration::from_millis(20));
        assert_eq!(s.max, Duration::from_millis(30));
    }

    #[test]
    fn clear_resets_state() {
        let mut m = PerfMetrics::new();
        m.record("bash", Duration::from_millis(5));
        m.clear();
        assert!(m.summary().is_empty());
    }

    #[test]
    fn percentile_p95_single_element() {
        let data = vec![Duration::from_millis(42)];
        assert_eq!(percentile(&data, 95), Duration::from_millis(42));
    }

    #[test]
    fn summary_sorted_by_total_desc() {
        let mut m = PerfMetrics::new();
        // "fast" tool: 1 call × 1 ms
        m.record("fast", Duration::from_millis(1));
        // "slow" tool: 1 call × 100 ms
        m.record("slow", Duration::from_millis(100));

        let summaries = m.summary();
        assert_eq!(summaries[0].tool_name, "slow");
    }
}
