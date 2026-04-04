//! Integration tests for rate limiting with header parsing.
//! Tests the rate_limit_headers module for parsing 429 responses from
//! various LLM providers (Anthropic, OpenAI-compatible).

use oxicode_api::rate_limit_headers;
use oxicode_common::{RateLimitInfo, RateLimitType};
use reqwest::header::{HeaderMap, HeaderValue};

/// Test that OpenAI header parsing correctly detects TokensPerMinute limit.
#[test]
fn test_parse_openai_tokens_limit() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ratelimit-remaining-tokens",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "x-ratelimit-reset-tokens",
        HeaderValue::from_static("2026-04-04T12:00:30Z"),
    );
    headers.insert("retry-after", HeaderValue::from_static("60"));

    let info = rate_limit_headers::parse_openai_headers(&headers);

    assert_eq!(info.limit_type, RateLimitType::TokensPerMinute);
    assert_eq!(info.retry_after_secs, Some(60.0));
    assert_eq!(info.remaining, Some(0));
    assert!(info.reset_at.is_some());
}

/// Test that OpenAI header parsing correctly detects RequestsPerMinute limit.
#[test]
fn test_parse_openai_requests_limit() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ratelimit-remaining-requests",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "x-ratelimit-reset-requests",
        HeaderValue::from_static("2026-04-04T12:00:45Z"),
    );
    headers.insert("retry-after", HeaderValue::from_static("45"));

    let info = rate_limit_headers::parse_openai_headers(&headers);

    assert_eq!(info.limit_type, RateLimitType::RequestsPerMinute);
    assert_eq!(info.retry_after_secs, Some(45.0));
    assert_eq!(info.remaining, Some(0));
}

/// Test that Anthropic header parsing correctly detects tokens limit.
#[test]
fn test_parse_anthropic_tokens_limit() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "anthropic-ratelimit-tokens-remaining",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "anthropic-ratelimit-tokens-reset",
        HeaderValue::from_static("2026-04-04T12:00:00Z"),
    );
    headers.insert("retry-after", HeaderValue::from_static("30"));

    let info = rate_limit_headers::parse_anthropic_headers(&headers);

    assert_eq!(info.limit_type, RateLimitType::TokensPerMinute);
    assert_eq!(info.retry_after_secs, Some(30.0));
    assert_eq!(info.remaining, Some(0));
}

/// Test that Anthropic header parsing correctly detects requests limit.
#[test]
fn test_parse_anthropic_requests_limit() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "anthropic-ratelimit-requests-remaining",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "anthropic-ratelimit-requests-reset",
        HeaderValue::from_static("2026-04-04T12:01:00Z"),
    );

    let info = rate_limit_headers::parse_anthropic_headers(&headers);

    assert_eq!(info.limit_type, RateLimitType::RequestsPerMinute);
    assert_eq!(info.remaining, Some(0));
}

/// Test that Anthropic input tokens limit is detected correctly.
#[test]
fn test_parse_anthropic_input_tokens_limit() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "anthropic-ratelimit-input-tokens-remaining",
        HeaderValue::from_static("0"),
    );

    let info = rate_limit_headers::parse_anthropic_headers(&headers);

    assert_eq!(info.limit_type, RateLimitType::InputTokensPerMinute);
}

/// Test that Anthropic output tokens limit is detected correctly.
#[test]
fn test_parse_anthropic_output_tokens_limit() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "anthropic-ratelimit-output-tokens-remaining",
        HeaderValue::from_static("0"),
    );

    let info = rate_limit_headers::parse_anthropic_headers(&headers);

    assert_eq!(info.limit_type, RateLimitType::OutputTokensPerMinute);
}

/// Test that retry-after with integer seconds is parsed correctly.
/// Note: HTTP date (RFC 2822) parsing is tested in rate_limit_headers unit tests.
#[test]
fn test_retry_after_seconds() {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_static("45"));

    let info = rate_limit_headers::parse_openai_headers(&headers);
    assert_eq!(info.retry_after_secs, Some(45.0));
}

/// Test that missing rate limit headers result in Unknown limit type.
#[test]
fn test_missing_headers_unknown_limit() {
    let headers = HeaderMap::new();

    let info_openai = rate_limit_headers::parse_openai_headers(&headers);
    assert_eq!(info_openai.limit_type, RateLimitType::Unknown);

    let info_anthropic = rate_limit_headers::parse_anthropic_headers(&headers);
    assert_eq!(info_anthropic.limit_type, RateLimitType::Unknown);
}

