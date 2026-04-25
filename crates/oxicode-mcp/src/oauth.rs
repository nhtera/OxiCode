//! OAuth PKCE flow for MCP servers requiring authentication.
//!
//! Implements RFC 7636 (PKCE) for secure authorization code flow.
//! Tokens stored in `~/.oxicode/mcp-tokens/{server-name}.json` with 0600 permissions.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::McpAuthConfig;

/// Stored OAuth token for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
}

impl OAuthToken {
    /// Check if the token has expired (with 60s buffer).
    pub fn is_expired(&self) -> bool {
        let Some(expires_at) = self.expires_at else {
            return false; // No expiry info, assume valid.
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now + 60 >= expires_at
    }
}

/// PKCE challenge pair.
struct PkceChallenge {
    verifier: String,
    challenge: String,
}

/// Generate a PKCE verifier and challenge (S256 method).
fn generate_pkce() -> PkceChallenge {
    use std::io::Read;

    // Generate 32 random bytes for verifier.
    let mut random_bytes = [0u8; 32];
    // Use /dev/urandom on Unix, fallback to timestamp-based for portability.
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut random_bytes);
    } else {
        // Fallback: use system time nanos as seed (not cryptographically strong, but functional).
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for (i, byte) in random_bytes.iter_mut().enumerate() {
            *byte = ((nanos >> (i % 16)) & 0xFF) as u8 ^ (i as u8);
        }
    }

    let verifier = base64_url_encode(&random_bytes);

    // S256: challenge = BASE64URL(SHA256(verifier))
    // Simple SHA-256 using a basic implementation to avoid adding a dependency.
    let challenge = base64_url_encode(&simple_sha256(verifier.as_bytes()));

    PkceChallenge {
        verifier,
        challenge,
    }
}

/// Base64 URL-safe encoding without padding.
fn base64_url_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity((data.len() * 4).div_ceil(3));
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0u32, |&b| u32::from(b));
        let b2 = chunk.get(2).map_or(0u32, |&b| u32::from(b));
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        }
    }
    result
}

/// Minimal SHA-256 (pure Rust, no deps). Used only for PKCE challenge derivation.
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
// SHA-256 implementation inherently uses single-char variables (a..h) per spec
fn simple_sha256(data: &[u8]) -> [u8; 32] {
    // SHA-256 constants.
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];

    // Padding.
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process 512-bit blocks.
    for block in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        result[i * 4..i * 4 + 4].copy_from_slice(&val.to_be_bytes());
    }
    result
}

/// MCP OAuth manager — handles PKCE flow, token storage, and refresh.
pub struct McpOAuth;

impl McpOAuth {
    /// Token storage directory.
    fn token_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".oxicode").join("mcp-tokens"))
    }

    /// Token file path for a server.
    fn token_path(server_name: &str) -> Option<PathBuf> {
        Self::token_dir().map(|d| d.join(format!("{server_name}.json")))
    }

    /// Load stored token for a server.
    pub fn load_token(server_name: &str) -> Option<OAuthToken> {
        let path = Self::token_path(server_name)?;
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Store token for a server with restricted permissions.
    pub fn store_token(server_name: &str, token: &OAuthToken) -> Result<(), String> {
        let dir = Self::token_dir().ok_or("Cannot determine home directory")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create token dir: {e}"))?;

        let path = dir.join(format!("{server_name}.json"));
        let json = serde_json::to_string_pretty(token)
            .map_err(|e| format!("Failed to serialize token: {e}"))?;

        std::fs::write(&path, &json).map_err(|e| format!("Failed to write token: {e}"))?;

        // Set restricted file permissions (Unix only).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&path, perms);
        }

        Ok(())
    }

    /// Build the authorization URL for a PKCE flow.
    ///
    /// Returns `(auth_url, pkce_verifier)` — the verifier must be stored
    /// until the callback is received.
    pub fn build_auth_url(auth_config: &McpAuthConfig, redirect_port: u16) -> (String, String) {
        let pkce = generate_pkce();
        let redirect_uri = format!("http://localhost:{redirect_port}/callback");

        let url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&scope={}",
            auth_config.auth_url,
            urlencoding(&auth_config.client_id),
            urlencoding(&redirect_uri),
            urlencoding(&pkce.challenge),
            urlencoding(&auth_config.scopes),
        );

        (url, pkce.verifier)
    }

    /// Exchange an authorization code for tokens.
    pub async fn exchange_code(
        auth_config: &McpAuthConfig,
        code: &str,
        verifier: &str,
        redirect_port: u16,
    ) -> Result<OAuthToken, String> {
        let redirect_uri = format!("http://localhost:{redirect_port}/callback");
        let mut params = HashMap::new();
        params.insert("grant_type", "authorization_code");
        params.insert("code", code);
        params.insert("redirect_uri", &redirect_uri);
        params.insert("client_id", &auth_config.client_id);
        params.insert("code_verifier", verifier);

        let client = reqwest::Client::new();
        let resp = client
            .post(&auth_config.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Token exchange failed: {e}"))?;

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

        Ok(OAuthToken {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            token_type: token_resp
                .token_type
                .unwrap_or_else(|| "Bearer".to_string()),
            expires_at,
            scope: token_resp.scope,
        })
    }

    /// Refresh an expired token.
    pub async fn refresh_token(
        auth_config: &McpAuthConfig,
        refresh_token: &str,
    ) -> Result<OAuthToken, String> {
        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token");
        params.insert("refresh_token", refresh_token);
        params.insert("client_id", &auth_config.client_id);

        let client = reqwest::Client::new();
        let resp = client
            .post(&auth_config.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Token refresh failed: {e}"))?;

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

        Ok(OAuthToken {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            token_type: token_resp
                .token_type
                .unwrap_or_else(|| "Bearer".to_string()),
            expires_at,
            scope: token_resp.scope,
        })
    }

    /// Find an available port for the OAuth callback listener.
    pub fn find_callback_port() -> Result<u16, String> {
        for port in 8880..8900 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return Ok(port);
            }
        }
        Err("No available port in range 8880-8899 for OAuth callback".to_string())
    }
}

