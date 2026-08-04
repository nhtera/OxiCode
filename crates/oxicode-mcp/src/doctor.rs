//! MCP server diagnostics: test connectivity, report capabilities and latency.
//!
//! Used by `/mcp doctor` command to verify server health.
//! Uses rmcp transports for connectivity testing.

use std::fmt;
use std::time::{Duration, Instant};

use rmcp::transport::TokioChildProcess;
use rmcp::ServiceExt;

use crate::config::{McpConfig, McpServerConfig, McpTransportType};

/// Timeout for a single server diagnostic (5 seconds).
const DIAG_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of diagnosing a single MCP server.
#[derive(Debug, Clone)]
pub struct DiagResult {
    pub server_name: String,
    pub status: DiagStatus,
    pub latency: Option<Duration>,
    /// Whether the server advertises tools capability.
    pub has_tools: bool,
    pub error_detail: Option<String>,
}

/// Diagnostic status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagStatus {
    Ok,
    Error,
    Timeout,
}

impl fmt::Display for DiagStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Error => write!(f, "error"),
            Self::Timeout => write!(f, "timeout"),
        }
    }
}

impl fmt::Display for DiagResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "  {:<20} [{}]", self.server_name, self.status)?;
        if let Some(lat) = self.latency {
            write!(f, " {lat:?}")?;
        }
        if self.has_tools {
            write!(f, " (tools: yes)")?;
        }
        if let Some(ref err) = self.error_detail {
            write!(f, "\n    Error: {err}")?;
        }
        Ok(())
    }
}

/// Diagnose a single MCP server: attempt connection and initialize via rmcp.
async fn diagnose_server_inner(name: &str, config: &McpServerConfig) -> DiagResult {
    let start = Instant::now();

    let result = match &config.transport {
        McpTransportType::Stdio => {
            let Some(ref command) = config.command else {
                return DiagResult {
                    server_name: name.to_string(),
                    status: DiagStatus::Error,
                    latency: None,
                    has_tools: false,
                    error_detail: Some("No command configured".to_string()),
                };
            };

            let mut cmd = tokio::process::Command::new(command);
            cmd.args(&config.args);
            for (k, v) in &config.env {
                cmd.env(k, v);
            }

            let transport = match TokioChildProcess::new(cmd) {
                Ok(t) => t,
                Err(e) => {
                    return DiagResult {
                        server_name: name.to_string(),
                        status: DiagStatus::Error,
                        latency: Some(start.elapsed()),
                        has_tools: false,
                        error_detail: Some(format!("Spawn failed: {e}")),
                    };
                }
            };

            match ().serve(transport).await {
                Ok(client) => {
                    let has_tools = !client.list_all_tools().await.map_or(true, |t| t.is_empty());
                    let _ = client.cancel().await;
                    Ok(has_tools)
                }
                Err(e) => Err(e.to_string()),
            }
        }
        McpTransportType::Sse | McpTransportType::Http => {
            let Some(ref url) = config.url else {
                return DiagResult {
                    server_name: name.to_string(),
                    status: DiagStatus::Error,
                    latency: None,
                    has_tools: false,
                    error_detail: Some("No URL configured".to_string()),
                };
            };

            let transport = rmcp::transport::StreamableHttpClientTransport::from_uri(url.as_str());

            match ().serve(transport).await {
                Ok(client) => {
                    let has_tools = !client.list_all_tools().await.map_or(true, |t| t.is_empty());
                    let _ = client.cancel().await;
                    Ok(has_tools)
                }
                Err(e) => Err(e.to_string()),
            }
        }
    };

    let latency = start.elapsed();

    match result {
        Ok(has_tools) => DiagResult {
            server_name: name.to_string(),
            status: DiagStatus::Ok,
            latency: Some(latency),
            has_tools,
            error_detail: None,
        },
        Err(e) => DiagResult {
            server_name: name.to_string(),
            status: DiagStatus::Error,
            latency: Some(latency),
            has_tools: false,
            error_detail: Some(e),
        },
    }
}

/// Diagnose a single server with a timeout wrapper.
pub async fn diagnose_server(name: &str, config: &McpServerConfig) -> DiagResult {
    match tokio::time::timeout(DIAG_TIMEOUT, diagnose_server_inner(name, config)).await {
        Ok(result) => result,
        Err(_) => DiagResult {
            server_name: name.to_string(),
            status: DiagStatus::Timeout,
            latency: Some(DIAG_TIMEOUT),
            has_tools: false,
            error_detail: Some(format!("Timed out after {DIAG_TIMEOUT:?}")),
        },
    }
}

/// Diagnose all enabled servers from config.
pub async fn diagnose_all(config: &McpConfig) -> Vec<DiagResult> {
    let mut results = Vec::new();
    for (name, server_config) in config.enabled_servers() {
        results.push(diagnose_server(name, server_config).await);
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diag_status_display() {
        assert_eq!(format!("{}", DiagStatus::Ok), "ok");
        assert_eq!(format!("{}", DiagStatus::Error), "error");
        assert_eq!(format!("{}", DiagStatus::Timeout), "timeout");
    }

    #[test]
    fn test_diag_result_display_ok() {
        let result = DiagResult {
            server_name: "test-server".to_string(),
            status: DiagStatus::Ok,
            latency: Some(Duration::from_millis(42)),
            has_tools: true,
            error_detail: None,
        };
        let output = format!("{result}");
        assert!(output.contains("test-server"));
        assert!(output.contains("ok"));
        assert!(output.contains("tools: yes"));
    }

    #[test]
    fn test_diag_result_display_error() {
        let result = DiagResult {
            server_name: "broken".to_string(),
            status: DiagStatus::Error,
            latency: None,
            has_tools: false,
            error_detail: Some("Connection refused".to_string()),
        };
        let output = format!("{result}");
        assert!(output.contains("error"));
        assert!(output.contains("Connection refused"));
    }

    #[tokio::test]
    async fn test_diagnose_server_no_command() {
        use std::collections::HashMap;
        let cfg = McpServerConfig {
            transport: McpTransportType::Stdio,
            command: None,
            args: vec![],
            url: None,
            env: HashMap::new(),
            enabled: true,
            auth: None,
            allowed_tools: vec![],
            blocked_tools: vec![],
        };
        let result = diagnose_server("no-cmd", &cfg).await;
        assert_eq!(result.status, DiagStatus::Error);
        assert!(result.error_detail.unwrap().contains("No command"));
    }

    #[tokio::test]
    async fn test_diagnose_server_no_url() {
        use std::collections::HashMap;
        let cfg = McpServerConfig {
            transport: McpTransportType::Sse,
            command: None,
            args: vec![],
            url: None,
            env: HashMap::new(),
            enabled: true,
            auth: None,
            allowed_tools: vec![],
            blocked_tools: vec![],
        };
        let result = diagnose_server("no-url", &cfg).await;
        assert_eq!(result.status, DiagStatus::Error);
        assert!(result.error_detail.unwrap().contains("No URL"));
    }
}
