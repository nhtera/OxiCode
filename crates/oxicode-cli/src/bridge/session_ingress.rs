//! Session ingress — HMAC-SHA256 token generation, validation, and session routing.
//!
//! Provides:
//! - Generate short-lived ingress tokens for bridge WebSocket connections
//! - Validate token signature + expiry
//! - Route validated tokens to active sessions

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::debug;

type HmacSha256 = Hmac<Sha256>;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default token lifetime: 24 hours.
const TOKEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Environment variable for the ingress secret.
pub const INGRESS_SECRET_ENV: &str = "OXICODE_INGRESS_SECRET";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Claims embedded in an ingress token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IngressClaims {
    pub session_id: String,
    /// Seconds since UNIX epoch.
    pub created_at: u64,
    /// Seconds since UNIX epoch.
    pub expires_at: u64,
}

/// Errors from ingress token operations.
#[derive(Debug, thiserror::Error)]
pub enum IngressError {
    #[error("invalid token encoding")]
    InvalidEncoding,

    #[error("invalid token payload")]
    InvalidPayload,

    #[error("invalid HMAC signature")]
    InvalidSignature,

    #[error("token expired")]
    TokenExpired,

    #[error("session not found: {0}")]
    SessionNotFound(String),
}

/// Opaque handle to a routed session.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// Token generation
// ---------------------------------------------------------------------------

/// Generate an HMAC-SHA256 ingress token for the given session.
///
/// Token format: `base64url(payload_json) . base64url(hmac_signature)`
pub fn generate_ingress_token(session_id: &str, secret: &[u8]) -> String {
    let now = current_epoch_secs();

    let claims = IngressClaims {
        session_id: session_id.to_string(),
        created_at: now,
        expires_at: now + TOKEN_TTL.as_secs(),
    };

    let payload_json = serde_json::to_string(&claims).expect("claims serialize");
    let payload_b64 = base64_url_encode(payload_json.as_bytes());

    let signature = compute_hmac(secret, payload_b64.as_bytes());
    let sig_b64 = base64_url_encode(&signature);

    format!("{payload_b64}.{sig_b64}")
}

// ---------------------------------------------------------------------------
// Token validation
// ---------------------------------------------------------------------------

/// Validate an ingress token and return its claims.
///
/// Checks:
/// 1. Token format (`payload.signature`)
/// 2. HMAC-SHA256 signature
/// 3. Expiry
pub fn validate_ingress_token(
    token: &str,
    secret: &[u8],
) -> Result<IngressClaims, IngressError> {
    let (payload_b64, sig_b64) = token
        .split_once('.')
        .ok_or(IngressError::InvalidEncoding)?;

    // Verify signature.
    let expected_sig = compute_hmac(secret, payload_b64.as_bytes());
    let provided_sig =
        base64_url_decode(sig_b64).map_err(|_| IngressError::InvalidEncoding)?;

    if !constant_time_eq(&expected_sig, &provided_sig) {
        return Err(IngressError::InvalidSignature);
    }

    // Decode payload.
    let payload_bytes =
        base64_url_decode(payload_b64).map_err(|_| IngressError::InvalidEncoding)?;
    let claims: IngressClaims =
        serde_json::from_slice(&payload_bytes).map_err(|_| IngressError::InvalidPayload)?;

    // Check expiry.
    let now = current_epoch_secs();
    if now > claims.expires_at {
        return Err(IngressError::TokenExpired);
    }

    debug!(session_id = %claims.session_id, "ingress token validated");
    Ok(claims)
}

// ---------------------------------------------------------------------------
// Session routing
// ---------------------------------------------------------------------------

/// Route an ingress token's claims to an active session.
///
/// `sessions` is a map of session_id → any value (presence = active).
pub fn route_to_session<V>(
    claims: &IngressClaims,
    sessions: &HashMap<String, V>,
) -> Result<SessionHandle, IngressError> {
    if sessions.contains_key(&claims.session_id) {
        Ok(SessionHandle {
            session_id: claims.session_id.clone(),
        })
    } else {
        Err(IngressError::SessionNotFound(claims.session_id.clone()))
    }
}

// ---------------------------------------------------------------------------
// Secret management
// ---------------------------------------------------------------------------