/// OAuth token endpoint response.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
}

/// Simple percent-encoding for URL query parameters.
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push('%');
                result.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
                result.push(char::from(b"0123456789ABCDEF"[(byte & 0x0F) as usize]));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let pkce = generate_pkce();
        assert!(!pkce.verifier.is_empty());
        assert!(!pkce.challenge.is_empty());
        // Verifier and challenge should be different.
        assert_ne!(pkce.verifier, pkce.challenge);
    }

    #[test]
    fn test_base64_url_encode() {
        let data = b"hello";
        let encoded = base64_url_encode(data);
        // Should not contain +, /, or =.
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn test_sha256_empty() {
        use std::fmt::Write;
        let hash = simple_sha256(b"");
        // SHA-256 of empty string is well-known.
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let hex: String = hash.iter().fold(String::new(), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        });
        assert_eq!(hex, expected);
    }

    #[test]
    fn test_sha256_hello() {
        use std::fmt::Write;
        let hash = simple_sha256(b"hello");
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let hex: String = hash.iter().fold(String::new(), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        });
        assert_eq!(hex, expected);
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
        assert_eq!(urlencoding("a=b&c=d"), "a%3Db%26c%3Dd");
        assert_eq!(urlencoding("safe-string_v2.0"), "safe-string_v2.0");
    }

    #[test]
    fn test_oauth_token_not_expired() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = OAuthToken {
            access_token: "test".to_string(),
            refresh_token: None,
            token_type: "Bearer".to_string(),
            expires_at: Some(now + 3600),
            scope: None,
        };
        assert!(!token.is_expired());
    }

    #[test]
    fn test_oauth_token_expired() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            refresh_token: None,
            token_type: "Bearer".to_string(),
            expires_at: Some(1000), // Long in the past.
            scope: None,
        };
        assert!(token.is_expired());
    }

    #[test]
    fn test_oauth_token_no_expiry() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            refresh_token: None,
            token_type: "Bearer".to_string(),
            expires_at: None,
            scope: None,
        };
        assert!(!token.is_expired()); // No expiry = never expired.
    }

    #[test]
    fn test_build_auth_url() {
        let config = McpAuthConfig {
            auth_url: "https://auth.example.com/authorize".to_string(),
            token_url: "https://auth.example.com/token".to_string(),
            client_id: "my-client".to_string(),
            scopes: "read write".to_string(),
        };
        let (url, verifier) = McpOAuth::build_auth_url(&config, 8880);
        assert!(url.starts_with("https://auth.example.com/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=my-client"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(!verifier.is_empty());
    }

    #[test]
    fn test_find_callback_port() {
        // Should find at least one available port.
        let result = McpOAuth::find_callback_port();
        assert!(result.is_ok());
        let port = result.unwrap();
        assert!((8880..8900).contains(&port));
    }
}
