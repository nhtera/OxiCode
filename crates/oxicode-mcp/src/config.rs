//! MCP server configuration: load server definitions from config files.
//!
//! Config sources (merged in order):
//! 1. `~/.oxicode/mcp.toml` (user-level)
//! 2. `.oxicode/mcp.toml` (project-level)
//! 3. `OXICODE_MCP_SERVERS` env var (JSON)

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Transport type for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportType {
    Stdio,
    Sse,
    #[serde(alias = "ws", alias = "websocket")]
    WebSocket,
}

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Transport type.
    pub transport: McpTransportType,

    /// For stdio: the command to run.
    #[serde(default)]
    pub command: Option<String>,

    /// For stdio: command arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// For SSE: the server URL.
    #[serde(default)]
    pub url: Option<String>,

    /// Extra environment variables for the subprocess.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Whether this server is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// All MCP server configurations keyed by server name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

impl McpConfig {
    /// Load and merge MCP config from all sources.
    pub fn load() -> Self {
        let mut config = Self::default();

        // 1. User-level config: ~/.oxicode/mcp.toml
        if let Some(home) = dirs::home_dir() {
            let user_config = home.join(".oxicode").join("mcp.toml");
            config.merge_from_file(&user_config);
        }

        // 2. Project-level config: .oxicode/mcp.toml
        let project_config = PathBuf::from(".oxicode").join("mcp.toml");
        config.merge_from_file(&project_config);

        // 3. Environment variable: OXICODE_MCP_SERVERS (JSON)
        if let Ok(env_val) = std::env::var("OXICODE_MCP_SERVERS") {
            if let Ok(env_servers) = serde_json::from_str::<HashMap<String, McpServerConfig>>(&env_val) {
                config.servers.extend(env_servers);
            } else {
                tracing::warn!("Failed to parse OXICODE_MCP_SERVERS env var");
            }
        }

        config
    }

    /// Merge servers from a TOML file.
    fn merge_from_file(&mut self, path: &PathBuf) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return,
        };

        let parsed: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to parse MCP config at {}: {e}", path.display());
                return;
            }
        };

        // Expect a [servers] table or top-level server entries.
        let servers_table = parsed
            .get("servers")
            .or(Some(&parsed))
            .and_then(|v| v.as_table());

        if let Some(table) = servers_table {
            for (name, value) in table {
                if name == "servers" {
                    continue;
                }
                let toml_str = match toml::to_string(value) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                match toml::from_str::<McpServerConfig>(&toml_str) {
                    Ok(server_config) => {
                        self.servers.insert(name.clone(), server_config);
                    }
                    Err(e) => {
                        tracing::warn!("Invalid MCP server config '{name}': {e}");
                    }
                }
            }
        }
    }

    /// Get enabled servers only.
    pub fn enabled_servers(&self) -> impl Iterator<Item = (&str, &McpServerConfig)> {
        self.servers
            .iter()
            .filter(|(_, cfg)| cfg.enabled)
            .map(|(name, cfg)| (name.as_str(), cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdio_config_deser() {
        let toml_str = r#"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
"#;
        let config: McpServerConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.transport, McpTransportType::Stdio));
        assert_eq!(config.command.unwrap(), "npx");
        assert_eq!(config.args.len(), 2);
    }

    #[test]
    fn test_sse_config_deser() {
        let toml_str = r#"
transport = "sse"
url = "http://localhost:3000/mcp"
"#;
        let config: McpServerConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.transport, McpTransportType::Sse));
        assert_eq!(config.url.unwrap(), "http://localhost:3000/mcp");
    }

    #[test]
    fn test_disabled_server_filtered() {
        let mut config = McpConfig::default();
        config.servers.insert(
            "enabled".to_string(),
            McpServerConfig {
                transport: McpTransportType::Stdio,
                command: Some("echo".to_string()),
                args: vec![],
                url: None,
                env: HashMap::new(),
                enabled: true,
            },
        );
        config.servers.insert(
            "disabled".to_string(),
            McpServerConfig {
                transport: McpTransportType::Stdio,
                command: Some("echo".to_string()),
                args: vec![],
                url: None,
                env: HashMap::new(),
                enabled: false,
            },
        );
        let enabled: Vec<_> = config.enabled_servers().collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].0, "enabled");
    }
}
