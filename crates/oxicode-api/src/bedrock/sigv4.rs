//! Minimal AWS SigV4 request signing for Bedrock Runtime.

use std::fmt::Write;

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Parameters for SigV4 request signing.
pub struct SignParams<'a> {
    pub url: &'a str,
    pub host: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub session_token: Option<&'a str>,
    pub body: &'a [u8],
}

/// Sign an HTTP POST request with AWS SigV4 and return the headers to add.
pub fn sign_request(params: &SignParams<'_>) -> Vec<(String, String)> {
    let now = Utc::now();
    let datestamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

    // Extract path from URL.
    let path = params
        .url
        .find("://")
        .and_then(|i| params.url[i + 3..].find('/'))
        .map_or("/", |i| {
            let start = params.url.find("://").unwrap() + 3 + i;
            &params.url[start..]
        });

    let payload_hash = hex::encode(Sha256::digest(params.body));

    // Canonical headers (must be sorted).
    let mut canonical_headers = format!(
        "content-type:application/json\nhost:{}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n",
        params.host
    );
    let mut signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date".to_string();

    if let Some(token) = params.session_token {
        let _ = writeln!(canonical_headers, "x-amz-security-token:{token}");
        signed_headers.push_str(";x-amz-security-token");
    }

    let canonical_request =
        format!("POST\n{path}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}");

    let credential_scope = format!(
        "{datestamp}/{}/{}/aws4_request",
        params.region, params.service
    );
    let canonical_request_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));

    let string_to_sign =
        format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{canonical_request_hash}");

    // Derive signing key.
    let k_date = hmac_sha256(
        format!("AWS4{}", params.secret_key).as_bytes(),
        datestamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, params.region.as_bytes());
    let k_service = hmac_sha256(&k_region, params.service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");

    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        params.access_key
    );

    let mut headers = vec![
        ("host".to_string(), params.host.to_string()),
        ("x-amz-date".to_string(), amz_date),
        ("x-amz-content-sha256".to_string(), payload_hash),
        ("authorization".to_string(), authorization),
    ];

    if let Some(token) = params.session_token {
        headers.push(("x-amz-security-token".to_string(), token.to_string()));
    }

    headers
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_request_returns_required_headers() {
        let headers = sign_request(&SignParams {
            url: "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-3-sonnet/invoke-with-response-stream",
            host: "bedrock-runtime.us-east-1.amazonaws.com",
            region: "us-east-1",
            service: "bedrock",
            access_key: "AKIAIOSFODNN7EXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            session_token: None,
            body: b"{}",
        });

        let header_names: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
        assert!(header_names.contains(&"authorization"));
        assert!(header_names.contains(&"x-amz-date"));
        assert!(header_names.contains(&"x-amz-content-sha256"));

        let auth = headers.iter().find(|(k, _)| k == "authorization").unwrap();
        assert!(auth.1.starts_with("AWS4-HMAC-SHA256"));
    }

    #[test]
    fn test_sign_with_session_token() {
        let headers = sign_request(&SignParams {
            url: "https://bedrock-runtime.us-east-1.amazonaws.com/model/test/invoke",
            host: "bedrock-runtime.us-east-1.amazonaws.com",
            region: "us-east-1",
            service: "bedrock",
            access_key: "AKID",
            secret_key: "SECRET",
            session_token: Some("SESSION_TOKEN"),
            body: b"{}",
        });

        let header_names: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
        assert!(header_names.contains(&"x-amz-security-token"));
    }
}
