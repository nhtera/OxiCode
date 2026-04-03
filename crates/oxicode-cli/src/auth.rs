//! Authentication and credential management.
//!
//! Manages API keys from env vars and credential store, plus OAuth PKCE
//! integration. Resolution priority: OAuth token > env var > credentials.toml > prompt.

use std::collections::HashMap;

pub use crate::oauth::AuthSource;

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

        // Check OAuth status.
        if crate::oauth::is_logged_in() {
            lines.push("  oauth: logged in".to_string());
        }

        lines.join("\n")
    }
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
    fn test_auth_manager_new() {
        let mgr = AuthManager::new();
        // Should not panic; may or may not have env credentials.
        let _ = mgr.status_summary();
    }

    #[test]
    fn test_auth_source_token() {
        let src = AuthSource::OAuth {
            token: "test-tok".into(),
            email: None,
        };
        assert_eq!(src.token(), Some("test-tok"));

        let src = AuthSource::ApiKey {
            key: "sk-xxx".into(),
            display: "sk-...".into(),
        };
        assert_eq!(src.token(), Some("sk-xxx"));

        let src = AuthSource::None;
        assert!(src.token().is_none());
    }
}
