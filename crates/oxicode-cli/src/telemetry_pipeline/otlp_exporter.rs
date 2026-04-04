//! OTLP exporter — OpenTelemetry Protocol export for traces and metrics.
//!
//! Feature-gated behind `--features telemetry-otlp`. Sends batched telemetry
//! events to an OTLP-compatible backend (e.g. Jaeger, Grafana, Datadog).
//! Respects the remote killswitch — if active, no data is sent.

use super::event_collector::TelemetryEvent;

/// Default OTLP HTTP endpoint.
const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4318/v1/traces";

/// Batch flush interval in seconds.
const FLUSH_INTERVAL_SECS: u64 = 30;

/// OTLP exporter configuration.
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    /// OTLP HTTP endpoint URL.
    pub endpoint: String,
    /// Service name for trace attribution.
    pub service_name: String,
    /// Flush interval in seconds.
    pub flush_interval_secs: u64,
    /// Optional API key/token for the backend.
    pub api_key: Option<String>,
}

impl Default for OtlpConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_OTLP_ENDPOINT.to_string(),
            service_name: "oxicode-cli".to_string(),
            flush_interval_secs: FLUSH_INTERVAL_SECS,
            api_key: None,
        }
    }
}

/// OTLP batch exporter — accumulates events and sends in batches.
pub struct OtlpExporter {
    config: OtlpConfig,
    client: reqwest::Client,
}

impl OtlpExporter {
    /// Create a new OTLP exporter with the given config.
    pub fn new(config: OtlpConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Export a batch of telemetry events to the OTLP endpoint.
    ///
    /// Converts events to a simplified JSON payload and POSTs to the endpoint.
    /// Returns the number of events successfully exported.
    pub async fn export(&self, events: &[TelemetryEvent]) -> Result<usize, String> {
        if events.is_empty() {
            return Ok(0);
        }

        let payload = self.build_payload(events);

        let mut req = self
            .client
            .post(&self.config.endpoint)
            .header("Content-Type", "application/json")
            .json(&payload);

        if let Some(ref key) = self.config.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("OTLP export failed: {e}"))?;

        if resp.status().is_success() {
            tracing::debug!(count = events.len(), "OTLP export successful");
            Ok(events.len())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(format!("OTLP export error ({status}): {body}"))
        }
    }

    /// Build the JSON payload for OTLP export.
    /// Uses a simplified format — real OTLP protobuf can be added later.
    fn build_payload(&self, events: &[TelemetryEvent]) -> serde_json::Value {
        let spans: Vec<serde_json::Value> = events
            .iter()
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect();

        serde_json::json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": { "stringValue": self.config.service_name }
                    }]
                },
                "scopeSpans": [{
                    "scope": { "name": "oxicode.telemetry" },
                    "spans": spans
                }]
            }]
        })
    }

    /// Reference to the exporter config.
    pub fn config(&self) -> &OtlpConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::super::event_collector::now_unix_secs;
    use super::*;

    #[test]
    fn test_default_config() {
        let config = OtlpConfig::default();
        assert!(config.endpoint.contains("4318"));
        assert_eq!(config.service_name, "oxicode-cli");
        assert_eq!(config.flush_interval_secs, 30);
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_build_payload_structure() {
        let exporter = OtlpExporter::new(OtlpConfig::default());
        let events = vec![TelemetryEvent::SessionStart {
            session_id: "s1".to_string(),
            model: "test".to_string(),
            timestamp: now_unix_secs(),
        }];

        let payload = exporter.build_payload(&events);
        assert!(payload["resourceSpans"].is_array());
        let resource = &payload["resourceSpans"][0];
        assert!(resource["resource"]["attributes"][0]["key"]
            .as_str()
            .unwrap()
            .contains("service.name"));
    }

    #[test]
    fn test_build_payload_empty() {
        let exporter = OtlpExporter::new(OtlpConfig::default());
        let payload = exporter.build_payload(&[]);
        let spans = &payload["resourceSpans"][0]["scopeSpans"][0]["spans"];
        assert!(spans.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_exporter_config_ref() {
        let config = OtlpConfig {
            endpoint: "http://custom:4318".to_string(),
            ..Default::default()
        };
        let exporter = OtlpExporter::new(config);
        assert_eq!(exporter.config().endpoint, "http://custom:4318");
    }
}
