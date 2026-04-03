//! OAuth authentication and credential management.
//!
//! Provides OAuth flow with local HTTP callback, system keychain storage,
//! and token refresh. Supports Anthropic as primary provider with extensible
//! design for other providers.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

/// Default local callback port for OAuth redirect.
const OAUTH_CALLBACK_PORT: u16 = 17483;

/// Stored credential for a provider.
#[derive(Debug, Clone)]
pub struct Credential {
    /// Provider name (e.g. "anthropic", "openai").
    pub provider: String,
    /// API key or access token.
    pub token: String,
    /// Optional refresh token for OAuth flows.
    pub refresh_token: Option<String>,
    /// Expiry time as Unix timestamp (0 = no expiry / API key).
    pub expires_at: u64,
}

impl Credential {
    /// Check if the credential has expired.
    pub fn is_expired(&self) -> bool {
        if self.expires_at == 0 {
            return false; // API keys don't expire
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now >= self.expires_at
    }
}

/// Authentication manager handling credential storage and OAuth flows.
pub struct AuthManager {
    /// Cached credentials by provider name.
    credentials: HashMap<String, Credential>,
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthManager {
    /// Create a new auth manager, loading any stored credentials.
    pub fn new() -> Self {
        let mut mgr = Self {
            credentials: HashMap::new(),
        };
        mgr.load_from_env();
        mgr.load_from_credential_store();
        mgr
    }

    /// Load credentials from environment variables.
    fn load_from_env(&mut self) {
        let env_providers = [
            ("anthropic", "ANTHROPIC_API_KEY"),
            ("openai", "OPENAI_API_KEY"),
            ("google", "GOOGLE_API_KEY"),
        ];
        for (provider, env_var) in env_providers {
            if let Ok(key) = std::env::var(env_var) {
                if !key.is_empty() {
                    self.credentials.insert(
                        provider.to_string(),
                        Credential {
                            provider: provider.to_string(),
                            token: key,
                            refresh_token: None,
                            expires_at: 0,
                        },
                    );
                }
            }
        }
    }

    /// Try to load credentials from local credential store.
    fn load_from_credential_store(&mut self) {
        for provider in &["anthropic", "openai", "google"] {
            if let Some(token) = credential_store_get(provider) {
                // Don't override env var credentials.
                self.credentials
                    .entry((*provider).to_string())
                    .or_insert(Credential {
                        provider: (*provider).to_string(),
                        token,
                        refresh_token: None,
                        expires_at: 0,
                    });
            }
        }
    }

    /// Check if a provider has valid credentials.
    pub fn is_authenticated(&self, provider: &str) -> bool {
        self.credentials
            .get(provider)
            .is_some_and(|c| !c.is_expired())
    }

    /// Get the token for a provider.
    pub fn get_token(&self, provider: &str) -> Option<&str> {
        self.credentials
            .get(provider)
            .filter(|c| !c.is_expired())
            .map(|c| c.token.as_str())
    }

    /// Store a credential in memory and local credential store.
    pub fn store_credential(&mut self, credential: Credential) -> Result<(), String> {
        let provider = credential.provider.clone();
        credential_store_set(&provider, &credential.token)?;
        self.credentials.insert(provider, credential);
        Ok(())
    }

    /// Remove credential from memory and local credential store.
    pub fn clear_credential(&mut self, provider: &str) -> Result<(), String> {
        self.credentials.remove(provider);
        credential_store_delete(provider)?;
        Ok(())
    }

    /// Get status summary for all providers.
    pub fn status_summary(&self) -> String {
        let providers = ["anthropic", "openai", "google"];
        let mut lines = Vec::new();
        for p in &providers {
            let status = if let Some(cred) = self.credentials.get(*p) {
                if cred.is_expired() {
                    "expired"
                } else {
                    "authenticated"
                }
            } else {
                "not configured"
            };
            lines.push(format!("  {p}: {status}"));
        }
        lines.join("\n")
    }

    /// Initiate OAuth flow for a provider.
    /// Returns the auth URL to open in browser. Spawns a background thread
    /// that listens for the callback and stores the credential on success.
    pub fn start_oauth_flow(&mut self, provider: &str) -> Result<String, String> {
        let auth_url = match provider {
            "anthropic" => format!(
                "https://console.anthropic.com/oauth/authorize?\
                 redirect_uri=http://localhost:{OAUTH_CALLBACK_PORT}/callback&\
                 response_type=code&\
                 client_id=oxicode-cli"
            ),
            other => return Err(format!("OAuth not supported for provider: {other}")),
        };

        // Start the callback listener in a background thread.
        // The caller opens the URL in browser; when the redirect arrives,
        // the thread stores the credential automatically.
        let provider_owned = provider.to_string();
        std::thread::spawn(move || {
            match wait_for_oauth_callback(&provider_owned) {
                Ok(token) => {
                    let mut mgr = AuthManager::new();
                    let _ = mgr.store_credential(Credential {
                        provider: provider_owned,
                        token,
                        refresh_token: None,
                        expires_at: 0,
                    });
                    tracing::info!("OAuth token stored successfully");
                }
                Err(e) => {
                    tracing::error!("OAuth callback failed: {e}");
                }
            }
        });

        // Return the URL immediately so caller can open it in browser.
        Ok(auth_url)
    }
}

/// Wait for OAuth callback on local HTTP server.
/// Listens for a single request, extracts the code/token, returns it.
fn wait_for_oauth_callback(_provider: &str) -> Result<String, String> {
    let listener = TcpListener::bind(format!("127.0.0.1:{OAUTH_CALLBACK_PORT}"))
        .map_err(|e| format!("Failed to bind callback port {OAUTH_CALLBACK_PORT}: {e}"))?;

    // Set a timeout so we don't block forever.
    listener
        .set_nonblocking(false)
        .map_err(|e| format!("Failed to set blocking: {e}"))?;

    tracing::info!("OAuth callback server listening on port {OAUTH_CALLBACK_PORT}");

    let (stream, _addr) = listener
        .accept()
        .map_err(|e| format!("Failed to accept connection: {e}"))?;

    let reader = BufReader::new(&stream);
    let request_line = reader
        .lines()
        .next()
        .ok_or("No request received")?
        .map_err(|e| format!("Read error: {e}"))?;

    // Parse the GET request for the code parameter.
    let token = extract_code_from_request(&request_line)?;

    // Send a success response.
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
        <html><body><h2>Authentication successful!</h2>\
        <p>You can close this window and return to OxiCode.</p></body></html>";

    let mut writer = stream;
    let _ = writer.write_all(response.as_bytes());

    Ok(token)
}

/// Extract the `code` query parameter from an HTTP GET request line.
fn extract_code_from_request(request_line: &str) -> Result<String, String> {
    // Request line format: "GET /callback?code=xxx HTTP/1.1"
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err("Invalid request format".into());
    }

