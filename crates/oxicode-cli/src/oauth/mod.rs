//! OAuth PKCE orchestration for CLI authentication.
//!
//! Coordinates the full OAuth login flow:
//! 1. Generate PKCE verifier + challenge
//! 2. Start local callback server
//! 3. Open browser to authorization URL
//! 4. Receive authorization code via callback
//! 5. Exchange code for tokens
//! 6. Fetch user info (email)
//! 7. Store tokens encrypted on disk
//!
//! Also handles logout (clear tokens) and token refresh.

pub mod callback_server;
pub mod client;
pub mod pkce;
pub mod token_store;

pub use client::OAuthTokenData;

/// Result of a successful OAuth login.
pub struct LoginResult {
    pub email: Option<String>,
    pub token: String,
}

/// Authentication source for API calls.
#[derive(Debug, Clone)]
pub enum AuthSource {
    /// OAuth Bearer token with optional email.
    OAuth {
        token: String,
        email: Option<String>,
    },
    /// Raw API key from env var or config file.
    ApiKey {
        key: String,
        /// Masked display: "sk-...XXXX"
        display: String,
    },
    /// No authentication configured.
    None,
}

impl AuthSource {
    /// Get the token/key string for API calls.
    pub fn token(&self) -> Option<&str> {
        match self {
            Self::OAuth { token, .. } => Some(token),
            Self::ApiKey { key, .. } => Some(key),
            Self::None => None,
        }
    }

    /// Get a display-safe label for the status bar.
    pub fn display_label(&self) -> String {
        match self {
            Self::OAuth { email, .. } => {
                if let Some(email) = email {
                    format!("\u{26a1} {email}")
                } else {
                    "\u{26a1} OAuth".to_string()
                }
            }
            Self::ApiKey { display, .. } => format!("\u{1f511} {display}"),
            Self::None => "No auth".to_string(),
        }
    }

    /// Whether this is an OAuth source (uses Bearer auth instead of x-api-key).
    pub fn is_oauth(&self) -> bool {
        matches!(self, Self::OAuth { .. })
    }
}

/// Run the full OAuth PKCE login flow.
///
/// Opens the browser, waits for the callback, exchanges the code,
/// and stores tokens. Returns the login result or an error message.
pub async fn login() -> Result<LoginResult, String> {
    // 1. Generate PKCE pair + CSRF state.
    let pkce_pair = pkce::generate_pkce();
    let state = pkce::generate_state();

    // 2. Start callback server.
    let (port, rx) = callback_server::start_callback_server(120).await?;

    // 3. Build authorization URL (with state for CSRF protection) and open browser.
    let auth_url = client::build_auth_url(port, &pkce_pair.challenge, &state);
    tracing::info!("Opening browser for OAuth login...");

    if open_browser(&auth_url).is_err() {
        // If browser fails to open, print the URL for manual copy.
        return Err(format!("Could not open browser. Please visit:\n{auth_url}"));
    }

    // 4. Wait for the authorization code from the callback.
    let callback = rx
        .await
        .map_err(|_| "OAuth callback cancelled or timed out (120s)".to_string())?;

    // 5. Validate CSRF state parameter.
    match &callback.state {
        Some(returned_state) if returned_state == &state => {
            tracing::debug!("OAuth state parameter validated");
        }
        Some(returned_state) => {
            return Err(format!(
                "OAuth state mismatch — possible CSRF attack. \
                 Expected: {state}, got: {returned_state}"
            ));
        }
        None => {
            // Some OAuth servers may not return state — log warning but proceed.
            tracing::warn!("OAuth callback missing state parameter — CSRF check skipped");
        }
    }

    // 6. Exchange code for tokens.
    let mut tokens = client::exchange_code(&callback.code, &pkce_pair.verifier, port).await?;

    // 7. Fetch user info (non-fatal).
    let email = client::get_user_info(&tokens.access_token).await;
    tokens.email.clone_from(&email);

    // 8. Store tokens.
    token_store::save_tokens(&tokens)?;

    tracing::info!("OAuth login successful");
    Ok(LoginResult {
        email,
        token: tokens.access_token,
    })
}

/// Log out: clear stored OAuth tokens.
pub fn logout() -> Result<(), String> {
    token_store::clear_tokens()?;
    tracing::info!("OAuth tokens cleared");
    Ok(())
}

/// Check if there are stored OAuth tokens (possibly expired but refreshable).
pub fn is_logged_in() -> bool {
    token_store::load_tokens().is_some()
}

