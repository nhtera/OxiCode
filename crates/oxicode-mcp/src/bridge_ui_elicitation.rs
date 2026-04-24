//! Channel-backed `ElicitationHandler` that forwards requests to an async UI
//! (typically the TUI) and blocks the sync trait method until the UI replies.
//!
//! Pattern mirrors `PendingPermission`: the handler owns an `mpsc::UnboundedSender`
//! to deliver a `(request, oneshot::Sender<response>)` pair; `handle()` creates
//! the oneshot pair, forwards, then blocks on `reply_rx.blocking_recv()`.

use tokio::sync::{mpsc, oneshot};

use crate::elicitation::{ElicitationHandler, ElicitationRequest, ElicitationResponse};

/// One forwarded elicitation request + the reply channel the UI writes to.
pub type ElicitationEnvelope = (ElicitationRequest, oneshot::Sender<ElicitationResponse>);

/// `ElicitationHandler` that dispatches each request over a channel to the UI.
///
/// `handle()` blocks the calling thread until the UI sends the response.
/// Callers running inside a tokio runtime MUST invoke this via
/// `spawn_blocking` (or from a blocking task), since `blocking_recv()`
/// panics on the current runtime thread.
pub struct ChannelElicitationHandler {
    sender: mpsc::UnboundedSender<ElicitationEnvelope>,
}

impl ChannelElicitationHandler {
    /// Create a handler + matching receiver. Pass the receiver to the UI task.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<ElicitationEnvelope>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { sender: tx }, rx)
    }

    /// Wrap an existing sender (useful when the UI already owns the channel).
    pub fn from_sender(sender: mpsc::UnboundedSender<ElicitationEnvelope>) -> Self {
        Self { sender }
    }

    /// Build a denial response (used on channel errors).
    fn deny(request: &ElicitationRequest) -> ElicitationResponse {
        ElicitationResponse {
            id: request.id.clone(),
            approved: false,
            value: String::new(),
        }
    }
}

impl ElicitationHandler for ChannelElicitationHandler {
    fn handle(&self, request: &ElicitationRequest) -> ElicitationResponse {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self.sender.send((request.clone(), reply_tx)).is_err() {
            tracing::warn!(
                "Elicitation UI channel closed; auto-denying request '{}'",
                request.id
            );
            return Self::deny(request);
        }
        reply_rx.blocking_recv().unwrap_or_else(|_| {
            tracing::warn!(
                "Elicitation UI dropped reply channel for '{}'; auto-denying",
                request.id
            );
            Self::deny(request)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elicitation::ElicitationInputType;

    fn make_request() -> ElicitationRequest {
        ElicitationRequest {
            id: "req-1".to_string(),
            message: "Pick a color".to_string(),
            input_type: ElicitationInputType::Select,
            choices: vec!["red".to_string(), "blue".to_string()],
            default_value: None,
        }
    }

    #[tokio::test]
    async fn forwards_request_and_returns_reply() {
        let (handler, mut rx) = ChannelElicitationHandler::new();

        // Simulated UI task: drain one envelope, respond.
        let ui = tokio::spawn(async move {
            let (req, reply_tx) = rx.recv().await.expect("envelope");
            assert_eq!(req.id, "req-1");
            reply_tx
                .send(ElicitationResponse {
                    id: req.id,
                    approved: true,
                    value: "blue".to_string(),
                })
                .unwrap();
        });

        let req = make_request();
        let resp = tokio::task::spawn_blocking(move || handler.handle(&req))
            .await
            .expect("join");
        ui.await.expect("ui");

        assert!(resp.approved);
        assert_eq!(resp.value, "blue");
    }

    #[tokio::test]
    async fn denies_when_ui_channel_is_closed() {
        let (handler, rx) = ChannelElicitationHandler::new();
        drop(rx); // UI never listens.

        let req = make_request();
        let resp = tokio::task::spawn_blocking(move || handler.handle(&req))
            .await
            .expect("join");

        assert!(!resp.approved);
        assert!(resp.value.is_empty());
    }

    #[tokio::test]
    async fn denies_when_ui_drops_reply_tx() {
        let (handler, mut rx) = ChannelElicitationHandler::new();

        let ui = tokio::spawn(async move {
            let (_req, reply_tx) = rx.recv().await.expect("envelope");
            drop(reply_tx); // UI accepts but fails to respond.
        });

        let req = make_request();
        let resp = tokio::task::spawn_blocking(move || handler.handle(&req))
            .await
            .expect("join");
        ui.await.expect("ui");

        assert!(!resp.approved);
    }
}