/// Load ingress secret from `OXICODE_INGRESS_SECRET` env var, or generate a
/// cryptographically random 32-byte secret if not set.
pub fn load_or_generate_secret() -> Vec<u8> {
    if let Ok(val) = std::env::var(INGRESS_SECRET_ENV) {
        if !val.is_empty() {
            return val.into_bytes();
        }
    }

    // Generate 32 cryptographically random bytes via `rand` (uses OS CSPRNG).
    let mut secret = vec![0u8; 32];
    rand::fill(&mut secret[..]);

    tracing::info!("generated random ingress secret (set {INGRESS_SECRET_ENV} to persist)");
    secret
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute HMAC-SHA256 of `data` with `key`.
fn compute_hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac =
        HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Constant-time comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Base64url encode without padding (RFC 4648 §5).
fn base64_url_encode(data: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(data)
}

/// Base64url decode without padding (RFC 4648 §5).
fn base64_url_decode(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.decode(data)
}

/// Current time as seconds since UNIX epoch.
fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &[u8] = b"test-secret-key-for-unit-tests";

    #[test]
    fn token_roundtrip() {
        let token = generate_ingress_token("sess-123", TEST_SECRET);
        let claims = validate_ingress_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.session_id, "sess-123");
        assert!(claims.expires_at > claims.created_at);
    }

    #[test]
    fn token_invalid_signature() {
        let token = generate_ingress_token("sess-123", TEST_SECRET);
        let result = validate_ingress_token(&token, b"wrong-secret");
        assert!(matches!(result, Err(IngressError::InvalidSignature)));
    }

    #[test]
    fn token_tampered_payload() {
        let token = generate_ingress_token("sess-123", TEST_SECRET);
        // Tamper by replacing payload with different content but same format.
        let parts: Vec<&str> = token.splitn(2, '.').collect();
        let tampered = format!("AAAA{}.{}", &parts[0][4..], parts[1]);
        let result = validate_ingress_token(&tampered, TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn token_malformed() {
        assert!(matches!(
            validate_ingress_token("not-a-token", TEST_SECRET),
            Err(IngressError::InvalidEncoding)
        ));
    }

    #[test]
    fn token_expired() {
        // Manually craft an expired token.
        let claims = IngressClaims {
            session_id: "expired-sess".to_string(),
            created_at: 1000,
            expires_at: 2000, // well in the past
        };
        let payload_json = serde_json::to_string(&claims).unwrap();
        let payload_b64 = base64_url_encode(payload_json.as_bytes());
        let sig = compute_hmac(TEST_SECRET, payload_b64.as_bytes());
        let sig_b64 = base64_url_encode(&sig);
        let token = format!("{payload_b64}.{sig_b64}");

        assert!(matches!(
            validate_ingress_token(&token, TEST_SECRET),
            Err(IngressError::TokenExpired)
        ));
    }

    #[test]
    fn route_to_existing_session() {
        let mut sessions = HashMap::new();
        sessions.insert("sess-1".to_string(), ());
        let claims = IngressClaims {
            session_id: "sess-1".to_string(),
            created_at: 0,
            expires_at: u64::MAX,
        };
        let handle = route_to_session(&claims, &sessions).unwrap();
        assert_eq!(handle.session_id, "sess-1");
    }

    #[test]
    fn route_to_missing_session() {
        let sessions: HashMap<String, ()> = HashMap::new();
        let claims = IngressClaims {
            session_id: "nonexistent".to_string(),
            created_at: 0,
            expires_at: u64::MAX,
        };
        let result = route_to_session(&claims, &sessions);
        assert!(matches!(result, Err(IngressError::SessionNotFound(_))));
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn base64url_roundtrip() {
        let data = b"test data with special chars: +/=";
        let encoded = base64_url_encode(data);
        let decoded = base64_url_decode(&encoded).unwrap();
        assert_eq!(&decoded, data);
    }

    #[test]
    fn load_or_generate_secret_returns_32_bytes() {
        // Without env var set, should generate 32 bytes.
        let secret = load_or_generate_secret();
        assert_eq!(secret.len(), 32);
    }

    #[test]
    fn error_display() {
        assert_eq!(
            IngressError::SessionNotFound("s1".into()).to_string(),
            "session not found: s1"
        );
        assert_eq!(
            IngressError::TokenExpired.to_string(),
            "token expired"
        );
    }
}
