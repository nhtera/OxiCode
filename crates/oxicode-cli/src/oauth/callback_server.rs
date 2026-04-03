//! Local HTTP callback server for OAuth redirect.
//!
//! Starts a temporary async HTTP listener on localhost (port 8880-8899)
//! to receive the OAuth authorization code redirect from the browser.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// Port range for the OAuth callback listener.
const PORT_RANGE_START: u16 = 8880;
const PORT_RANGE_END: u16 = 8900;

/// Callback server result: the authorization code extracted from the redirect.
pub struct CallbackResult {
    pub code: String,
    pub state: Option<String>,
}

/// Start a local callback server and return (port, receiver).
///
/// The server binds to the first available port in 8880-8899, listens
/// for a single GET request with `?code=...`, sends an HTML success page,
/// then shuts down. Auto-timeout after `timeout_secs`.
pub async fn start_callback_server(
    timeout_secs: u64,
) -> Result<(u16, oneshot::Receiver<CallbackResult>), String> {
    let listener = bind_available_port().await?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local addr: {e}"))?
        .port();

    let (tx, rx) = oneshot::channel();

    // Spawn the listener task — it will auto-shutdown after receiving a code or timeout.
    tokio::spawn(async move {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            accept_callback(&listener),
        )
        .await;

        match result {
            Ok(Ok(callback)) => {
                let _ = tx.send(callback);
            }
            Ok(Err(e)) => {
                tracing::error!("OAuth callback error: {e}");
            }
            Err(_) => {
                tracing::warn!("OAuth callback server timed out after {timeout_secs}s");
            }
        }
    });

    Ok((port, rx))
}

/// Find and bind to the first available port in the range.
async fn bind_available_port() -> Result<TcpListener, String> {
    for port in PORT_RANGE_START..PORT_RANGE_END {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            tracing::info!("OAuth callback server listening on port {port}");
            return Ok(listener);
        }
    }
    Err(format!(
        "No available port in range {PORT_RANGE_START}-{PORT_RANGE_END} for OAuth callback"
    ))
}

/// Accept a single connection and extract the authorization code.
async fn accept_callback(listener: &TcpListener) -> Result<CallbackResult, String> {
    let (mut stream, addr) = listener
        .accept()
        .await
        .map_err(|e| format!("Failed to accept connection: {e}"))?;

    // Only accept connections from localhost.
    if !addr.ip().is_loopback() {
        return Err(format!("Rejected non-localhost connection from {addr}"));
    }

    let (reader, mut writer) = stream.split();
    let mut buf_reader = BufReader::new(reader);
    let mut request_line = String::new();
    buf_reader
        .read_line(&mut request_line)
        .await
        .map_err(|e| format!("Failed to read request: {e}"))?;

    let callback = extract_code_from_request(&request_line)?;

    // Send success HTML response.
    let response = "HTTP/1.1 200 OK\r\n\
        Content-Type: text/html; charset=utf-8\r\n\
        Connection: close\r\n\r\n\
        <html><body style=\"font-family:system-ui;text-align:center;padding:60px\">\
        <h2>Authentication successful!</h2>\
        <p>You can close this window and return to OxiCode.</p>\
        </body></html>";

    let _ = writer.write_all(response.as_bytes()).await;
    let _ = writer.flush().await;

    Ok(callback)
}

/// Extract `code` and optional `state` from an HTTP GET request line.
fn extract_code_from_request(request_line: &str) -> Result<CallbackResult, String> {
    // Format: "GET /callback?code=xxx&state=yyy HTTP/1.1"
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err("Invalid HTTP request format".into());
    }

    let path = parts[1];
    let query = path
        .split('?')
        .nth(1)
        .ok_or("No query parameters in callback URL")?;

    let mut code = None;
    let mut state = None;

    for param in query.split('&') {
        if let Some(value) = param.strip_prefix("code=") {
            if !value.is_empty() {
                code = Some(value.to_string());
            }
        } else if let Some(value) = param.strip_prefix("state=") {
            if !value.is_empty() {
                state = Some(value.to_string());
            }
        }
    }

    let code = code.ok_or("No authorization code found in callback")?;
    Ok(CallbackResult { code, state })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_code_basic() {
        let result = extract_code_from_request("GET /callback?code=abc123 HTTP/1.1").unwrap();
        assert_eq!(result.code, "abc123");
        assert!(result.state.is_none());
    }

    #[test]
    fn test_extract_code_with_state() {
        let result =
            extract_code_from_request("GET /callback?code=abc&state=xyz HTTP/1.1").unwrap();
        assert_eq!(result.code, "abc");
        assert_eq!(result.state.unwrap(), "xyz");
    }

    #[test]
    fn test_extract_code_missing() {
        let result = extract_code_from_request("GET /callback?foo=bar HTTP/1.1");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_code_no_query() {
        let result = extract_code_from_request("GET /callback HTTP/1.1");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_callback_server_binds_port() {
        let result = start_callback_server(5).await;
        assert!(result.is_ok());
        let (port, _rx) = result.unwrap();
        assert!((PORT_RANGE_START..PORT_RANGE_END).contains(&port));
    }
}
