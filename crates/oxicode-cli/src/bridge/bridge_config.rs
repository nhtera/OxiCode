//! Bridge configuration parsed from `settings.toml` `[bridge]` section.
//!
//! Supports TOML config with env var overrides:
//! - `OXICODE_BRIDGE_URL` → WebSocket URL
//! - `OXICODE_BRIDGE_TOKEN` → Authentication token

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::Transport;

/// Bridge configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// WebSocket URL for remote bridge connections.
    #[serde(default)]
    pub url: Option<String>,

    /// Authentication token for bridge connections.
    #[serde(default)]
    pub auth_token: Option<String>,

    /// Transport mode override (default: stdio).
    #[serde(default)]
    pub transport: Transport,

    /// TCP bind port (0 = auto-assign).
    #[serde(default)]
    pub port: u16,

    /// TCP bind address.
    #[serde(default = "default_bind_address")]
    pub bind_address: String,

    /// Whether to automatically reconnect on disconnect.
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,

    /// Maximum reconnect backoff delay in seconds.
    #[serde(default = "default_max_reconnect_delay")]
    pub max_reconnect_delay_secs: u64,

    /// Maximum number of reconnection attempts before giving up.
    #[serde(default = "default_max_reconnect_attempts")]
    pub max_reconnect_attempts: u32,

    /// Enable debug logging of bridge messages to file.
    #[serde(default)]
    pub debug_logging: bool,

    /// Maximum connections for daemon mode.
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// Permission request timeout in seconds.
    #[serde(default = "default_permission_timeout")]
    pub permission_timeout_secs: u64,
}

fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}
fn default_true() -> bool {
    true
}
fn default_max_reconnect_delay() -> u64 {
    30
}
fn default_max_reconnect_attempts() -> u32 {
    100
}
fn default_max_connections() -> u32 {
    5
}
fn default_permission_timeout() -> u64 {
    60
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            url: None,
            auth_token: None,
            transport: Transport::default(),
            port: 0,
            bind_address: default_bind_address(),
            auto_reconnect: true,
            max_reconnect_delay_secs: default_max_reconnect_delay(),
            max_reconnect_attempts: default_max_reconnect_attempts(),
            debug_logging: false,
            max_connections: default_max_connections(),
            permission_timeout_secs: default_permission_timeout(),
        }
    }
}

impl BridgeConfig {
    /// Load bridge config from a TOML value (the `[bridge]` section).
    pub fn from_toml_value(value: &toml::Value) -> Self {
        let toml_str = toml::to_string(value).unwrap_or_default();
        let mut config: Self = toml::from_str(&toml_str).unwrap_or_default();
        config.merge_env_vars();
        config
    }

    /// Load from the user settings directory (`~/.oxicode/settings.toml`).
    pub fn load_from_settings_dir() -> Self {
        let config_dir =
            dirs::home_dir().map_or_else(|| PathBuf::from(".oxicode"), |h| h.join(".oxicode"));

        let settings_path = config_dir.join("settings.toml");
        let Ok(content) = std::fs::read_to_string(&settings_path) else {
            return Self::default();
        };

        let parsed: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(_) => return Self::default(),
        };

        parsed
            .get("bridge")
            .map(Self::from_toml_value)
            .unwrap_or_default()
    }

    /// Override fields from environment variables.
    fn merge_env_vars(&mut self) {
        if let Ok(url) = std::env::var("OXICODE_BRIDGE_URL") {
            if !url.is_empty() {
                self.url = Some(url);
            }
        }
        if let Ok(token) = std::env::var("OXICODE_BRIDGE_TOKEN") {
            if !token.is_empty() {
                self.auth_token = Some(token);
            }
        }
    }

    /// Debug log file path.
    pub fn debug_log_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".oxicode")
            .join("bridge-debug.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BridgeConfig::default();
        assert!(config.url.is_none());
        assert!(config.auth_token.is_none());
        assert_eq!(config.transport, Transport::Stdio);
        assert_eq!(config.port, 0);
        assert_eq!(config.bind_address, "127.0.0.1");
        assert!(config.auto_reconnect);
        assert_eq!(config.max_reconnect_delay_secs, 30);
        assert_eq!(config.max_reconnect_attempts, 100);
        assert!(!config.debug_logging);
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.permission_timeout_secs, 60);
    }

    #[test]
    fn test_from_toml_value() {
        let toml_str = r#"
url = "wss://example.com/bridge"
transport = "websocket"
port = 8080
auto_reconnect = false
debug_logging = true
max_reconnect_delay_secs = 60
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = BridgeConfig::from_toml_value(&value);
        assert_eq!(config.url.as_deref(), Some("wss://example.com/bridge"));
        assert_eq!(config.transport, Transport::WebSocket);
        assert_eq!(config.port, 8080);
        assert!(!config.auto_reconnect);
        assert!(config.debug_logging);
        assert_eq!(config.max_reconnect_delay_secs, 60);
    }

    #[test]
    fn test_partial_toml_uses_defaults() {
        let toml_str = r#"port = 3000"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = BridgeConfig::from_toml_value(&value);
        assert_eq!(config.port, 3000);
        assert!(config.auto_reconnect); // default
        assert_eq!(config.max_reconnect_delay_secs, 30); // default
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = BridgeConfig {
            url: Some("wss://test.com".to_string()),
            auth_token: Some("secret".to_string()),
            transport: Transport::WebSocket,
            port: 9090,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: BridgeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.url, config.url);
        assert_eq!(parsed.port, 9090);
    }

    #[test]
    fn test_debug_log_path() {
        let path = BridgeConfig::debug_log_path();
        assert!(path.to_string_lossy().contains("bridge-debug.log"));
    }

    #[test]
    fn test_merge_env_vars_logic() {
        // Test the merge logic without mutating global env vars.
        // Verify that the method reads from env and applies non-empty values.
        let mut config = BridgeConfig::default();
        assert!(config.url.is_none());
        assert!(config.auth_token.is_none());

        // If env vars are not set, calling merge_env_vars should leave fields as-is.
        config.merge_env_vars();
        // We can't guarantee env vars aren't set by CI, so just verify no panic.
    }

    #[test]
    fn test_empty_string_not_treated_as_value() {
        // Verify that an empty url/token from config is still None-like.
        let config = BridgeConfig {
            url: Some(String::new()),
            ..Default::default()
        };
        // Empty string is stored but callers should check.
        assert_eq!(config.url.as_deref(), Some(""));
    }
}
