//! JWT authentication for the bridge WebSocket server.
//!
//! Uses HS256 (HMAC-SHA256) with optional `iss`/`aud` validation.
//! Secret is read from `OXICODE_BRIDGE_JWT_SECRET` — **never logged**.

use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

/// Claims extracted from a verified JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — user / client identifier.
    pub sub: String,
    /// Expiration time (Unix seconds). Required by default validation.
    pub exp: u64,
    /// Issuer (optional — validated if `iss_expected` is set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    /// Audience (optional — validated if `aud_expected` is set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
}

/// Configuration for JWT validation.
#[derive(Debug, Clone, Default)]
pub struct JwtConfig {
    /// Expected issuer (`iss` claim). `None` disables iss validation.
    pub iss: Option<String>,
    /// Expected audience (`aud` claim). `None` disables aud validation.
    pub aud: Option<String>,
}

/// Error returned when JWT verification fails.
#[derive(Debug, thiserror::Error)]
pub enum JwtError {
    #[error("JWT validation failed: {0}")]
    Invalid(#[from] jsonwebtoken::errors::Error),
    #[error("JWT is missing required `exp` claim")]
    MissingExp,
    #[error("JWT has expired")]
    Expired,
    #[error("JWT `sub` claim is empty")]
    EmptySub,
}

/// Verify a JWT token using HS256 and the given secret.
///
/// Validates: signature, expiry, sub presence, and optionally iss/aud.
/// The `secret` slice is **never** logged or included in error messages.
pub fn verify_token(token: &str, secret: &[u8], config: &JwtConfig) -> Result<Claims, JwtError> {
    let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);

    // Require exp — jsonwebtoken validates it automatically when set.
    validation.validate_exp = true;
    // Zero leeway: tokens must not be expired at the moment of validation.
    // The default 60-second leeway would allow recently-expired tokens through.
    validation.leeway = 0;

    // Configure aud validation only when caller provides an expected value.
    if let Some(ref aud) = config.aud {
        validation.set_audience(&[aud.as_str()]);
    } else {
        // Disable aud validation when no expected value is provided.
        validation.validate_aud = false;
    }

    // Configure iss validation only when caller provides an expected value.
    if let Some(ref iss) = config.iss {
        validation.set_issuer(&[iss.as_str()]);
    }

    let key = DecodingKey::from_secret(secret);
    let token_data = decode::<Claims>(token, &key, &validation)?;

    let claims = token_data.claims;

    // Belt-and-suspenders: verify sub is not empty.
    if claims.sub.trim().is_empty() {
        return Err(JwtError::EmptySub);
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    const TEST_SECRET: &[u8] = b"test-secret-that-is-long-enough-for-hs256";

    fn make_claims(exp_offset_secs: i64) -> Claims {
        let exp = (chrono::Utc::now().timestamp() + exp_offset_secs) as u64;
        Claims {
            sub: "test-user".to_string(),
            exp,
            iss: None,
            aud: None,
        }
    }

    fn sign(claims: &Claims, secret: &[u8]) -> String {
        encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(secret),
        )
        .expect("encode failed in test")
    }

    #[test]
    fn test_valid_token() {
        let claims = make_claims(3600);
        let token = sign(&claims, TEST_SECRET);
        let result = verify_token(&token, TEST_SECRET, &JwtConfig::default());
        assert!(result.is_ok());
        let c = result.unwrap();
        assert_eq!(c.sub, "test-user");
    }

    #[test]
    fn test_expired_token() {
        // exp in the past → should fail.
        let claims = make_claims(-60);
        let token = sign(&claims, TEST_SECRET);
        let result = verify_token(&token, TEST_SECRET, &JwtConfig::default());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // jsonwebtoken reports this as ExpiredSignature
        assert!(
            err.contains("ExpiredSignature") || err.contains("expired") || err.contains("JWT"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_wrong_signature() {
        let claims = make_claims(3600);
        let token = sign(&claims, b"wrong-secret-entirely-different");
        let result = verify_token(&token, TEST_SECRET, &JwtConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_token() {
        let result = verify_token("not.a.jwt", TEST_SECRET, &JwtConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_sub_rejected() {
        let exp = (chrono::Utc::now().timestamp() + 3600) as u64;
        let claims = Claims {
            sub: "  ".to_string(),
            exp,
            iss: None,
            aud: None,
        };
        let token = sign(&claims, TEST_SECRET);
        let result = verify_token(&token, TEST_SECRET, &JwtConfig::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sub"));
    }

    #[test]
    fn test_iss_validation_pass() {
        let exp = (chrono::Utc::now().timestamp() + 3600) as u64;
        let claims = Claims {
            sub: "user".to_string(),
            exp,
            iss: Some("my-issuer".to_string()),
            aud: None,
        };
        let token = sign(&claims, TEST_SECRET);
        let config = JwtConfig {
            iss: Some("my-issuer".to_string()),
            aud: None,
        };
        assert!(verify_token(&token, TEST_SECRET, &config).is_ok());
    }

    #[test]
    fn test_iss_validation_fail() {
        let exp = (chrono::Utc::now().timestamp() + 3600) as u64;
        let claims = Claims {
            sub: "user".to_string(),
            exp,
            iss: Some("wrong-issuer".to_string()),
            aud: None,
        };
        let token = sign(&claims, TEST_SECRET);
        let config = JwtConfig {
            iss: Some("expected-issuer".to_string()),
            aud: None,
        };
        assert!(verify_token(&token, TEST_SECRET, &config).is_err());
    }
}
