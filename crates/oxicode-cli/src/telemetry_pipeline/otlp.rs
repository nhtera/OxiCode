//! OTLP tracing layer — builds an OpenTelemetry tracing-subscriber layer.
//!
//! Feature-gated behind `--features telemetry-otlp`. Uses the official
//! `opentelemetry-otlp` crate with tonic gRPC transport and a batch span
//! processor. The layer is composed into the tracing-subscriber stack by
//! `telemetry::install_otlp_layer`.
//!
//! Environment variables (all tolerant of missing/malformed values):
//! - `OXICODE_OTLP_ENDPOINT`    gRPC endpoint (default: http://localhost:4317)
//! - `OXICODE_OTLP_HEADERS`     Comma-separated key=value pairs for auth/routing
//!                               (values are NEVER logged for security)
//! - `OXICODE_OTLP_SAMPLE_RATE` Float in [0.0, 1.0], default 0.1 (10%)

use std::collections::HashMap;

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{RandomIdGenerator, Sampler, TracerProvider};
use tonic::metadata::{Ascii, MetadataKey, MetadataMap, MetadataValue};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::Layer;

/// Default gRPC OTLP endpoint.
const DEFAULT_ENDPOINT: &str = "http://localhost:4317";

/// Default trace sample rate (10%).
const DEFAULT_SAMPLE_RATE: f64 = 0.1;

/// Build an OpenTelemetry tracing layer reading config from environment variables.
///
/// Returns `None` if the exporter cannot be initialised (logs the reason and
/// falls back gracefully — the agentic loop is never blocked).
///
/// # Security
/// `OXICODE_OTLP_HEADERS` values are parsed but never logged.
pub fn build_layer<S>() -> Option<Box<dyn Layer<S> + Send + Sync + 'static>>
where
    S: tracing::Subscriber
        + Send
        + Sync
        + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    let endpoint = read_endpoint();
    let headers = read_headers();
    let sample_rate = read_sample_rate();

    tracing::info!(
        endpoint = %endpoint,
        sample_rate = %sample_rate,
        "OTLP telemetry layer initialising"
    );

    let mut exporter_builder = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint);

    // Apply auth/routing headers if provided. Values are NOT logged.
    if !headers.is_empty() {
        exporter_builder = exporter_builder.with_metadata(build_tonic_metadata(&headers));
    }

    let exporter = match exporter_builder.build() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "OTLP exporter build failed; telemetry-otlp disabled");
            return None;
        }
    };

    // Batch processor — async export via tokio runtime, never blocks the main loop.
    let batch_processor =
        opentelemetry_sdk::trace::BatchSpanProcessor::builder(
            exporter,
            opentelemetry_sdk::runtime::Tokio,
        )
        .build();

    let resource = Resource::new(vec![KeyValue::new("service.name", "oxicode-cli")]);

    let provider = TracerProvider::builder()
        .with_span_processor(batch_processor)
        .with_sampler(Sampler::TraceIdRatioBased(sample_rate))
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource)
        .build();

    // Obtain the SDK Tracer directly from the provider.
    // `provider.tracer()` returns `opentelemetry_sdk::trace::Tracer` which
    // implements `PreSampledTracer` as required by `OpenTelemetryLayer::new`.
    let tracer = provider.tracer("oxicode");

    // Register the provider globally so spans are picked up by OTel APIs elsewhere.
    opentelemetry::global::set_tracer_provider(provider);

    Some(Box::new(OpenTelemetryLayer::new(tracer)))
}

/// Read OTLP gRPC endpoint from environment, falling back to the default.
fn read_endpoint() -> String {
    match std::env::var("OXICODE_OTLP_ENDPOINT") {
        Ok(v) if !v.trim().is_empty() => v,
        Ok(_) => {
            tracing::debug!("OXICODE_OTLP_ENDPOINT is empty, using default");
            DEFAULT_ENDPOINT.to_string()
        }
        Err(_) => DEFAULT_ENDPOINT.to_string(),
    }
}

/// Parse optional headers from `OXICODE_OTLP_HEADERS` (key=value,key=value).
/// Values are deliberately NOT logged.
fn read_headers() -> HashMap<String, String> {
    let raw = match std::env::var("OXICODE_OTLP_HEADERS") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return HashMap::new(),
    };

    let mut map = HashMap::new();
    for pair in raw.split(',') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            let key = k.trim().to_string();
            if key.is_empty() {
                tracing::debug!("Skipping malformed OTLP header entry (empty key)");
            } else {
                map.insert(key, v.to_string()); // value preserved verbatim, never logged
            }
        } else if !pair.is_empty() {
            tracing::debug!("Skipping malformed OTLP header entry (missing '=')");
        }
    }
    map
}