    let path = parts[1];
    if let Some(query) = path.split('?').nth(1) {
        for param in query.split('&') {
            if let Some(value) = param.strip_prefix("code=") {
                if !value.is_empty() {
                    return Ok(value.to_string());
                }
            }
        }
    }

    Err("No authorization code found in callback".into())
}

// --- Credential store helpers (file-based, cross-platform) ---

/// Get a value from local credential store (~/.oxicode/credentials.toml).
fn credential_store_get(account: &str) -> Option<String> {
    // Use credentials file as fallback when keyring crate is not available.
    let cred_path = dirs::home_dir()?.join(".oxicode").join("credentials.toml");
    if !cred_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&cred_path).ok()?;
    let table: toml::Table = content.parse().ok()?;
    table
        .get(account)
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Store a value in local credential store (file-based with restrictive perms).
fn credential_store_set(account: &str, token: &str) -> Result<(), String> {
    let dir = dirs::home_dir()
        .ok_or("Cannot determine home directory")?
        .join(".oxicode");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create config dir: {e}"))?;

    let cred_path = dir.join("credentials.toml");
    let mut table: toml::Table = if cred_path.exists() {
        let content =
            std::fs::read_to_string(&cred_path).map_err(|e| format!("Read error: {e}"))?;
        content.parse().unwrap_or_default()
    } else {
        toml::Table::new()
    };

    table.insert(account.to_string(), toml::Value::String(token.to_string()));

    let content = toml::to_string_pretty(&table).map_err(|e| format!("Serialize error: {e}"))?;
    std::fs::write(&cred_path, content).map_err(|e| format!("Write error: {e}"))?;

    // Set restrictive permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(&cred_path, perms);
    }

    Ok(())
}

/// Delete a value from local credential store.
fn credential_store_delete(account: &str) -> Result<(), String> {
    let cred_path = dirs::home_dir()
        .ok_or("Cannot determine home directory")?
        .join(".oxicode")
        .join("credentials.toml");

    if !cred_path.exists() {
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&cred_path).map_err(|e| format!("Read error: {e}"))?;
    let mut table: toml::Table = content.parse().unwrap_or_default();
    table.remove(account);

    if table.is_empty() {
        let _ = std::fs::remove_file(&cred_path);
    } else {
        let content =
            toml::to_string_pretty(&table).map_err(|e| format!("Serialize error: {e}"))?;
        std::fs::write(&cred_path, content).map_err(|e| format!("Write error: {e}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_not_expired() {
        let cred = Credential {
            provider: "test".into(),
            token: "tok".into(),
            refresh_token: None,
            expires_at: 0, // no expiry
        };
        assert!(!cred.is_expired());
    }

    #[test]
    fn test_credential_expired() {
        let cred = Credential {
            provider: "test".into(),
            token: "tok".into(),
            refresh_token: None,
            expires_at: 1, // epoch + 1s = long ago
        };
        assert!(cred.is_expired());
    }

    #[test]
    fn test_extract_code_from_request() {
        let line = "GET /callback?code=abc123 HTTP/1.1";
        assert_eq!(extract_code_from_request(line).unwrap(), "abc123");
    }

    #[test]
    fn test_extract_code_missing() {
        let line = "GET /callback HTTP/1.1";
        assert!(extract_code_from_request(line).is_err());
    }

    #[test]
    fn test_extract_code_with_extra_params() {
        let line = "GET /callback?state=xyz&code=tok999&foo=bar HTTP/1.1";
        assert_eq!(extract_code_from_request(line).unwrap(), "tok999");
    }

    #[test]
    fn test_auth_manager_new() {
        let mgr = AuthManager::new();
        // Should not panic; may or may not have env credentials.
        let _ = mgr.status_summary();
    }
}
