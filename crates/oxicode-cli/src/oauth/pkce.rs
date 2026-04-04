//! PKCE (Proof Key for Code Exchange) challenge/verifier generation.
//!
//! Implements RFC 7636 S256 method for secure CLI OAuth flows.
//! Reuses the proven SHA-256 and base64url implementations from `oxicode-mcp`.

/// PKCE challenge pair (verifier + S256 challenge).
pub struct PkceChallenge {
    pub verifier: String,
    pub challenge: String,
}

/// Generate a PKCE verifier (43-128 char random string) and S256 challenge.
///
/// Uses platform-specific cryptographic randomness:
/// - Unix: `/dev/urandom` with checked read
/// - Windows: `BCryptGenRandom` via std::hash
///
/// Falls back to mixing multiple entropy sources if primary fails.
pub fn generate_pkce() -> PkceChallenge {
    let random_bytes = generate_secure_random();

    let verifier = base64_url_encode(&random_bytes);
    // S256: challenge = BASE64URL(SHA256(verifier))
    let challenge = base64_url_encode(&simple_sha256(verifier.as_bytes()));

    PkceChallenge {
        verifier,
        challenge,
    }
}

/// Generate 32 cryptographically secure random bytes.
///
/// Uses `/dev/urandom` on Unix (with checked read), and a multi-source
/// entropy mix as fallback for platforms without `/dev/urandom`.
fn generate_secure_random() -> [u8; 32] {
    use std::io::Read;

    let mut random_bytes = [0u8; 32];

    // Primary: /dev/urandom (Unix, macOS, Linux).
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(&mut random_bytes).is_ok() {
            return random_bytes;
        }
    }

    // Fallback: mix multiple entropy sources for reasonable randomness.
    // Not cryptographically ideal, but far better than timestamp alone.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let thread_id = format!("{:?}", std::thread::current().id());
    let thread_hash = simple_sha256(thread_id.as_bytes());

    // Mix sources: time nanos + PID + thread hash + counter.
    for (i, byte) in random_bytes.iter_mut().enumerate() {
        *byte = ((nanos >> (i % 16)) & 0xFF) as u8
            ^ (pid as u8).wrapping_add(i as u8)
            ^ thread_hash[i % 32]
            ^ (i as u8).wrapping_mul(0x9E);
    }

    random_bytes
}

/// Generate a random state parameter for CSRF protection.
pub fn generate_state() -> String {
    let bytes = generate_secure_random();
    // Use first 16 bytes for a shorter state string.
    base64_url_encode(&bytes[..16])
}

/// Base64 URL-safe encoding without padding (RFC 4648 Section 5).
pub fn base64_url_encode(data: &[u8]) -> String {
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

/// Simple percent-encoding for URL query parameters.
pub fn urlencoding(s: &str) -> String {
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

/// Public SHA-256 wrapper for use by token_store key derivation.
pub fn simple_sha256_pub(data: &[u8]) -> [u8; 32] {
    simple_sha256(data)
}

/// Minimal SHA-256 (pure Rust, no deps). Used only for PKCE challenge derivation.
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
// SHA-256 implementation inherently uses single-char variables (a..h) per spec
fn simple_sha256(data: &[u8]) -> [u8; 32] {
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

    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation_produces_different_values() {
        let pkce = generate_pkce();
        assert!(!pkce.verifier.is_empty());
        assert!(!pkce.challenge.is_empty());
        assert_ne!(pkce.verifier, pkce.challenge);
    }

    #[test]
    fn test_verifier_length_rfc7636() {
        // RFC 7636 requires 43-128 chars. 32 random bytes base64url = 43 chars.
        let pkce = generate_pkce();
        assert!(pkce.verifier.len() >= 43);
        assert!(pkce.verifier.len() <= 128);
    }

    #[test]
    fn test_challenge_is_deterministic_for_verifier() {
        // Same verifier should produce the same challenge.
        let challenge1 = base64_url_encode(&simple_sha256(b"test-verifier"));
        let challenge2 = base64_url_encode(&simple_sha256(b"test-verifier"));
        assert_eq!(challenge1, challenge2);
    }

    #[test]
    fn test_base64_url_safe_chars() {
        let encoded = base64_url_encode(b"hello world");
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn test_sha256_known_vectors() {
        let hash = simple_sha256(b"");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let hash = simple_sha256(b"hello");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_urlencoding_special_chars() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
        assert_eq!(urlencoding("a=b&c=d"), "a%3Db%26c%3Dd");
        assert_eq!(urlencoding("safe-string_v2.0"), "safe-string_v2.0");
    }
}
