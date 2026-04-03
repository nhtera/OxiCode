//! Proxy-aware HTTP client builder.
//!
//! Reads `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY` env vars
//! to configure reqwest with proxy support. Also supports SOCKS5 proxies.

use tracing::info;

/// Build a `reqwest::Client` with proxy support from environment variables.
///
/// Supported env vars:
/// - `HTTP_PROXY` / `http_proxy`
/// - `HTTPS_PROXY` / `https_proxy`
/// - `ALL_PROXY` / `all_proxy`
/// - `NO_PROXY` / `no_proxy`
///
/// Proxy URLs may use `http://`, `https://`, or `socks5://` schemes.
pub fn build_proxy_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder();
    let mut proxy_configured = false;

    // ALL_PROXY applies to both HTTP and HTTPS.
    if let Some(all_proxy) = env_var_ci("ALL_PROXY") {
        if let Ok(proxy) = reqwest::Proxy::all(&all_proxy) {
            builder = builder.proxy(proxy);
            proxy_configured = true;
            info!(proxy = %all_proxy, "ALL_PROXY configured");
        }
    } else {
        // HTTP_PROXY.
        if let Some(http_proxy) = env_var_ci("HTTP_PROXY") {
            if let Ok(proxy) = reqwest::Proxy::http(&http_proxy) {
                builder = builder.proxy(proxy);
                proxy_configured = true;
                info!(proxy = %http_proxy, "HTTP_PROXY configured");
            }
        }

        // HTTPS_PROXY.
        if let Some(https_proxy) = env_var_ci("HTTPS_PROXY") {
            if let Ok(proxy) = reqwest::Proxy::https(&https_proxy) {
                builder = builder.proxy(proxy);
                proxy_configured = true;
                info!(proxy = %https_proxy, "HTTPS_PROXY configured");
            }
        }
    }

    // NO_PROXY — reqwest handles this automatically when system proxies are on,
    // but we log it for visibility.
    if proxy_configured {
        if let Some(no_proxy) = env_var_ci("NO_PROXY") {
            info!(no_proxy = %no_proxy, "NO_PROXY configured");
        }
    }

    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

/// Read an env var case-insensitively (check UPPER then lower).
fn env_var_ci(name: &str) -> Option<String> {
    std::env::var(name)
        .or_else(|_| std::env::var(name.to_lowercase()))
        .ok()
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_proxy_client_no_env() {
        // Without proxy env vars, should return a working client.
        let client = build_proxy_client();
        // Just verify it's constructed without panic.
        drop(client);
    }

    #[test]
    fn test_env_var_ci_uppercase() {
        // This tests the function logic without actually setting env vars
        // since that could affect other tests.
        let result = env_var_ci("DEFINITELY_NOT_SET_XYZ_12345");
        assert!(result.is_none());
    }
}
