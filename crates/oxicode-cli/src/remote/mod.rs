//! Remote bridge module — headless multi-session server via WebSocket.
//!
//! Feature-gated behind `--features bridge`. Provides:
//! - WebSocket server for remote clients (IDE extensions, cloud)
//! - Multi-session pool with lifecycle management
//! - JWT authentication for remote access
//! - Permission bridge (forward approvals to remote client)
//! - SDK message adapter (wire format <-> internal types)

#[cfg(feature = "bridge")]
pub mod permission_bridge;
#[cfg(feature = "bridge")]
pub mod sdk_adapter;
pub mod session_pool;

/// Default bridge port.
pub const DEFAULT_BRIDGE_PORT: u16 = 8080;

/// Maximum concurrent sessions per bridge instance.
pub const DEFAULT_MAX_SESSIONS: usize = 10;

/// Session idle timeout in seconds (30 minutes).
pub const SESSION_IDLE_TIMEOUT_SECS: u64 = 1800;

/// Bridge server configuration.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Port to bind on.
    pub port: u16,
    /// Maximum concurrent sessions.
    pub max_sessions: usize,
    /// JWT secret for authentication (256-bit minimum recommended).
    pub jwt_secret: Option<String>,
    /// Bind address (default: 127.0.0.1 for security).
    pub bind_address: String,
    /// Session idle timeout in seconds.
    pub idle_timeout_secs: u64,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_BRIDGE_PORT,
            max_sessions: DEFAULT_MAX_SESSIONS,
            jwt_secret: None,
            bind_address: "127.0.0.1".to_string(),
            idle_timeout_secs: SESSION_IDLE_TIMEOUT_SECS,
        }
    }
}

impl BridgeConfig {
    /// Create config with specific port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set JWT secret for authentication.
    pub fn with_jwt_secret(mut self, secret: String) -> Self {
        self.jwt_secret = Some(secret);
        self
    }

    /// Set bind address (e.g. "0.0.0.0" for external access).
    pub fn with_bind_address(mut self, addr: String) -> Self {
        self.bind_address = addr;
        self
    }

    /// Full socket address string (e.g. "127.0.0.1:8080").
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind_address, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BridgeConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.max_sessions, 10);
        assert!(config.jwt_secret.is_none());
        assert_eq!(config.bind_address, "127.0.0.1");
    }

    #[test]
    fn test_config_builder() {
        let config = BridgeConfig::default()
            .with_port(3000)
            .with_jwt_secret("my-secret".to_string())
            .with_bind_address("0.0.0.0".to_string());
        assert_eq!(config.port, 3000);
        assert_eq!(config.jwt_secret.as_deref(), Some("my-secret"));
        assert_eq!(config.bind_address, "0.0.0.0");
    }

    #[test]
    fn test_socket_addr() {
        let config = BridgeConfig::default();
        assert_eq!(config.socket_addr(), "127.0.0.1:8080");

        let config = BridgeConfig::default().with_port(3000);
        assert_eq!(config.socket_addr(), "127.0.0.1:3000");
    }

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_BRIDGE_PORT, 8080);
        assert_eq!(DEFAULT_MAX_SESSIONS, 10);
        assert_eq!(SESSION_IDLE_TIMEOUT_SECS, 1800);
    }
}