/// Test that partial rate limit info is handled gracefully.
#[test]
fn test_partial_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_static("20"));

    let info = rate_limit_headers::parse_openai_headers(&headers);

    assert_eq!(info.retry_after_secs, Some(20.0));
    assert_eq!(info.limit_type, RateLimitType::Unknown);
    assert_eq!(info.remaining, None);
    assert_eq!(info.reset_at, None);
}

/// Test Anthropic limit type priority (tokens > requests > input > output).
#[test]
fn test_anthropic_limit_type_priority() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "anthropic-ratelimit-tokens-remaining",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "anthropic-ratelimit-requests-remaining",
        HeaderValue::from_static("0"),
    );

    let info = rate_limit_headers::parse_anthropic_headers(&headers);
    assert_eq!(info.limit_type, RateLimitType::TokensPerMinute);
}

/// Test OpenAI limit type priority (tokens > requests).
#[test]
fn test_openai_limit_type_priority() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ratelimit-remaining-tokens",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "x-ratelimit-remaining-requests",
        HeaderValue::from_static("0"),
    );

    let info = rate_limit_headers::parse_openai_headers(&headers);
    assert_eq!(info.limit_type, RateLimitType::TokensPerMinute);
}

/// Test that non-zero remaining values don't trigger limit detection.
#[test]
fn test_nonzero_remaining_unknown_limit() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ratelimit-remaining-tokens",
        HeaderValue::from_static("100"),
    );
    headers.insert(
        "x-ratelimit-reset-tokens",
        HeaderValue::from_static("2026-04-04T12:00:30Z"),
    );

    let info = rate_limit_headers::parse_openai_headers(&headers);
    assert_eq!(info.limit_type, RateLimitType::Unknown);
}

/// Test retry-after with fractional seconds.
#[test]
fn test_retry_after_fractional() {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_static("2.5"));

    let info = rate_limit_headers::parse_openai_headers(&headers);
    assert_eq!(info.retry_after_secs, Some(2.5));
}

/// Test invalid retry-after values are ignored gracefully.
#[test]
fn test_invalid_retry_after() {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_static("not-a-number"));

    let info = rate_limit_headers::parse_openai_headers(&headers);
    assert_eq!(info.retry_after_secs, None);
}

/// Test reset_at datetime parsing with ISO 8601.
#[test]
fn test_reset_at_iso8601() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ratelimit-remaining-tokens",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "x-ratelimit-reset-tokens",
        HeaderValue::from_static("2026-04-04T12:00:30Z"),
    );

    let info = rate_limit_headers::parse_openai_headers(&headers);
    assert!(info.reset_at.is_some());
}

/// Test reset_at with Anthropic ISO 8601 format.
#[test]
fn test_reset_at_anthropic_iso8601() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "anthropic-ratelimit-tokens-remaining",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "anthropic-ratelimit-tokens-reset",
        HeaderValue::from_static("2026-04-04T12:00:30Z"),
    );

    let info = rate_limit_headers::parse_anthropic_headers(&headers);
    assert!(info.reset_at.is_some());
}

/// Test complete RateLimitInfo with all fields populated.
#[test]
fn test_complete_rate_limit_info() {
    use chrono::Utc;

    let info = RateLimitInfo {
        retry_after_secs: Some(30.0),
        limit_type: RateLimitType::TokensPerMinute,
        remaining: Some(0),
        reset_at: Some(Utc::now()),
        message: "Rate limited (tokens/min)".to_string(),
    };

    assert_eq!(info.retry_after_secs, Some(30.0));
    assert_eq!(info.limit_type, RateLimitType::TokensPerMinute);
    assert_eq!(info.remaining, Some(0));
    assert!(info.reset_at.is_some());
}

/// Test Anthropic input tokens limit with explicit remaining header.
#[test]
fn test_anthropic_input_tokens_with_remaining() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "anthropic-ratelimit-input-tokens-remaining",
        HeaderValue::from_static("0"),
    );
    headers.insert(
        "anthropic-ratelimit-input-tokens-reset",
        HeaderValue::from_static("2026-04-04T12:00:00Z"),
    );

    let info = rate_limit_headers::parse_anthropic_headers(&headers);
    assert_eq!(info.limit_type, RateLimitType::InputTokensPerMinute);
}

/// Test that message is properly built from parsed headers.
#[test]
fn test_message_built_from_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ratelimit-remaining-tokens",
        HeaderValue::from_static("0"),
    );
    headers.insert("retry-after", HeaderValue::from_static("45"));

    let info = rate_limit_headers::parse_openai_headers(&headers);
    assert!(!info.message.is_empty());
    assert!(info.message.contains("Rate limited"));
}
