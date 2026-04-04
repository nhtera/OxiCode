//! IDE bridge module — protocol + transport for VS Code / JetBrains integration.
//!
//! Phase A (MVP): IDE spawns `oxicode --server` and communicates via JSON-RPC
//! over stdin/stdout. The bridge layer adds IDE-specific methods on top of the
//! existing server protocol.
//!
//! Phase B (Daemon): TCP/WebSocket listener so IDE connects to a running daemon.
//! Gated behind `bridge` feature flag.

pub mod bridge_config;
pub mod bridge_debug;
pub mod bridge_status;
pub mod daemon_listener;
pub mod messages;
pub mod permission_bridge;
pub mod reconnection;
pub mod session_bridge;
pub mod session_ingress;

use serde::{Deserialize, Serialize};

/// Bridge protocol version.
pub const PROTOCOL_VERSION: &str = "1.0";

/// Transport mode for the bridge server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Transport {
    /// JSON-RPC over stdin/stdout (default, Phase A).
    #[default]
    Stdio,
    /// TCP socket (Phase B, requires `bridge` feature).
    Tcp,
    /// WebSocket upgrade over TCP (Phase B, requires `bridge` feature).
    WebSocket,
}


impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio => write!(f, "stdio"),
            Self::Tcp => write!(f, "tcp"),
            Self::WebSocket => write!(f, "websocket"),
        }
    }
}

/// Capabilities that the bridge server advertises.
pub fn server_capabilities() -> Vec<String> {
    vec![
        "streaming".to_string(),
        "permissions".to_string(),
        "tool_execution".to_string(),
        "session_management".to_string(),
        "model_switching".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_default_is_stdio() {
        assert_eq!(Transport::default(), Transport::Stdio);
    }

    #[test]
    fn transport_display() {
        assert_eq!(Transport::Stdio.to_string(), "stdio");
        assert_eq!(Transport::Tcp.to_string(), "tcp");
        assert_eq!(Transport::WebSocket.to_string(), "websocket");
    }

    #[test]
    fn transport_serde_roundtrip() {
        let json = serde_json::to_string(&Transport::WebSocket).unwrap();
        assert_eq!(json, "\"websocket\"");
        let parsed: Transport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Transport::WebSocket);
    }

    #[test]
    fn server_capabilities_not_empty() {
        let caps = server_capabilities();
        assert!(!caps.is_empty());
        assert!(caps.contains(&"streaming".to_string()));
        assert!(caps.contains(&"permissions".to_string()));
    }

    #[test]
    fn protocol_version_is_set() {
        assert_eq!(PROTOCOL_VERSION, "1.0");
    }
}
