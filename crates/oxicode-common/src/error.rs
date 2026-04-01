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
            }
        )
    }
}

/// Convenience Result type for `OxiCode`.
pub type OxiResult<T> = Result<T, OxiError>;
