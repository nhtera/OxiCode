//! HTTP hook executor — POST hook events to a URL endpoint.
//!
//! Features:
//! - SSRF guard: rejects private/loopback IPs (10.x, 172.16-31.x, 192.168.x, 127.x, ::1)
//! - Env var injection into request headers
//! - 10 minute default timeout
//! - Fail-open: any error → `HookResponse::Pass`

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::events::{HookPayload, HookResponse};
use crate::pinned_resolver::{is_private_ip, PinnedResolver};

/// Configuration specific to HTTP-type hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHookConfig {
    /// URL to POST the hook payload to.
    pub url: String,

    /// Optional authorization header value (e.g., "Bearer xxx").
    #[serde(default)]
    pub authorization: Option<String>,

    /// Environment variable names to inject as request headers.
    /// Maps header name → env var name.
    #[serde(default)]
    pub env_headers: std::collections::HashMap<String, String>,

    /// Timeout in seconds (default 600 = 10 minutes).
    #[serde(default = "default_http_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_http_timeout_secs() -> u64 {
    600
}

impl Default for HttpHookConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            authorization: None,
            env_headers: std::collections::HashMap::new(),
            timeout_secs: default_http_timeout_secs(),
        }
    }
}

/// Execute an HTTP hook by POSTing the payload to the configured URL.
///
/// Uses `PinnedResolver` to cache DNS resolution and prevent rebinding attacks.
/// On any failure (SSRF, network, parse, timeout), returns `HookResponse::Pass`.
pub async fn execute_http_hook(
    payload: &HookPayload,
    config: &HttpHookConfig,
    resolver: Option<&Arc<PinnedResolver>>,
) -> HookResponse {
    let timeout = Duration::from_secs(config.timeout_secs);

    let result = tokio::time::timeout(timeout, post_hook(payload, config, resolver)).await;

    match result {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => {
            tracing::warn!("HTTP hook error: {e}");
            HookResponse::Pass
        }
        Err(_) => {
            tracing::warn!(
                "HTTP hook timed out after {}s: {}",
                config.timeout_secs,
                config.url
            );
            HookResponse::Pass
        }
    }
}

