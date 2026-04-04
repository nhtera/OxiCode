//! In-process MCP transport: paired memory channels for bundled servers.
//!
//! Avoids subprocess overhead by using `tokio::sync::mpsc` channels.
//! Two transports are created as a pair — each holds the other's sender.
//! Mirrors the StdioTransport API: `request()`, `notify()`, `shutdown()`.
//!
//! Bidirectional close: shutting down one side marks both sides as closed.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};

use oxicode_common::{OxiError, OxiResult};

use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

/// Buffer size for the mpsc channels (64 messages).
const CHANNEL_BUFFER: usize = 64;

/// Default timeout for in-process requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Shared state between paired transports for bidirectional close.
struct SharedPairState {
    /// When either side closes, both see this as true.
    closed: AtomicBool,
}

/// Type alias for the close callback to reduce type complexity.
type CloseCallback = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

/// In-memory MCP transport using paired tokio mpsc channels.
///
/// Two transports form a pair: A's outbound is B's inbound and vice versa.
/// No subprocess, no I/O — messages stay in memory.
///
/// Bidirectional close: calling `shutdown()` on either side marks both as closed.
pub struct InProcessTransport {
    /// Send messages to the peer transport.
    tx: mpsc::Sender<String>,
    /// Receive messages from the peer transport.
    rx: Arc<Mutex<mpsc::Receiver<String>>>,
    /// Auto-incrementing request ID.
    next_id: AtomicU64,
    /// Shared closed state — both peers see the same flag.
    shared: Arc<SharedPairState>,
    /// Optional close callback — called once when transport closes.
    on_close: CloseCallback,
}

impl InProcessTransport {
    /// Create a linked pair of in-process transports.
    ///
    /// Returns `(client, server)` — client sends requests, server receives them.
    /// Both sides are symmetric; the naming is just a convention.
    ///
    /// Equivalent to `createLinkedTransportPair()` in the MCP SDK.
    pub fn pair() -> (Self, Self) {
        let (tx_a, rx_b) = mpsc::channel(CHANNEL_BUFFER);
        let (tx_b, rx_a) = mpsc::channel(CHANNEL_BUFFER);

        // Shared closed flag — both transports reference the same state.
        let shared = Arc::new(SharedPairState {
            closed: AtomicBool::new(false),
        });

        let transport_a = Self {
            tx: tx_a,
            rx: Arc::new(Mutex::new(rx_a)),
            next_id: AtomicU64::new(1),
            shared: Arc::clone(&shared),
            on_close: Arc::new(Mutex::new(None)),
        };

        let transport_b = Self {
            tx: tx_b,
            rx: Arc::new(Mutex::new(rx_b)),
            next_id: AtomicU64::new(1),
            shared,
            on_close: Arc::new(Mutex::new(None)),
        };

        (transport_a, transport_b)
    }

    /// Set a close callback — called once when the transport closes.
    ///
    /// Set a close callback — called once when the transport closes.
    pub async fn set_on_close(&self, callback: impl FnOnce() + Send + 'static) {
        let mut on_close = self.on_close.lock().await;
        *on_close = Some(Box::new(callback));
    }

