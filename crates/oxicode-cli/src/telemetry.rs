//! Opt-in telemetry for observability.
//!
//! Provides OpenTelemetry-compatible tracing and metrics for API calls,
//! tool executions, and session lifecycle. Disabled by default; enabled
//! via settings or `/telemetry` command.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// Whether telemetry collection is currently enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Default OTLP gRPC endpoint used by `TelemetryConfig`.
const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4317";

/// Global singleton metrics collector.
static GLOBAL_COLLECTOR: OnceLock<MetricsCollector> = OnceLock::new();

/// Get the global shared metrics collector.
pub fn global_collector() -> &'static MetricsCollector {
    GLOBAL_COLLECTOR.get_or_init(MetricsCollector::new)
}

/// Telemetry configuration.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Whether telemetry is enabled.
    pub enabled: bool,
    /// OTLP gRPC endpoint for trace/metric export.
    pub otlp_endpoint: String,
    /// Service name for traces.
    pub service_name: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            otlp_endpoint: DEFAULT_OTLP_ENDPOINT.to_string(),
            service_name: "oxicode-cli".to_string(),
        }
    }
}

/// Lightweight metrics collector (no external deps required).
/// Tracks counters and latencies in-process for `/telemetry` stats display.
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    inner: Arc<MetricsInner>,
}

#[derive(Debug)]
struct MetricsInner {
    api_calls: AtomicU64,
    api_errors: AtomicU64,
    api_latency_ms_sum: AtomicU64,
    tool_executions: AtomicU64,
    tool_errors: AtomicU64,
    tool_latency_ms_sum: AtomicU64,
    tokens_input: AtomicU64,
    tokens_output: AtomicU64,
    sessions_started: AtomicU64,
}

