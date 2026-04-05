//! DNS-pinning resolver — caches resolved IPs per-host to prevent DNS rebinding.
//!
//! Resolves hostname once, caches the result with a 30s TTL, and validates
//! that resolved IPs are not in private ranges. Prevents TOCTOU attacks where
//! DNS resolution changes between SSRF check and actual connection.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// TTL for cached DNS entries — 30 seconds.
const DNS_CACHE_TTL: Duration = Duration::from_secs(30);

/// Cached DNS resolution entry.
struct DnsEntry {
    addrs: Vec<SocketAddr>,
    resolved_at: Instant,
}

/// DNS-pinning resolver that caches resolved addresses per host.
///
/// After first resolution, subsequent lookups for the same host within TTL
/// return the cached addresses — preventing DNS rebinding between validation
/// and connection.
pub struct PinnedResolver {
    cache: Mutex<HashMap<String, DnsEntry>>,
}

impl PinnedResolver {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Resolve a hostname to socket addresses with caching + private IP validation.
    ///
    /// Returns cached addresses if within TTL, otherwise re-resolves.
    /// Rejects any resolution that includes private/loopback IPs.
    pub fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        let cache_key = format!("{host}:{port}");

        // Check cache first.
        if let Some(entry) = self.get_cached(&cache_key) {
            return Ok(entry);
        }

        // Resolve and validate.
        let addr_str = format!("{host}:{port}");
        let addrs: Vec<SocketAddr> = addr_str
            .to_socket_addrs()
            .map_err(|e| format!("DNS resolution failed for '{host}': {e}"))?
            .collect();

        if addrs.is_empty() {
            return Err(format!("DNS resolution returned no addresses for '{host}'"));
        }

        // Validate: reject private/loopback IPs.
        for addr in &addrs {
            if is_private_ip(&addr.ip()) {
                return Err(format!(
                    "DNS rebinding: '{host}' resolved to private IP {}",
                    addr.ip()
                ));
            }
        }

        // Cache the result.
        self.put_cached(cache_key, &addrs);

        Ok(addrs)
    }

    /// Get cached entry if exists and not expired.
    fn get_cached(&self, key: &str) -> Option<Vec<SocketAddr>> {
        let cache = self.cache.lock().ok()?;
        let entry = cache.get(key)?;
        if entry.resolved_at.elapsed() < DNS_CACHE_TTL {
            Some(entry.addrs.clone())
        } else {
            None
        }
    }

    /// Store resolved addresses in cache.
    fn put_cached(&self, key: String, addrs: &[SocketAddr]) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                key,
                DnsEntry {
                    addrs: addrs.to_vec(),
                    resolved_at: Instant::now(),
                },
            );
        }
    }
}

impl Default for PinnedResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if an IP address is private, loopback, or link-local.
///
/// Used by both `PinnedResolver` and `http_hook_executor` SSRF validation.
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // link-local
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // unique local
                || v6.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.is_unspecified()
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_private_ipv4_rejected() {
        assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"192.168.1.1".parse().unwrap()));
        assert!(is_private_ip(&"0.0.0.0".parse().unwrap()));
    }

    #[test]
    fn test_public_ipv4_allowed() {
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn test_loopback_ipv6_rejected() {
        assert!(is_private_ip(&"::1".parse().unwrap()));
    }

    #[test]
    fn test_resolver_caches_results() {
        let resolver = PinnedResolver::new();
        // Resolve a public hostname — should succeed and cache.
        let result = resolver.resolve("example.com", 443);
        assert!(result.is_ok());
        let addrs = result.unwrap();
        assert!(!addrs.is_empty());

        // Second call should hit cache and return same result.
        let result2 = resolver.resolve("example.com", 443);
        assert!(result2.is_ok());
        assert_eq!(result2.unwrap().len(), addrs.len());
    }

    #[test]
    fn test_resolver_rejects_private_host() {
        let resolver = PinnedResolver::new();
        let result = resolver.resolve("127.0.0.1", 80);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("private IP"));
    }

    #[test]
    fn test_resolver_bad_hostname() {
        let resolver = PinnedResolver::new();
        let result = resolver.resolve("this-host-does-not-exist-12345.invalid", 443);
        assert!(result.is_err());
    }
}
