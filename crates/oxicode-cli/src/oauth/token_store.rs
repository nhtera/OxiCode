//! Encrypted token persistence for OAuth credentials.
//!
//! Stores tokens as JSON at `~/.oxicode/oauth.json` with 0600 permissions.
//! Uses XOR-based obfuscation with a machine-derived key for at-rest protection.
//! (Not cryptographically strong — defense-in-depth; primary security is file permissions.)

use std::path::{Path, PathBuf};

use super::client::OAuthTokenData;

/// Default token file path: `~/.oxicode/oauth.json`.
fn default_token_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".oxicode").join("oauth.json"))
}

/// Save OAuth tokens to disk with restricted permissions.
pub fn save_tokens(tokens: &OAuthTokenData) -> Result<(), String> {
    let path = default_token_path().ok_or("Cannot determine home directory")?;
    save_tokens_to(tokens, &path)
}

/// Save OAuth tokens to a specific path (for testing).
pub fn save_tokens_to(tokens: &OAuthTokenData, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create token directory: {e}"))?;
    }

    let json = serde_json::to_string_pretty(tokens)
        .map_err(|e| format!("Failed to serialize tokens: {e}"))?;

    // Obfuscate the JSON before writing.
    let key = derive_machine_key();
    let obfuscated = xor_obfuscate(json.as_bytes(), &key);

    std::fs::write(path, obfuscated).map_err(|e| format!("Failed to write token file: {e}"))?;

    // Set restrictive permissions (Unix only).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }

    Ok(())
}

/// Load OAuth tokens from disk.
pub fn load_tokens() -> Option<OAuthTokenData> {
    let path = default_token_path()?;
    load_tokens_from(&path)
}

/// Load OAuth tokens from a specific path (for testing).
pub fn load_tokens_from(path: &Path) -> Option<OAuthTokenData> {
    let data = std::fs::read(path).ok()?;
    let key = derive_machine_key();
    let deobfuscated = xor_obfuscate(&data, &key);
    let json = String::from_utf8(deobfuscated).ok()?;
    serde_json::from_str(&json).ok()
}

/// Clear stored tokens by deleting the file.
pub fn clear_tokens() -> Result<(), String> {
    let path = default_token_path().ok_or("Cannot determine home directory")?;
    clear_tokens_at(&path)
}

/// Clear tokens at a specific path (for testing).
pub fn clear_tokens_at(path: &Path) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("Failed to remove token file: {e}"))?;
    }
    Ok(())
}

/// Derive a machine-specific key for token obfuscation.
///
/// Combines hostname + username + a fixed salt to produce a reproducible key.
/// This is NOT cryptographic encryption — it's obfuscation to prevent
/// casual inspection. Real security comes from file permissions (0600).
fn derive_machine_key() -> [u8; 32] {
    let mut seed = String::from("oxicode-oauth-v1:");

    // Append hostname.
    if let Ok(hostname) = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()))
    {
        seed.push_str(&hostname);
    }

    // Append username.
    if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
        seed.push_str(&user);
    }

    // Hash the seed to get a fixed-size key using SHA-256 for better mixing.
    let seed_hash =
        super::pkce::base64_url_encode(&super::pkce::simple_sha256_pub(seed.as_bytes()));
    let mut key = [0u8; 32];
    let key_bytes = seed_hash.as_bytes();
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = key_bytes[i % key_bytes.len()] ^ (i as u8).wrapping_mul(0x5A);
    }
    key
}

/// XOR-based obfuscation (symmetric — same function for encode/decode).
fn xor_obfuscate(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &byte)| byte ^ key[i % 32])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xor_roundtrip() {
        let key = derive_machine_key();
        let original = b"hello world secret token data";
        let encrypted = xor_obfuscate(original, &key);
        let decrypted = xor_obfuscate(&encrypted, &key);
        assert_eq!(&decrypted, original);
    }

    #[test]
    fn test_xor_obfuscates_data() {
        let key = derive_machine_key();
        let original = b"secret-token-data";
        let encrypted = xor_obfuscate(original, &key);
        // Encrypted should differ from original.
        assert_ne!(&encrypted[..], &original[..]);
    }

    #[test]
    fn test_machine_key_deterministic() {
        let key1 = derive_machine_key();
        let key2 = derive_machine_key();
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-oauth.json");

        let tokens = OAuthTokenData {
            access_token: "test-access-token".into(),
            refresh_token: Some("test-refresh-token".into()),
            token_type: "Bearer".into(),
            expires_at: Some(9_999_999_999),
            email: Some("user@example.com".into()),
        };

        save_tokens_to(&tokens, &path).unwrap();
        assert!(path.exists());

        let loaded = load_tokens_from(&path).unwrap();
        assert_eq!(loaded.access_token, "test-access-token");
        assert_eq!(loaded.refresh_token.unwrap(), "test-refresh-token");
        assert_eq!(loaded.email.unwrap(), "user@example.com");
    }

    #[test]
    fn test_clear_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-oauth.json");

        let tokens = OAuthTokenData {
            access_token: "tok".into(),
            refresh_token: None,
            token_type: "Bearer".into(),
            expires_at: None,
            email: None,
        };

        save_tokens_to(&tokens, &path).unwrap();
        assert!(path.exists());

        clear_tokens_at(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let result = load_tokens_from(std::path::Path::new("/tmp/nonexistent-oauth.json"));
        assert!(result.is_none());
    }
}
