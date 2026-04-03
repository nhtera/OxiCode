//! Parse rate limit headers from provider HTTP responses.
//!
//! Anthropic returns `anthropic-ratelimit-*` headers + `retry-after`.
//! OpenAI-compat APIs return `x-ratelimit-*` headers + `retry-after`.

use chrono::{DateTime, Utc};
use oxicode_common::{RateLimitInfo, RateLimitType};

/// Parse rate limit headers from an Anthropic 429 response.
pub fn parse_anthropic_headers(headers: &reqwest::header::HeaderMap) -> RateLimitInfo {
    let retry_after = parse_retry_after(headers);

    // Anthropic headers: anthropic-ratelimit-{tokens,requests,input-tokens,output-tokens}-{limit,remaining,reset}
    let limit_type = detect_anthropic_limit_type(headers);
    let remaining = header_as_u64(headers, "anthropic-ratelimit-tokens-remaining")
        .or_else(|| header_as_u64(headers, "anthropic-ratelimit-requests-remaining"));
    let reset_at = header_as_datetime(headers, "anthropic-ratelimit-tokens-reset")
        .or_else(|| header_as_datetime(headers, "anthropic-ratelimit-requests-reset"));

    let message = build_rate_limit_message(retry_after, &limit_type);

    RateLimitInfo {
        retry_after_secs: retry_after,
        limit_type,
        remaining,
        reset_at,
        message,
    }
}

/// Parse rate limit headers from an OpenAI-compatible 429 response.
pub fn parse_openai_headers(headers: &reqwest::header::HeaderMap) -> RateLimitInfo {
    let retry_after = parse_retry_after(headers);

    // OpenAI headers: x-ratelimit-{limit,remaining,reset}-{tokens,requests}
    let limit_type = detect_openai_limit_type(headers);
    let remaining = header_as_u64(headers, "x-ratelimit-remaining-tokens")
        .or_else(|| header_as_u64(headers, "x-ratelimit-remaining-requests"));
    let reset_at = header_as_datetime(headers, "x-ratelimit-reset-tokens")
        .or_else(|| header_as_datetime(headers, "x-ratelimit-reset-requests"));

    let message = build_rate_limit_message(retry_after, &limit_type);

    RateLimitInfo {
        retry_after_secs: retry_after,
        limit_type,
        remaining,
        reset_at,
        message,
    }
}

/// Parse the `retry-after` header (seconds or HTTP-date).
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<f64> {
    let val = headers.get("retry-after")?.to_str().ok()?;
    // Try as seconds first.
    if let Ok(secs) = val.parse::<f64>() {
        return Some(secs);
    }
    // Try as HTTP-date (RFC 2616).
    if let Ok(date) = DateTime::parse_from_rfc2822(val) {
        let now = Utc::now();
        let diff = date.signed_duration_since(now);
        return Some(diff.num_seconds().max(0) as f64);
    }
    None
}

/// Detect which Anthropic rate limit was hit based on which remaining header is 0.
fn detect_anthropic_limit_type(headers: &reqwest::header::HeaderMap) -> RateLimitType {
    if header_as_u64(headers, "anthropic-ratelimit-tokens-remaining") == Some(0) {
        return RateLimitType::TokensPerMinute;
    }
    if header_as_u64(headers, "anthropic-ratelimit-requests-remaining") == Some(0) {
        return RateLimitType::RequestsPerMinute;
    }
    if header_as_u64(headers, "anthropic-ratelimit-input-tokens-remaining") == Some(0) {
        return RateLimitType::InputTokensPerMinute;
    }
    if header_as_u64(headers, "anthropic-ratelimit-output-tokens-remaining") == Some(0) {
        return RateLimitType::OutputTokensPerMinute;
    }
    RateLimitType::Unknown
}