    /// Send a JSON-RPC request and wait for a matching response.
    pub async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> OxiResult<serde_json::Value> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(OxiError::Other(
                "InProcess transport is closed".to_string(),
            ));
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);
        let json = serde_json::to_string(&request)
            .map_err(|e| OxiError::Other(format!("Failed to serialize request: {e}")))?;

        // Send the request to the peer.
        self.tx
            .send(json)
            .await
            .map_err(|_| OxiError::Other("InProcess peer channel closed".to_string()))?;

        // Wait for matching response with timeout.
        let result = tokio::time::timeout(REQUEST_TIMEOUT, async {
            let mut rx = self.rx.lock().await;
            loop {
                match rx.recv().await {
                    Some(line) => {
                        if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) {
                            if resp.id == Some(id) {
                                return Ok(resp);
                            }
                            // Notification or mismatched id — skip.
                            tracing::debug!("InProcess: skipping message with id {:?}", resp.id);
                        }
                    }
                    None => {
                        return Err(OxiError::Other(
                            "InProcess peer channel closed".to_string(),
                        ));
                    }
                }
            }
        })
        .await;

        let response = match result {
            Ok(r) => r?,
            Err(_) => {
                return Err(OxiError::Other(format!(
                    "InProcess request '{method}' timed out"
                )));
            }
        };

        if let Some(error) = response.error {
            return Err(OxiError::Other(format!("MCP error: {error}")));
        }

        response
            .result
            .ok_or_else(|| OxiError::Other("MCP response has no result".to_string()))
    }

    /// Send a notification (no response expected).
    pub async fn notify(&self, method: &str, params: Option<serde_json::Value>) -> OxiResult<()> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(OxiError::Other(
                "InProcess transport is closed".to_string(),
            ));
        }

        let notification = JsonRpcNotification::new(method, params);
        let json = serde_json::to_string(&notification)
            .map_err(|e| OxiError::Other(format!("Failed to serialize notification: {e}")))?;

        self.tx
            .send(json)
            .await
            .map_err(|_| OxiError::Other("InProcess peer channel closed".to_string()))?;

        Ok(())
    }

    /// Receive the next raw JSON message from the peer.
    ///
    /// Used by the server side to process incoming requests.
    pub async fn receive(&self) -> OxiResult<String> {
        let mut rx = self.rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| OxiError::Other("InProcess peer channel closed".to_string()))
    }

    /// Send a raw JSON string to the peer.
    ///
    /// Used by the server side to send responses back.
    pub async fn send_raw(&self, json: String) -> OxiResult<()> {
        self.tx
            .send(json)
            .await
            .map_err(|_| OxiError::Other("InProcess peer channel closed".to_string()))
    }

    /// Close this transport and its peer (bidirectional close).
    ///
    /// Closing either side marks both as closed and fires the `on_close` callback.
    /// Idempotent — safe to call multiple times.
    pub async fn shutdown(&self) {
        // Use compare_exchange to ensure we only fire on_close once.
        if self
            .shared
            .closed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // Fire our on_close callback.
            let callback = self.on_close.lock().await.take();
            if let Some(cb) = callback {
                cb();
            }
        }
    }

    /// Check if the transport (or its peer) is closed.
    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool as StdAtomicBool;

    #[tokio::test]
    async fn test_pair_request_response_roundtrip() {
        let (client, server) = InProcessTransport::pair();

        // Spawn a server handler that echoes the request method back.
        let server_handle = tokio::spawn(async move {
            let msg = server.receive().await.unwrap();
            let req: serde_json::Value = serde_json::from_str(&msg).unwrap();

            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": req["id"],
                "result": { "echo": req["method"] }
            });
            server
                .send_raw(serde_json::to_string(&response).unwrap())
                .await
                .unwrap();
        });

        let result = client
            .request("test/method", Some(serde_json::json!({"key": "value"})))
            .await
            .unwrap();

        assert_eq!(result["echo"], "test/method");
        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_pair_notification_no_response() {
        let (client, server) = InProcessTransport::pair();

        let server_handle = tokio::spawn(async move {
            let msg = server.receive().await.unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
            // Notification has no "id" field.
            assert!(parsed.get("id").is_none());
            assert_eq!(parsed["method"], "notifications/initialized");
        });

        client
            .notify("notifications/initialized", None)
            .await
            .unwrap();

        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_closed_transport_errors() {
        let (client, _server) = InProcessTransport::pair();
        client.shutdown().await;

        let err = client
            .request("test", None)
            .await
            .expect_err("should fail on closed transport");
        assert!(err.to_string().contains("closed"));
    }

    #[tokio::test]
    async fn test_multiple_sequential_requests() {
        let (client, server) = InProcessTransport::pair();

        let server_handle = tokio::spawn(async move {
            for _ in 0..3 {
                let msg = server.receive().await.unwrap();
                let req: serde_json::Value = serde_json::from_str(&msg).unwrap();
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req["id"],
                    "result": { "method": req["method"] }
                });
                server
                    .send_raw(serde_json::to_string(&response).unwrap())
                    .await
                    .unwrap();
            }
        });

        for i in 0..3 {
            let method = format!("method_{i}");
            let result = client.request(&method, None).await.unwrap();
            assert_eq!(result["method"], method);
        }

        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_peer_drop_returns_error() {
        let (client, server) = InProcessTransport::pair();
        drop(server);

        let err = client
            .request("test", None)
            .await
            .expect_err("should fail when peer is dropped");
        assert!(err.to_string().contains("closed"));
    }

    #[tokio::test]
    async fn test_bidirectional_close_from_client() {
        let (client, server) = InProcessTransport::pair();

        // Close from client side — server should also see closed.
        client.shutdown().await;

        assert!(client.is_closed());
        assert!(server.is_closed(), "peer should also be closed");

        // Server operations should fail.
        let err = server.notify("test", None).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn test_bidirectional_close_from_server() {
        let (client, server) = InProcessTransport::pair();

        // Close from server side — client should also see closed.
        server.shutdown().await;

        assert!(server.is_closed());
        assert!(client.is_closed(), "peer should also be closed");
    }

    #[tokio::test]
    async fn test_shutdown_idempotent() {
        let (client, _server) = InProcessTransport::pair();

        // Multiple shutdown calls should be safe.
        client.shutdown().await;
        client.shutdown().await;
        client.shutdown().await;

        assert!(client.is_closed());
    }

    #[tokio::test]
    async fn test_on_close_callback_fires() {
        let (client, _server) = InProcessTransport::pair();
        let callback_fired = Arc::new(StdAtomicBool::new(false));
        let flag = Arc::clone(&callback_fired);

        client
            .set_on_close(move || {
                flag.store(true, Ordering::Release);
            })
            .await;

        client.shutdown().await;
        assert!(callback_fired.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_on_close_callback_fires_once() {
        let (client, _server) = InProcessTransport::pair();
        let call_count = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&call_count);

        client
            .set_on_close(move || {
                counter.fetch_add(1, Ordering::Relaxed);
            })
            .await;

        // Multiple shutdowns — callback should fire only once.
        client.shutdown().await;
        client.shutdown().await;
        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }
}