impl MetricsCollector {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(MetricsInner {
                api_calls: AtomicU64::new(0),
                api_errors: AtomicU64::new(0),
                api_latency_ms_sum: AtomicU64::new(0),
                tool_executions: AtomicU64::new(0),
                tool_errors: AtomicU64::new(0),
                tool_latency_ms_sum: AtomicU64::new(0),
                tokens_input: AtomicU64::new(0),
                tokens_output: AtomicU64::new(0),
                sessions_started: AtomicU64::new(0),
            }),
        }
    }

    /// Record an API call completion.
    pub fn record_api_call(&self, latency_ms: u64, is_error: bool) {
        if !is_telemetry_enabled() {
            return;
        }
        self.inner.api_calls.fetch_add(1, Ordering::Relaxed);
        self.inner
            .api_latency_ms_sum
            .fetch_add(latency_ms, Ordering::Relaxed);
        if is_error {
            self.inner.api_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a tool execution.
    pub fn record_tool_execution(&self, latency_ms: u64, is_error: bool) {
        if !is_telemetry_enabled() {
            return;
        }
        self.inner.tool_executions.fetch_add(1, Ordering::Relaxed);
        self.inner
            .tool_latency_ms_sum
            .fetch_add(latency_ms, Ordering::Relaxed);
        if is_error {
            self.inner.tool_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record token usage from an API response.
    pub fn record_tokens(&self, input: u64, output: u64) {
        if !is_telemetry_enabled() {
            return;
        }
        self.inner.tokens_input.fetch_add(input, Ordering::Relaxed);
        self.inner
            .tokens_output
            .fetch_add(output, Ordering::Relaxed);
    }

    /// Record a session start.
    pub fn record_session_start(&self) {
        if !is_telemetry_enabled() {
            return;
        }
        self.inner.sessions_started.fetch_add(1, Ordering::Relaxed);
    }

    /// Get a formatted summary of all metrics.
    pub fn summary(&self) -> String {
        let api_calls = self.inner.api_calls.load(Ordering::Relaxed);
        let api_errors = self.inner.api_errors.load(Ordering::Relaxed);
        let api_latency_sum = self.inner.api_latency_ms_sum.load(Ordering::Relaxed);
        let tool_execs = self.inner.tool_executions.load(Ordering::Relaxed);
        let tool_errors = self.inner.tool_errors.load(Ordering::Relaxed);
        let tool_latency_sum = self.inner.tool_latency_ms_sum.load(Ordering::Relaxed);
        let tokens_in = self.inner.tokens_input.load(Ordering::Relaxed);
        let tokens_out = self.inner.tokens_output.load(Ordering::Relaxed);
        let sessions = self.inner.sessions_started.load(Ordering::Relaxed);

        let api_avg = api_latency_sum.checked_div(api_calls).unwrap_or(0);
        let tool_avg = tool_latency_sum.checked_div(tool_execs).unwrap_or(0);
        #[allow(clippy::cast_precision_loss)]
        let error_rate = if api_calls > 0 {
            (api_errors as f64 / api_calls as f64) * 100.0
        } else {
            0.0
        };

        format!(
            "Telemetry: {}\n\
             \n\
             API Calls:       {api_calls}\n\
             API Errors:      {api_errors} ({error_rate:.1}%)\n\
             API Avg Latency: {api_avg}ms\n\
             \n\
             Tool Executions: {tool_execs}\n\
             Tool Errors:     {tool_errors}\n\
             Tool Avg Latency: {tool_avg}ms\n\
             \n\
             Tokens In:       {tokens_in}\n\
             Tokens Out:      {tokens_out}\n\
             Total Tokens:    {}\n\
             \n\
             Sessions:        {sessions}",
            if is_telemetry_enabled() {
                "ENABLED"
            } else {
                "DISABLED"
            },
            tokens_in + tokens_out,
        )
    }

    /// Reset all counters.
    pub fn reset(&self) {
        self.inner.api_calls.store(0, Ordering::Relaxed);
        self.inner.api_errors.store(0, Ordering::Relaxed);
        self.inner.api_latency_ms_sum.store(0, Ordering::Relaxed);
        self.inner.tool_executions.store(0, Ordering::Relaxed);
        self.inner.tool_errors.store(0, Ordering::Relaxed);
        self.inner.tool_latency_ms_sum.store(0, Ordering::Relaxed);
        self.inner.tokens_input.store(0, Ordering::Relaxed);
        self.inner.tokens_output.store(0, Ordering::Relaxed);
        self.inner.sessions_started.store(0, Ordering::Relaxed);
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// A simple span guard that records latency on drop.
pub struct SpanGuard {
    start: Instant,
    kind: SpanKind,
    collector: MetricsCollector,
    is_error: bool,
}

/// Kind of operation being traced.
#[derive(Debug, Clone, Copy)]
pub enum SpanKind {
    ApiCall,
    ToolExecution,
}

impl SpanGuard {
    /// Create a new span guard.
    pub fn new(kind: SpanKind, collector: &MetricsCollector) -> Self {
        Self {
            start: Instant::now(),
            kind,
            collector: collector.clone(),
            is_error: false,
        }
    }

    /// Mark this span as an error.
    pub fn set_error(&mut self) {
        self.is_error = true;
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        let latency_ms = self.start.elapsed().as_millis() as u64;
        match self.kind {
            SpanKind::ApiCall => self.collector.record_api_call(latency_ms, self.is_error),
            SpanKind::ToolExecution => self
                .collector
                .record_tool_execution(latency_ms, self.is_error),
        }
    }
}

/// Check if telemetry is globally enabled.
pub fn is_telemetry_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Enable or disable telemetry collection.
pub fn set_telemetry_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Toggle telemetry on/off. Returns the new state.
pub fn toggle_telemetry() -> bool {
    let prev = ENABLED.fetch_xor(true, Ordering::Relaxed);
    // fetch_xor returns the *previous* value; XOR with true flips the bit.
    !prev
}

/// Initialize telemetry from config. Call once at startup.
pub fn init(config: &TelemetryConfig) {
    set_telemetry_enabled(config.enabled);
    if config.enabled {
        tracing::info!(
            endpoint = %config.otlp_endpoint,
            service = %config.service_name,
            "Telemetry enabled"
        );
    }
}

// ---------------------------------------------------------------------------
// OTLP layer composition
// ---------------------------------------------------------------------------

/// Compose an OTLP tracing layer onto `subscriber` when the `telemetry-otlp`
/// feature is enabled.
///
/// With the feature enabled the returned subscriber wraps the input with an
/// OpenTelemetry layer that batches spans to the configured OTLP gRPC endpoint.
/// Without the feature the input subscriber is returned unchanged, preserving
/// the default file-based tracing behaviour.
#[cfg(feature = "telemetry-otlp")]
pub fn install_otlp_layer<S>(subscriber: S) -> impl tracing::Subscriber + Send + Sync
where
    S: tracing::Subscriber
        + Send
        + Sync
        + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    use tracing_subscriber::layer::SubscriberExt;
    let maybe_layer = crate::telemetry_pipeline::otlp::build_layer::<S>();
    subscriber.with(maybe_layer)
}

/// No-op pass-through when `telemetry-otlp` feature is disabled.
#[cfg(not(feature = "telemetry-otlp"))]
pub fn install_otlp_layer<S>(subscriber: S) -> S {
    subscriber
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector() {
        let collector = MetricsCollector::new();
        // Enable telemetry for the test.
        set_telemetry_enabled(true);

        collector.record_api_call(100, false);
        collector.record_api_call(200, true);
        collector.record_tool_execution(50, false);
        collector.record_tokens(1000, 500);
        collector.record_session_start();

        let summary = collector.summary();
        assert!(summary.contains("API Calls:       2"));
        assert!(summary.contains("API Errors:      1"));
        assert!(summary.contains("Tool Executions: 1"));
        assert!(summary.contains("Tokens In:       1000"));
        assert!(summary.contains("Tokens Out:      500"));

        // Restore default.
        set_telemetry_enabled(false);
    }

    #[test]
    fn test_metrics_disabled() {
        let collector = MetricsCollector::new();
        set_telemetry_enabled(false);

        collector.record_api_call(100, false);
        let summary = collector.summary();
        assert!(summary.contains("API Calls:       0"));
    }

    #[test]
    fn test_toggle() {
        set_telemetry_enabled(false);
        let new_state = toggle_telemetry();
        assert!(new_state);
        let new_state = toggle_telemetry();
        assert!(!new_state);
    }

    #[test]
    fn test_span_guard() {
        let collector = MetricsCollector::new();
        set_telemetry_enabled(true);

        {
            let _span = SpanGuard::new(SpanKind::ApiCall, &collector);
            // Simulates work.
        }

        let summary = collector.summary();
        assert!(summary.contains("API Calls:       1"));
        set_telemetry_enabled(false);
    }

    #[test]
    fn test_default_config() {
        let config = TelemetryConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.otlp_endpoint, DEFAULT_OTLP_ENDPOINT);
    }

    #[test]
    fn test_metrics_reset() {
        let collector = MetricsCollector::new();
        set_telemetry_enabled(true);
        collector.record_api_call(100, false);
        collector.reset();
        let summary = collector.summary();
        assert!(summary.contains("API Calls:       0"));
        set_telemetry_enabled(false);
    }
}
