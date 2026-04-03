use crate::types::RateLimitInfo;
use thiserror::Error;

/// Central error type for `OxiCode`.
#[derive(Debug, Error)]
pub enum OxiError {
    #[error("API error: {message}")]
    Api {
        message: String,
        status: Option<u16>,
        retryable: bool,
    },

    #[error("Rate limited: {}", info.message)]
    RateLimit { info: RateLimitInfo },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Tool error: {name} — {message}")]
    Tool { name: String, message: String },

    #[error("Permission denied: {0}")]
    Permission(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TUI error: {0}")]
    Tui(String),

    #[error("Stream closed unexpectedly")]
    StreamClosed,

    #[error("{0}")]
    Other(String),
}

impl OxiError {
    pub fn api(message: impl Into<String>) -> Self {
        Self::Api {
            message: message.into(),
            status: None,
            retryable: false,
        }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn api_with_status(message: impl Into<String>, status: u16) -> Self {
        let retryable = matches!(status, 429 | 500 | 502 | 503 | 529);
        Self::Api {
            message: message.into(),
            status: Some(status),
            retryable,
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Api {
                retryable: true,
                ..
            } | Self::RateLimit { .. }
        )
    }

    /// Extract rate limit info if this is a RateLimit error.
    pub fn rate_limit_info(&self) -> Option<&RateLimitInfo> {
        match self {
            Self::RateLimit { info } => Some(info),
            _ => None,
        }
    }
}

/// Convenience Result type for `OxiCode`.
pub type OxiResult<T> = Result<T, OxiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RateLimitType;

    #[test]
    fn test_rate_limit_error_is_retryable() {
        let info = RateLimitInfo {
            retry_after_secs: Some(30.0),
            limit_type: RateLimitType::TokensPerMinute,
            ..Default::default()
        };
        let err = OxiError::RateLimit { info };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_rate_limit_info_accessor() {
        let info = RateLimitInfo {
            retry_after_secs: Some(45.0),
            limit_type: RateLimitType::RequestsPerMinute,
            remaining: Some(50),
            ..Default::default()
        };
        let err = OxiError::RateLimit { info: info.clone() };

        let extracted = err.rate_limit_info();
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap().retry_after_secs, Some(45.0));
        assert_eq!(
            extracted.unwrap().limit_type,
            RateLimitType::RequestsPerMinute
        );
        assert_eq!(extracted.unwrap().remaining, Some(50));
    }

    #[test]
    fn test_rate_limit_info_accessor_on_non_rate_limit_error() {
        let err = OxiError::api("some error");
        assert!(err.rate_limit_info().is_none());
    }

    #[test]
    fn test_429_status_creates_retryable_error() {
        let err = OxiError::api_with_status("Too many requests", 429);
        assert!(err.is_retryable());
        match err {
            OxiError::Api {
                status,
                retryable,
                ..
            } => {
                assert_eq!(status, Some(429));
                assert!(retryable);
            }
            _ => panic!("Expected Api error"),
        }
    }

    #[test]
    fn test_500_status_creates_retryable_error() {
        let err = OxiError::api_with_status("Server error", 500);
        assert!(err.is_retryable());
    }

    #[test]
    fn test_502_status_creates_retryable_error() {
        let err = OxiError::api_with_status("Bad gateway", 502);
        assert!(err.is_retryable());
    }

    #[test]
    fn test_503_status_creates_retryable_error() {
        let err = OxiError::api_with_status("Service unavailable", 503);
        assert!(err.is_retryable());
    }

    #[test]
    fn test_529_status_creates_retryable_error() {
        let err = OxiError::api_with_status("Overloaded", 529);
        assert!(err.is_retryable());
    }

    #[test]
    fn test_400_status_not_retryable() {
        let err = OxiError::api_with_status("Bad request", 400);
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_401_status_not_retryable() {
        let err = OxiError::api_with_status("Unauthorized", 401);
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_non_api_error_not_retryable() {
        let err = OxiError::config("config error");
        assert!(!err.is_retryable());

        let err = OxiError::Permission("denied".to_string());
        assert!(!err.is_retryable());

        let err = OxiError::StreamClosed;
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_error_display_format() {
        let info = RateLimitInfo {
            retry_after_secs: Some(30.0),
            limit_type: RateLimitType::TokensPerMinute,
            message: "Rate limited (tokens/min). Retry after 30s".to_string(),
            ..Default::default()
        };
        let err = OxiError::RateLimit { info };
        let msg = err.to_string();
        assert!(msg.contains("Rate limited"));
        assert!(msg.contains("tokens/min"));
    }
}