/// Detect which OpenAI rate limit was hit based on which remaining header is 0.
fn detect_openai_limit_type(headers: &reqwest::header::HeaderMap) -> RateLimitType {
    if header_as_u64(headers, "x-ratelimit-remaining-tokens") == Some(0) {
        return RateLimitType::TokensPerMinute;
    }
    if header_as_u64(headers, "x-ratelimit-remaining-requests") == Some(0) {
        return RateLimitType::RequestsPerMinute;
    }
    RateLimitType::Unknown
}

/// Extract a header value as u64.
fn header_as_u64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

/// Extract a header value as a UTC datetime (ISO 8601 or RFC 2822).
fn header_as_datetime(headers: &reqwest::header::HeaderMap, name: &str) -> Option<DateTime<Utc>> {
    let val = headers.get(name)?.to_str().ok()?;
    // Try ISO 8601 first.
    if let Ok(dt) = val.parse::<DateTime<Utc>>() {
        return Some(dt);
    }
    // Try RFC 2822.
    DateTime::parse_from_rfc2822(val)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Build a human-readable message for TUI display.
fn build_rate_limit_message(retry_after: Option<f64>, limit_type: &RateLimitType) -> String {
    match retry_after {
        Some(secs) => format!("Rate limited ({limit_type}). Retry after {secs:.0}s"),
        None => format!("Rate limited ({limit_type})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn test_parse_retry_after_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("30"));
        assert_eq!(parse_retry_after(&headers), Some(30.0));
    }

    #[test]
    fn test_parse_retry_after_float() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("1.5"));
        assert_eq!(parse_retry_after(&headers), Some(1.5));
    }

    #[test]
    fn test_parse_retry_after_missing() {
        let headers = HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn test_anthropic_token_limit_detection() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-tokens-remaining",
            HeaderValue::from_static("0"),
        );
        headers.insert(
            "anthropic-ratelimit-tokens-reset",
            HeaderValue::from_static("2026-04-04T00:00:00Z"),
        );
        headers.insert("retry-after", HeaderValue::from_static("60"));

        let info = parse_anthropic_headers(&headers);
        assert_eq!(info.retry_after_secs, Some(60.0));
        assert_eq!(info.limit_type, RateLimitType::TokensPerMinute);
        assert_eq!(info.remaining, Some(0));
        assert!(info.reset_at.is_some());
    }

    #[test]
    fn test_anthropic_request_limit_detection() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-requests-remaining",
            HeaderValue::from_static("0"),
        );

        let info = parse_anthropic_headers(&headers);
        assert_eq!(info.limit_type, RateLimitType::RequestsPerMinute);
    }

    #[test]
    fn test_openai_token_limit_detection() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-ratelimit-remaining-tokens",
            HeaderValue::from_static("0"),
        );
        headers.insert("retry-after", HeaderValue::from_static("10"));

        let info = parse_openai_headers(&headers);
        assert_eq!(info.retry_after_secs, Some(10.0));
        assert_eq!(info.limit_type, RateLimitType::TokensPerMinute);
    }

    #[test]
    fn test_openai_request_limit_detection() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-ratelimit-remaining-requests",
            HeaderValue::from_static("0"),
        );

        let info = parse_openai_headers(&headers);
        assert_eq!(info.limit_type, RateLimitType::RequestsPerMinute);
    }

    #[test]
    fn test_unknown_limit_type_when_no_headers() {
        let headers = HeaderMap::new();
        let info = parse_anthropic_headers(&headers);
        assert_eq!(info.limit_type, RateLimitType::Unknown);
    }

    #[test]
    fn test_build_rate_limit_message_with_retry_after() {
        let msg = build_rate_limit_message(Some(30.0), &RateLimitType::TokensPerMinute);
        assert_eq!(msg, "Rate limited (tokens/min). Retry after 30s");
    }

    #[test]
    fn test_build_rate_limit_message_without_retry_after() {
        let msg = build_rate_limit_message(None, &RateLimitType::Unknown);
        assert_eq!(msg, "Rate limited (unknown)");
    }
}