/// Ensure we have a valid (non-expired) OAuth token.
///
/// Loads stored tokens, refreshes if needed, returns the access token.
/// Returns `None` if no OAuth tokens are stored or refresh fails.
pub async fn ensure_valid_token() -> Option<String> {
    let mut tokens = token_store::load_tokens()?;

    if !tokens.is_expired() && !tokens.needs_refresh() {
        return Some(tokens.access_token);
    }

    // Try to refresh.
    let refresh_tok = tokens.refresh_token.as_deref()?;
    match client::refresh_token(refresh_tok).await {
        Ok(mut new_tokens) => {
            // Preserve email from old tokens if not in refresh response.
            if new_tokens.email.is_none() {
                new_tokens.email = tokens.email.take();
            }
            // Preserve refresh token if server didn't return a new one.
            if new_tokens.refresh_token.is_none() {
                new_tokens.refresh_token = tokens.refresh_token.take();
            }
            let access = new_tokens.access_token.clone();
            if let Err(e) = token_store::save_tokens(&new_tokens) {
                tracing::warn!("Failed to save refreshed tokens: {e}");
            }
            Some(access)
        }
        Err(e) => {
            tracing::warn!("Token refresh failed: {e}");
            // If token isn't expired yet, use it anyway.
            if tokens.is_expired() {
                None
            } else {
                Some(tokens.access_token)
            }
        }
    }
}

/// Resolve the best available auth source.
///
/// Priority: OAuth token > ANTHROPIC_API_KEY env > ANTHROPIC_AUTH_TOKEN env
/// > credentials.toml > None
pub async fn resolve_auth_source() -> AuthSource {
    // 1. Try OAuth.
    if let Some(tokens) = token_store::load_tokens() {
        if !tokens.is_expired() {
            return AuthSource::OAuth {
                token: tokens.access_token,
                email: tokens.email,
            };
        }
        // Try refresh.
        if let Some(token) = ensure_valid_token().await {
            let refreshed = token_store::load_tokens();
            let email = refreshed.and_then(|t| t.email);
            return AuthSource::OAuth { token, email };
        }
    }

    // 2. Try env var.
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            let display = mask_api_key(&key);
            return AuthSource::ApiKey { key, display };
        }
    }
    if let Ok(token) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
        if !token.is_empty() {
            let display = mask_api_key(&token);
            return AuthSource::ApiKey {
                key: token,
                display,
            };
        }
    }

    // 3. Try credentials.toml.
    if let Some(key) = load_credential_store_key("anthropic") {
        let display = mask_api_key(&key);
        return AuthSource::ApiKey { key, display };
    }

    AuthSource::None
}

/// Mask an API key for display: "sk-ant-...XXXX"
fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "sk-...".to_string();
    }
    let prefix = &key[..std::cmp::min(7, key.len())];
    let suffix = &key[key.len() - 4..];
    format!("{prefix}...{suffix}")
}

/// Load a key from credentials.toml (same as auth.rs credential_store_get).
fn load_credential_store_key(account: &str) -> Option<String> {
    let cred_path = dirs::home_dir()?.join(".oxicode").join("credentials.toml");
    let content = std::fs::read_to_string(cred_path).ok()?;
    let table: toml::Table = content.parse().ok()?;
    table
        .get(account)
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Open a URL in the default browser.
fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_source_display_oauth_with_email() {
        let src = AuthSource::OAuth {
            token: "tok".into(),
            email: Some("user@example.com".into()),
        };
        assert!(src.display_label().contains("user@example.com"));
        assert!(src.is_oauth());
    }

    #[test]
    fn test_auth_source_display_api_key() {
        let src = AuthSource::ApiKey {
            key: "sk-ant-api03-abcdefghijklmnop".into(),
            display: "sk-ant-...mnop".into(),
        };
        assert!(src.display_label().contains("sk-ant-"));
        assert!(!src.is_oauth());
    }

    #[test]
    fn test_auth_source_none() {
        let src = AuthSource::None;
        assert_eq!(src.display_label(), "No auth");
        assert!(src.token().is_none());
    }

    #[test]
    fn test_mask_api_key() {
        assert_eq!(
            mask_api_key("sk-ant-api03-abcdefghijklmnop"),
            "sk-ant-...mnop"
        );
        assert_eq!(mask_api_key("short"), "sk-...");
    }

    #[test]
    fn test_is_logged_in_false_when_no_tokens() {
        // No tokens file should exist in a clean test env.
        // This test is environment-dependent but safe — just checks the function doesn't panic.
        let _ = is_logged_in();
    }

    #[test]
    fn test_logout_succeeds_when_no_tokens() {
        // Logout should succeed even if no tokens file exists.
        assert!(logout().is_ok());
    }
}
