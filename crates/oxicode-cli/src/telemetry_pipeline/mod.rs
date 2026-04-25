//! Telemetry pipeline — event collection, local file logging, and OTLP export.
//!
//! Extends the base `telemetry` module with:
//! - Structured telemetry events (session, tool, model, error, compaction)
//! - Local NDJSON file logger (always on, no network)
//! - OTLP exporter (opt-in, feature-gated behind `telemetry-otlp`)
//! - Remote killswitch support

pub mod event_collector;
pub mod local_file_logger;

#[cfg(feature = "telemetry-otlp")]
pub mod otlp_exporter;

/// OTLP tracing layer (tracing-opentelemetry integration).
#[cfg(feature = "telemetry-otlp")]
pub mod otlp;
