//! OAuth HTTP client for token exchange, refresh, and user info retrieval.
//!
//! Handles communication with Anthropic's OAuth token endpoint.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Anthropic OAuth endpoints.
const ANTHROPIC_AUTH_URL: &str = "https://console.anthropic.com/oauth/authorize";
const ANTHROPIC_TOKEN_URL: &str = "https://console.anthropic.com/oauth/token";
const ANTHROPIC_USERINFO_URL: &str = "https://api.anthropic.com/v1/me";
const ANTHROPIC_CLIENT_ID: &str = "oxicode-cli";

/// Stored OAuth token data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_at: Option<u64>,
    pub email: Option<String>,
}

impl OAuthTokenData {
    /// Check if token has expired (with 60s buffer for refresh).
    pub fn is_expired(&self) -> bool {
        let Some(expires_at) = self.expires_at else {
            return false;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now + 60 >= expires_at
    }

    /// Check if the token is within the refresh window (expires in < 5 min).
    pub fn needs_refresh(&self) -> bool {
        let Some(expires_at) = self.expires_at else {
            return false;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now + 300 >= expires_at
    }
}

/// OAuth token endpoint response from the authorization server.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<u64>,
}

/// User info response from the API.
#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// Build the Anthropic OAuth authorization URL with PKCE challenge and state.
pub fn build_auth_url(redirect_port: u16, challenge: &str, state: &str) -> String {
    use super::pkce::urlencoding;

    let redirect_uri = format!("http://localhost:{redirect_port}/callback");
    format!(
        "{ANTHROPIC_AUTH_URL}?\
         response_type=code&\
         client_id={}&\
         redirect_uri={}&\
         code_challenge={}&\
         code_challenge_method=S256&\
         state={}",
        urlencoding(ANTHROPIC_CLIENT_ID),
        urlencoding(&redirect_uri),
        urlencoding(challenge),
        urlencoding(state),
    )
}

/// Exchange an authorization code + PKCE verifier for tokens.
pub async fn exchange_code(
    code: &str,
    verifier: &str,
    redirect_port: u16,
) -> Result<OAuthTokenData, String> {
    let redirect_uri = format!("http://localhost:{redirect_port}/callback");
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", code);
    params.insert("redirect_uri", &redirect_uri);
    params.insert("client_id", ANTHROPIC_CLIENT_ID);
    params.insert("code_verifier", verifier);

    let client = reqwest::Client::new();
    let resp = client
        .post(ANTHROPIC_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token exchange request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange HTTP {status}: {body}"));
    }

    let token_resp: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse token response: {e}"))?;

    let expires_at = token_resp.expires_in.map(|secs| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + secs
    });

    Ok(OAuthTokenData {
        access_token: token_resp.access_token,
        refresh_token: token_resp.refresh_token,
        token_type: token_resp
            .token_type
            .unwrap_or_else(|| "Bearer".to_string()),
        expires_at,
        email: None, // Populated later via get_user_info.
    })
}

/// Refresh an expired token using the refresh token.
pub async fn refresh_token(current_refresh_token: &str) -> Result<OAuthTokenData, String> {
    let mut params = HashMap::new();
    params.insert("grant_type", "refresh_token");
    params.insert("refresh_token", current_refresh_token);
    params.insert("client_id", ANTHROPIC_CLIENT_ID);

    let client = reqwest::Client::new();
    let resp = client
        .post(ANTHROPIC_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token refresh request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token refresh HTTP {status}: {body}"));
    }

    let token_resp: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse refresh response: {e}"))?;

    let expires_at = token_resp.expires_in.map(|secs| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + secs
    });

    Ok(OAuthTokenData {
        access_token: token_resp.access_token,
        refresh_token: token_resp.refresh_token,
        token_type: token_resp
            .token_type
            .unwrap_or_else(|| "Bearer".to_string()),
        expires_at,
        email: None,
    })
}

/// Fetch user info (email) using an access token. Non-fatal on failure.
pub async fn get_user_info(access_token: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(ANTHROPIC_USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let info: UserInfoResponse = resp.json().await.ok()?;
    info.email.or(info.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_not_expired() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = OAuthTokenData {
            access_token: "test".into(),
            refresh_token: None,
            token_type: "Bearer".into(),
            expires_at: Some(now + 3600),
            email: None,
        };
        assert!(!token.is_expired());
    }

    #[test]
    fn test_token_expired() {
        let token = OAuthTokenData {
            access_token: "test".into(),
            refresh_token: None,
            token_type: "Bearer".into(),
            expires_at: Some(1000),
            email: None,
        };
        assert!(token.is_expired());
    }

    #[test]
    fn test_token_no_expiry() {
        let token = OAuthTokenData {
            access_token: "test".into(),
            refresh_token: None,
            token_type: "Bearer".into(),
            expires_at: None,
            email: None,
        };
        assert!(!token.is_expired());
        assert!(!token.needs_refresh());
    }

    #[test]
    fn test_needs_refresh_within_window() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = OAuthTokenData {
            access_token: "test".into(),
            refresh_token: Some("refresh".into()),
            token_type: "Bearer".into(),
            expires_at: Some(now + 120), // Expires in 2 min — within 5 min window.
            email: None,
        };
        assert!(token.needs_refresh());
        assert!(!token.is_expired());
    }

    #[test]
    fn test_build_auth_url_contains_required_params() {
        let url = build_auth_url(8880, "test-challenge", "test-state");
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id="));
        assert!(url.contains("code_challenge=test-challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("state=test-state"));
    }
}
