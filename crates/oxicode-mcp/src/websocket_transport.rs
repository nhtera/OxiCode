//! WebSocket transport for MCP: bidirectional JSON-RPC over WebSocket.
//!
//! Supports both `ws://` and `wss://` (TLS) connections.
//! Reconnects automatically with exponential backoff on disconnect.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use oxicode_common::{OxiError, OxiResult};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Default timeout for WebSocket requests.
const WS_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum reconnection attempts.
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
/// Base delay for exponential backoff (doubles each attempt).
const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(500);

type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type WsStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// WebSocket-based MCP transport for bidirectional JSON-RPC communication.
pub struct WebSocketTransport {
    url: String,
    sink: Arc<Mutex<WsSink>>,
    stream: Arc<Mutex<WsStream>>,
    next_id: AtomicU64,
}

impl WebSocketTransport {
    /// Connect to a WebSocket MCP server.
    pub async fn connect(url: &str) -> OxiResult<Self> {
        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| OxiError::Other(format!("WebSocket connect to '{url}' failed: {e}")))?;

        let (sink, stream) = ws_stream.split();

        tracing::info!("WebSocket MCP connected to {url}");

        Ok(Self {
            url: url.to_string(),
            sink: Arc::new(Mutex::new(sink)),
            stream: Arc::new(Mutex::new(stream)),
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a JSON-RPC request and wait for a matching response.
    pub async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> OxiResult<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);
        let request_json = serde_json::to_string(&request)
            .map_err(|e| OxiError::Other(format!("Serialize failed: {e}")))?;

        let result = tokio::time::timeout(WS_REQUEST_TIMEOUT, async {
            // Send request.
            {
                let mut sink = self.sink.lock().await;
                sink.send(Message::Text(request_json))
                    .await
                    .map_err(|e| OxiError::Other(format!("WebSocket send failed: {e}")))?;
            }

            // Read responses until matching id.
            let mut stream = self.stream.lock().await;
            loop {
                match stream.next().await {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&text) {
                            if resp.id == Some(id) {
                                return Ok(resp);
                            }
                            tracing::debug!("WS: skipping message with id {:?}", resp.id);
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        return Err(OxiError::Other("WebSocket closed by server".into()));
                    }
                    Some(Ok(_)) => {} // Ping/Pong/Binary — skip.
                    Some(Err(e)) => {
                        return Err(OxiError::Other(format!("WebSocket read error: {e}")));
                    }
                    None => {
                        return Err(OxiError::Other("WebSocket stream ended".into()));
                    }
                }
            }
        })
        .await;

        let response = match result {
            Ok(r) => r?,
            Err(_) => {
                return Err(OxiError::Other(format!(
                    "WebSocket request '{method}' timed out"
                )));
            }
        };

        if let Some(error) = response.error {
            return Err(OxiError::Other(format!("WS MCP error: {error}")));
        }

        response
            .result
            .ok_or_else(|| OxiError::Other("WS response has no result".into()))
    }

    /// Attempt to reconnect with exponential backoff.
    pub async fn reconnect(&mut self) -> OxiResult<()> {
        for attempt in 0..MAX_RECONNECT_ATTEMPTS {
            let delay = RECONNECT_BASE_DELAY * 2u32.pow(attempt);
            tracing::info!(
                "WebSocket reconnect attempt {} (delay {}ms)",
                attempt + 1,
                delay.as_millis()
            );
            tokio::time::sleep(delay).await;

            match connect_async(&self.url).await {
                Ok((ws_stream, _)) => {
                    let (sink, stream) = ws_stream.split();
                    *self.sink.lock().await = sink;
                    *self.stream.lock().await = stream;
                    tracing::info!("WebSocket reconnected to {}", self.url);
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("Reconnect attempt {} failed: {e}", attempt + 1);
                }
            }
        }

        Err(OxiError::Other(format!(
            "WebSocket reconnect failed after {MAX_RECONNECT_ATTEMPTS} attempts"
        )))
    }

    /// Close the WebSocket connection gracefully.
    pub async fn close(&self) {
        let mut sink = self.sink.lock().await;
        let _ = sink.send(Message::Close(None)).await;
        let _ = sink.close().await;
    }
}
