//! MCP server configuration: load server definitions from config files.
//!
//! Config sources (merged in order):
//! 1. `~/.oxicode/mcp.toml` (user-level)
//! 2. `.oxicode/mcp.toml` (project-level)
//! 3. `OXICODE_MCP_SERVERS` env var (JSON)

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env_expansion;

/// Transport type for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportType {
    Stdio,
    /// SSE (deprecated in MCP spec — maps to StreamableHttp internally).
    Sse,
    /// Streamable HTTP — modern MCP transport.
    #[serde(alias = "streamable-http", alias = "streamable_http")]
    Http,
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

    /// For SSE/HTTP: the server URL.
    #[serde(default)]
    pub url: Option<String>,

    /// Extra environment variables for the subprocess.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Whether this server is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// OAuth configuration for servers requiring authentication.
    #[serde(default)]
    pub auth: Option<McpAuthConfig>,

    /// Tool allowlist — only these tools are exposed (empty = all allowed).
    #[serde(default)]
    pub allowed_tools: Vec<String>,

    /// Tool blocklist — these tools are hidden (allowlist takes precedence if both set).
    #[serde(default)]
    pub blocked_tools: Vec<String>,
}

/// OAuth/auth configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAuthConfig {
    /// OAuth authorization URL.
    pub auth_url: String,
    /// OAuth token URL.
    pub token_url: String,
    /// Client ID.
    pub client_id: String,
    /// OAuth scopes (space-separated).
    #[serde(default)]
    pub scopes: String,
}

impl McpServerConfig {
    /// Apply environment variable expansion to all string fields.
    pub fn expand_env(&mut self) {
        if let Some(ref mut cmd) = self.command {
            *cmd = env_expansion::expand_env(cmd);
        }
        self.args = self
            .args
            .iter()
            .map(|a| env_expansion::expand_env(a))
            .collect();
        if let Some(ref mut url) = self.url {
            *url = env_expansion::expand_env(url);
        }
        self.env = env_expansion::expand_env_map(&self.env);
    }

    /// Check if a tool name is permitted by this server's allow/block lists.
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        // If allowlist is non-empty, tool must be in it.
        if !self.allowed_tools.is_empty() {
            return self.allowed_tools.iter().any(|t| t == tool_name);
        }
        // Otherwise, check blocklist.
        !self.blocked_tools.iter().any(|t| t == tool_name)
    }
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
            if let Ok(env_servers) =
                serde_json::from_str::<HashMap<String, McpServerConfig>>(&env_val)
            {
                config.servers.extend(env_servers);
            } else {
                tracing::warn!("Failed to parse OXICODE_MCP_SERVERS env var");
            }
        }

        // Apply environment variable expansion to all server configs.
        for cfg in config.servers.values_mut() {
            cfg.expand_env();
        }

        config
    }

    /// Merge servers from a TOML file.
    fn merge_from_file(&mut self, path: &PathBuf) {
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
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
                let Ok(toml_str) = toml::to_string(value) else {
                    continue;
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
                auth: None,
                allowed_tools: vec![],
                blocked_tools: vec![],
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
                auth: None,
                allowed_tools: vec![],
                blocked_tools: vec![],
            },
        );
        let enabled: Vec<_> = config.enabled_servers().collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].0, "enabled");
    }

    #[test]
    fn test_channel_permissions_allowlist() {
        let cfg = McpServerConfig {
            transport: McpTransportType::Stdio,
            command: Some("echo".to_string()),
            args: vec![],
            url: None,
            env: HashMap::new(),
            enabled: true,
            auth: None,
            allowed_tools: vec!["read_file".to_string(), "write_file".to_string()],
            blocked_tools: vec![],
        };
        assert!(cfg.is_tool_allowed("read_file"));
        assert!(!cfg.is_tool_allowed("delete_file"));
    }

    #[test]
    fn test_channel_permissions_blocklist() {
        let cfg = McpServerConfig {
            transport: McpTransportType::Stdio,
            command: Some("echo".to_string()),
            args: vec![],
            url: None,
            env: HashMap::new(),
            enabled: true,
            auth: None,
            allowed_tools: vec![],
            blocked_tools: vec!["dangerous_tool".to_string()],
        };
        assert!(cfg.is_tool_allowed("read_file"));
        assert!(!cfg.is_tool_allowed("dangerous_tool"));
    }

    #[test]
    fn test_http_config_deser() {
        let toml_str = r#"
transport = "http"
url = "http://localhost:8000/mcp"
"#;
        let config: McpServerConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.transport, McpTransportType::Http));
        assert_eq!(config.url.unwrap(), "http://localhost:8000/mcp");
    }

    #[test]
    fn test_http_alias_streamable_http() {
        let toml_str = r#"
transport = "streamable-http"
url = "http://localhost:8000/mcp"
"#;
        let config: McpServerConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.transport, McpTransportType::Http));
    }

    #[test]
    fn test_http_alias_streamable_http_underscore() {
        let toml_str = r#"
transport = "streamable_http"
url = "http://localhost:8000/mcp"
"#;
        let config: McpServerConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.transport, McpTransportType::Http));
    }

    #[test]
    fn test_allowlist_takes_precedence() {
        let cfg = McpServerConfig {
            transport: McpTransportType::Stdio,
            command: Some("echo".to_string()),
            args: vec![],
            url: None,
            env: HashMap::new(),
            enabled: true,
            auth: None,
            allowed_tools: vec!["read_file".to_string()],
            blocked_tools: vec!["read_file".to_string()], // conflicting — allowlist wins
        };
        // Allowlist is non-empty, so it takes precedence.
        assert!(cfg.is_tool_allowed("read_file"));
        assert!(!cfg.is_tool_allowed("write_file"));
    }
}