/// Parse sample rate from `OXICODE_OTLP_SAMPLE_RATE`. Falls back to default on error.
fn read_sample_rate() -> f64 {
    match std::env::var("OXICODE_OTLP_SAMPLE_RATE") {
        Ok(v) => match v.trim().parse::<f64>() {
            Ok(r) if (0.0..=1.0).contains(&r) => r,
            Ok(r) => {
                tracing::warn!(value = %r, "OXICODE_OTLP_SAMPLE_RATE out of [0,1]; using default");
                DEFAULT_SAMPLE_RATE
            }
            Err(_) => {
                tracing::warn!(raw = %v, "OXICODE_OTLP_SAMPLE_RATE is not a float; using default");
                DEFAULT_SAMPLE_RATE
            }
        },
        Err(_) => DEFAULT_SAMPLE_RATE,
    }
}

/// Convert the headers map into a tonic `MetadataMap`.
/// Invalid keys or non-ASCII values are skipped with a warning (values not logged).
fn build_tonic_metadata(headers: &HashMap<String, String>) -> MetadataMap {
    let mut meta = MetadataMap::new();
    for (k, v) in headers {
        let Ok(key) = k.parse::<MetadataKey<Ascii>>() else {
            tracing::warn!(key = %k, "Skipping OTLP header with invalid key");
            continue;
        };
        // Do NOT log the value — it may contain bearer tokens or API keys.
        let Ok(val) = v.parse::<MetadataValue<Ascii>>() else {
            tracing::warn!(key = %k, "Skipping OTLP header with non-ASCII value");
            continue;
        };
        meta.insert(key, val);
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_sample_rate_default() {
        std::env::remove_var("OXICODE_OTLP_SAMPLE_RATE");
        assert!((read_sample_rate() - DEFAULT_SAMPLE_RATE).abs() < f64::EPSILON);
    }

    #[test]
    fn test_read_sample_rate_valid() {
        std::env::set_var("OXICODE_OTLP_SAMPLE_RATE", "0.5");
        assert!((read_sample_rate() - 0.5).abs() < f64::EPSILON);
        std::env::remove_var("OXICODE_OTLP_SAMPLE_RATE");
    }

    #[test]
    fn test_read_sample_rate_out_of_range() {
        std::env::set_var("OXICODE_OTLP_SAMPLE_RATE", "2.0");
        assert!((read_sample_rate() - DEFAULT_SAMPLE_RATE).abs() < f64::EPSILON);
        std::env::remove_var("OXICODE_OTLP_SAMPLE_RATE");
    }

    #[test]
    fn test_read_sample_rate_invalid_string() {
        std::env::set_var("OXICODE_OTLP_SAMPLE_RATE", "not_a_number");
        assert!((read_sample_rate() - DEFAULT_SAMPLE_RATE).abs() < f64::EPSILON);
        std::env::remove_var("OXICODE_OTLP_SAMPLE_RATE");
    }

    #[test]
    fn test_read_headers_empty_env() {
        std::env::remove_var("OXICODE_OTLP_HEADERS");
        assert!(read_headers().is_empty());
    }

    #[test]
    fn test_read_headers_parses_valid_pairs() {
        std::env::set_var("OXICODE_OTLP_HEADERS", "x-team=eng,x-env=prod");
        let h = read_headers();
        assert_eq!(h.get("x-team").map(String::as_str), Some("eng"));
        assert_eq!(h.get("x-env").map(String::as_str), Some("prod"));
        std::env::remove_var("OXICODE_OTLP_HEADERS");
    }

    #[test]
    fn test_read_headers_skips_malformed() {
        std::env::set_var("OXICODE_OTLP_HEADERS", "valid=yes,no_separator,=emptykey");
        let h = read_headers();
        assert_eq!(h.get("valid").map(String::as_str), Some("yes"));
        assert!(!h.contains_key("no_separator"));
        assert!(!h.contains_key(""));
        std::env::remove_var("OXICODE_OTLP_HEADERS");
    }

    #[test]
    fn test_read_endpoint_default() {
        std::env::remove_var("OXICODE_OTLP_ENDPOINT");
        assert_eq!(read_endpoint(), DEFAULT_ENDPOINT);
    }

    #[test]
    fn test_read_endpoint_from_env() {
        std::env::set_var("OXICODE_OTLP_ENDPOINT", "http://otel-collector:4317");
        assert_eq!(read_endpoint(), "http://otel-collector:4317");
        std::env::remove_var("OXICODE_OTLP_ENDPOINT");
    }
}