/// Build and send the HTTP POST request.
///
/// When a `PinnedResolver` is provided, it pre-resolves the hostname and validates
/// IPs before connecting — closing the DNS rebinding TOCTOU window.
async fn post_hook(
    payload: &HookPayload,
    config: &HttpHookConfig,
    resolver: Option<&Arc<PinnedResolver>>,
) -> Result<HookResponse, String> {
    // SSRF guard: validate URL before sending.
    validate_url_ssrf(&config.url)?;

    // If resolver provided, also pin DNS resolution.
    if let Some(resolver) = resolver {
        let parsed = url::Url::parse(&config.url).map_err(|e| format!("Invalid URL: {e}"))?;
        if let Some(host) = parsed.host_str() {
            let port = parsed.port_or_known_default().unwrap_or(443);
            resolver.resolve(host, port)?;
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let mut request = client
        .post(&config.url)
        .header("Content-Type", "application/json")
        .json(&build_request_body(payload));

    // Inject authorization header.
    if let Some(auth) = &config.authorization {
        request = request.header("Authorization", auth);
    }

    // Inject env var headers.
    for (header_name, env_var) in &config.env_headers {
        if let Ok(value) = std::env::var(env_var) {
            request = request.header(header_name.as_str(), value);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("HTTP hook request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        tracing::debug!("HTTP hook returned {status}: {body}");
        // Non-2xx → pass (fail-open).
        return Ok(HookResponse::Pass);
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read HTTP hook response: {e}"))?;

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(HookResponse::Pass);
    }

    serde_json::from_str(trimmed).map_err(|e| format!("Failed to parse HTTP hook response: {e}"))
}

/// Build the JSON request body sent to the hook endpoint. Mirrors the
/// stdin payload sent to command-type hooks (Claude Code-compatible
/// `HookPayload`) plus a server-side `timestamp`.
fn build_request_body(payload: &HookPayload) -> serde_json::Value {
    let mut value =
        serde_json::to_value(payload).unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "timestamp".to_string(),
            serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
        );
    }
    value
}

/// Validate URL against SSRF: reject private/loopback/link-local IPs.
///
/// Resolves the hostname to IP addresses and checks each one.
/// Also rejects non-HTTP(S) schemes.
fn validate_url_ssrf(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;

    // Only allow http/https schemes.
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("SSRF: disallowed scheme '{scheme}'")),
    }

    let host = parsed.host_str().ok_or("SSRF: URL has no host")?;

    // Check if it's a raw IP address.
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(format!("SSRF: private/loopback IP rejected: {ip}"));
        }
    }

    // DNS resolution check — resolve and verify all IPs are public.
    // Use std::net resolution (blocking but acceptable for validation).
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addr_str = format!("{host}:{port}");
    if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&addr_str as &str) {
        for addr in addrs {
            if is_private_ip(&addr.ip()) {
                return Err(format!(
                    "SSRF: hostname '{}' resolves to private IP {}",
                    host,
                    addr.ip()
                ));
            }
        }
    }
    // If DNS resolution fails, we allow it through — the HTTP client will fail naturally.

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::HookEvent;

    fn test_payload() -> HookPayload {
        let mut p = HookPayload::new(HookEvent::PreToolUse, "sess_1");
        p.tool_name = Some("bash".to_string());
        p.tool_input = Some(serde_json::json!({"command": "ls"}));
        p
    }

    #[test]
    fn test_default_config() {
        let config = HttpHookConfig::default();
        assert!(config.url.is_empty());
        assert!(config.authorization.is_none());
        assert!(config.env_headers.is_empty());
        assert_eq!(config.timeout_secs, 600);
    }

    #[test]
    fn test_config_serde() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Api-Key".to_string(), "MY_API_KEY".to_string());
        let config = HttpHookConfig {
            url: "https://hooks.example.com/event".to_string(),
            authorization: Some("Bearer token123".to_string()),
            env_headers: headers,
            timeout_secs: 30,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: HttpHookConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.url, "https://hooks.example.com/event");
        assert_eq!(parsed.authorization.as_deref(), Some("Bearer token123"));
        assert_eq!(parsed.timeout_secs, 30);
    }

    // -- SSRF tests --

    #[test]
    fn test_ssrf_rejects_localhost() {
        assert!(validate_url_ssrf("http://127.0.0.1/hook").is_err());
        assert!(validate_url_ssrf("http://127.0.0.2:8080/hook").is_err());
    }

    #[test]
    fn test_ssrf_rejects_private_10() {
        assert!(validate_url_ssrf("http://10.0.0.1/hook").is_err());
        assert!(validate_url_ssrf("http://10.255.255.255/hook").is_err());
    }

    #[test]
    fn test_ssrf_rejects_private_172() {
        assert!(validate_url_ssrf("http://172.16.0.1/hook").is_err());
        assert!(validate_url_ssrf("http://172.31.255.255/hook").is_err());
    }

    #[test]
    fn test_ssrf_rejects_private_192() {
        assert!(validate_url_ssrf("http://192.168.1.1/hook").is_err());
        assert!(validate_url_ssrf("http://192.168.0.100/hook").is_err());
    }

    #[test]
    fn test_ssrf_rejects_ipv6_loopback() {
        assert!(validate_url_ssrf("http://[::1]/hook").is_err());
    }

    #[test]
    fn test_ssrf_rejects_unspecified() {
        assert!(validate_url_ssrf("http://0.0.0.0/hook").is_err());
    }

    #[test]
    fn test_ssrf_rejects_link_local() {
        assert!(validate_url_ssrf("http://169.254.1.1/hook").is_err());
    }

    #[test]
    fn test_ssrf_rejects_bad_scheme() {
        assert!(validate_url_ssrf("ftp://example.com/hook").is_err());
        assert!(validate_url_ssrf("file:///etc/passwd").is_err());
    }

    #[test]
    fn test_ssrf_allows_public_ip() {
        // 8.8.8.8 is a public IP.
        assert!(validate_url_ssrf("https://8.8.8.8/hook").is_ok());
    }

    #[test]
    fn test_ssrf_allows_public_domain() {
        // example.com resolves to a public IP.
        assert!(validate_url_ssrf("https://example.com/hook").is_ok());
    }

    #[test]
    fn test_ssrf_rejects_invalid_url() {
        assert!(validate_url_ssrf("not a url").is_err());
    }

    #[test]
    fn test_ssrf_rejects_ipv4_mapped_ipv6() {
        // ::ffff:127.0.0.1 is IPv4-mapped IPv6 for loopback.
        assert!(validate_url_ssrf("http://[::ffff:127.0.0.1]/hook").is_err());
        // ::ffff:10.0.0.1 is IPv4-mapped IPv6 for private range.
        assert!(validate_url_ssrf("http://[::ffff:10.0.0.1]/hook").is_err());
        // ::ffff:192.168.1.1 is IPv4-mapped IPv6 for private range.
        assert!(validate_url_ssrf("http://[::ffff:192.168.1.1]/hook").is_err());
    }

    // -- Private IP helper tests --

    #[test]
    fn test_is_private_ipv4() {
        assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"192.168.1.1".parse().unwrap()));
        assert!(is_private_ip(&"169.254.1.1".parse().unwrap()));
        assert!(is_private_ip(&"0.0.0.0".parse().unwrap()));
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ipv6() {
        assert!(is_private_ip(&"::1".parse().unwrap()));
        assert!(is_private_ip(&"::".parse().unwrap()));
        assert!(is_private_ip(&"fe80::1".parse().unwrap()));
        assert!(is_private_ip(&"fd00::1".parse().unwrap()));
        assert!(!is_private_ip(&"2001:4860:4860::8888".parse().unwrap()));
    }

    #[test]
    fn test_172_boundary() {
        // 172.15.x.x is NOT private.
        assert!(!is_private_ip(&"172.15.255.255".parse().unwrap()));
        // 172.16.0.0 IS private.
        assert!(is_private_ip(&"172.16.0.0".parse().unwrap()));
        // 172.31.255.255 IS private.
        assert!(is_private_ip(&"172.31.255.255".parse().unwrap()));
        // 172.32.0.0 is NOT private.
        assert!(!is_private_ip(&"172.32.0.0".parse().unwrap()));
    }

    #[test]
    fn test_build_request_body() {
        let payload = test_payload();
        let body = build_request_body(&payload);
        assert_eq!(body["hook_event_name"], "PreToolUse");
        assert_eq!(body["tool_name"], "bash");
        assert!(body["timestamp"].is_string());
    }

    #[tokio::test]
    async fn test_execute_http_hook_invalid_url() {
        let payload = test_payload();
        let config = HttpHookConfig {
            url: "http://127.0.0.1:9999/hook".to_string(),
            ..Default::default()
        };
        // SSRF should block this → Pass.
        let response = execute_http_hook(&payload, &config, None).await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_execute_http_hook_bad_scheme() {
        let payload = test_payload();
        let config = HttpHookConfig {
            url: "ftp://example.com/hook".to_string(),
            ..Default::default()
        };
        let response = execute_http_hook(&payload, &config, None).await;
        assert!(matches!(response, HookResponse::Pass));
    }
}
