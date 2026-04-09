//! Server request handlers — session management, message dispatch, permission bridge.
//!
//! Each handler receives typed params, interacts with the query engine, and
//! returns a JSON-RPC response. Streaming events are emitted as notifications
//! via the `notify_tx` channel.
//!
//! ## Fixes applied (code review feedback):
//! - **C1:** `active_perms` Arc stored in SessionState so `tool.approve`/`tool.deny`
//!   can resolve permissions while `message.send` is still running.
//! - **C2:** `message.send` dispatched as background task in server.rs (not here).
//! - **M1:** All conversation messages synced back to session (not just final msg).
//! - **M2:** Shutdown cancels all active turns before saving.

use std::collections::HashMap;
use std::sync::Arc;

use oxicode_common::{Message, PermissionResponse};
use oxicode_core::{Conversation, QueryEngine, TurnEvent};
use oxicode_session::Session;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::bridge;
use crate::server_protocol::{
    error_codes, CompactParams, ErrorNotificationParams, MessageCancelParams, MessageSendParams,
    PermissionAskParams, RpcId, RpcNotification, RpcRequest, RpcResponse, SessionCreateParams,
    SessionResumeParams, SessionUpdatedParams, StreamTextParams, ToolDecisionParams,
    ToolResultParams, ToolStartParams,
};

/// Shared map of pending permission reply channels.
type PermMap = Arc<Mutex<HashMap<String, oneshot::Sender<PermissionResponse>>>>;

/// Per-session state managed by the server.
struct SessionState {
    session: Session,
    conversation: Conversation,
    /// Cancel token for the active message.send request (if any).
    cancel_tx: Option<oneshot::Sender<()>>,
    /// Pending permission reply channels keyed by permission_id.
    /// FIX C1: This is the *live* Arc shared with the forwarder task so
    /// `tool.approve`/`tool.deny` can resolve permissions during execution.
    active_perms: PermMap,
}

/// Manages all active sessions and dispatches JSON-RPC requests.
pub struct ServerHandler {
    engine: Arc<QueryEngine>,
    sessions: Mutex<HashMap<String, SessionState>>,
    model: String,
    /// Channel for sending notifications back to the server writer loop.
    notify_tx: mpsc::Sender<RpcNotification>,
}

impl ServerHandler {
    pub fn new(
        engine: Arc<QueryEngine>,
        model: String,
        notify_tx: mpsc::Sender<RpcNotification>,
    ) -> Self {
        Self {
            engine,
            sessions: Mutex::new(HashMap::new()),
            model,
            notify_tx,
        }
    }

    /// Dispatch a request to the appropriate handler.
    pub async fn handle(&self, req: RpcRequest) -> RpcResponse {
        match req.method.as_str() {
            "session.create" => self.handle_session_create(req.id, req.params).await,
            "session.resume" => self.handle_session_resume(req.id, req.params).await,
            "session.list" => self.handle_session_list(req.id).await,
            "message.send" => self.handle_message_send(req.id, req.params).await,
            "message.cancel" => self.handle_message_cancel(req.id, req.params).await,
            "tool.approve" => self.handle_tool_decision(req.id, req.params, true).await,
            "tool.deny" => self.handle_tool_decision(req.id, req.params, false).await,
            "compact" => self.handle_compact(req.id, req.params).await,
            "shutdown" => self.handle_shutdown(req.id).await,
            // Bridge protocol methods (IDE-specific).
            m if m.starts_with("bridge.") => self.handle_bridge_method(req.id, m, req.params).await,
            _ => RpcResponse::err(
                req.id,
                error_codes::METHOD_NOT_FOUND,
                format!("Unknown method: {}", req.method),
            ),
        }
    }

    // -----------------------------------------------------------------------
    // Bridge protocol handlers (IDE integration)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_lines)] // method dispatch with necessary per-method handling
    async fn handle_bridge_method(
        &self,
        id: RpcId,
        method: &str,
        params: serde_json::Value,
    ) -> RpcResponse {
        use bridge::messages::{
            ApprovePermissionParams, CancelTurnParams, GetConversationParams, GetStatusParams,
            GetStatusResult, InitializeParams, InitializeResult, SendMessageParams,
            SwitchModelParams, METHOD_APPROVE_PERMISSION, METHOD_CANCEL_TURN,
            METHOD_GET_CONVERSATION, METHOD_GET_STATUS, METHOD_INITIALIZE, METHOD_SEND_MESSAGE,
            METHOD_SWITCH_MODEL,
        };

        match method {
            METHOD_INITIALIZE => {
                let params: InitializeParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        return RpcResponse::err(
                            id,
                            error_codes::INVALID_PARAMS,
                            format!("Invalid params: {e}"),
                        )
                    }
                };

                tracing::info!(
                    protocol = %params.protocol_version,
                    ide = ?params.ide_name,
                    "bridge.initialize"
                );

                // Create a session for this IDE connection.
                let session = Session::new(&self.model);
                let session_id = session.id.clone();
                let state = SessionState {
                    session,
                    conversation: Conversation::new(),
                    cancel_tx: None,
                    active_perms: Arc::new(Mutex::new(HashMap::new())),
                };
                self.sessions.lock().await.insert(session_id.clone(), state);

                let result = InitializeResult {
                    protocol_version: bridge::PROTOCOL_VERSION.to_string(),
                    session_id,
                    model: self.model.clone(),
                    capabilities: bridge::server_capabilities(),
                };
                RpcResponse::ok(id, serde_json::to_value(result).unwrap_or_default())
            }

            METHOD_GET_STATUS => {
                let params: GetStatusParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        return RpcResponse::err(
                            id,
                            error_codes::INVALID_PARAMS,
                            format!("Invalid params: {e}"),
                        )
                    }
                };

                let sessions = self.sessions.lock().await;
                let Some(state) = sessions.get(&params.session_id) else {
                    return RpcResponse::err(
                        id,
                        error_codes::SESSION_NOT_FOUND,
                        "Session not found",
                    );
                };

                let is_streaming = state.cancel_tx.is_some();
                let perm_pending = !state.active_perms.try_lock().is_ok_and(|p| p.is_empty());

                let status_str = if is_streaming {
                    "streaming"
                } else if perm_pending {
                    "awaiting_permission"
                } else {
                    "idle"
                };

                let result = GetStatusResult {
                    session_id: params.session_id,
                    state: status_str.to_string(),
                    model: self.model.clone(),
                    message_count: state.session.messages.len(),
                };
                RpcResponse::ok(id, serde_json::to_value(result).unwrap_or_default())
            }

            METHOD_GET_CONVERSATION => {
                let params: GetConversationParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        return RpcResponse::err(
                            id,
                            error_codes::INVALID_PARAMS,
                            format!("Invalid params: {e}"),
                        )
                    }
                };

                let sessions = self.sessions.lock().await;
                let Some(state) = sessions.get(&params.session_id) else {
                    return RpcResponse::err(
                        id,
                        error_codes::SESSION_NOT_FOUND,
                        "Session not found",
                    );
                };

                let messages = &state.session.messages;
                let start = params.after_index.unwrap_or(0);
                let slice: Vec<serde_json::Value> = messages
                    .iter()
                    .skip(start)
                    .enumerate()
                    .map(|(i, msg)| {
                        serde_json::json!({
                            "index": start + i,
                            "role": format!("{:?}", msg.role).to_lowercase(),
                            "text": msg.text(),
                        })
                    })
                    .collect();

                RpcResponse::ok(
                    id,
                    serde_json::json!({
                        "session_id": params.session_id,
                        "messages": slice,
                        "total": messages.len(),
                    }),
                )
            }

            METHOD_SEND_MESSAGE => {
                // Delegate to existing message.send handler via params adaptation.
                let params: SendMessageParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        return RpcResponse::err(
                            id,
                            error_codes::INVALID_PARAMS,
                            format!("Invalid params: {e}"),
                        )
                    }
                };
                let adapted = serde_json::to_value(MessageSendParams {
                    session_id: params.session_id,
                    content: params.content,
                })
                .unwrap_or_default();
                self.handle_message_send(id, adapted).await
            }

            METHOD_APPROVE_PERMISSION => {
                let params: ApprovePermissionParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        return RpcResponse::err(
                            id,
                            error_codes::INVALID_PARAMS,
                            format!("Invalid params: {e}"),
                        )
                    }
                };
                let adapted = serde_json::to_value(ToolDecisionParams {
                    session_id: params.session_id,
                    permission_id: params.permission_id,
                    always: params.always,
                })
                .unwrap_or_default();
                self.handle_tool_decision(id, adapted, params.approve).await
            }

            METHOD_CANCEL_TURN => {
                let params: CancelTurnParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        return RpcResponse::err(
                            id,
                            error_codes::INVALID_PARAMS,
                            format!("Invalid params: {e}"),
                        )
                    }
                };
                let adapted = serde_json::to_value(MessageCancelParams {
                    session_id: params.session_id,
                })
                .unwrap_or_default();
                self.handle_message_cancel(id, adapted).await
            }

            METHOD_SWITCH_MODEL => {
                let params: SwitchModelParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        return RpcResponse::err(
                            id,
                            error_codes::INVALID_PARAMS,
                            format!("Invalid params: {e}"),
                        )
                    }
                };

                // Just acknowledge the model switch request.
                // Actual model switching requires engine reconfiguration (future work).
                tracing::info!(
                    session = %params.session_id,
                    model = %params.model,
                    "bridge.switchModel (acknowledged, not yet implemented)"
                );
                RpcResponse::ok(
                    id,
                    serde_json::json!({
                        "session_id": params.session_id,
                        "model": params.model,
                        "switched": false,
                        "reason": "model switching requires engine reconfiguration (planned)",
                    }),
                )
            }

            _ => RpcResponse::err(
                id,
                error_codes::METHOD_NOT_FOUND,
                format!("Unknown bridge method: {method}"),
            ),
        }
    }

    // -----------------------------------------------------------------------
    // Session handlers
    // -----------------------------------------------------------------------

    async fn handle_session_create(&self, id: RpcId, params: serde_json::Value) -> RpcResponse {
        let model = serde_json::from_value::<SessionCreateParams>(params)
            .ok()
            .and_then(|p| p.model)
            .unwrap_or_else(|| self.model.clone());

        let session = Session::new(&model);
        let session_id = session.id.clone();

        let state = SessionState {
            session,
            conversation: Conversation::new(),
            cancel_tx: None,
            active_perms: Arc::new(Mutex::new(HashMap::new())),
        };
        self.sessions.lock().await.insert(session_id.clone(), state);

        RpcResponse::ok(
            id,
            serde_json::json!({"session_id": session_id, "model": model}),
        )
    }

    async fn handle_session_resume(&self, id: RpcId, params: serde_json::Value) -> RpcResponse {
        let params: SessionResumeParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return RpcResponse::err(
                    id,
                    error_codes::INVALID_PARAMS,
                    format!("Invalid params: {e}"),
                )
            }
        };

        // Already loaded?
        {
            let sessions = self.sessions.lock().await;
            if let Some(s) = sessions.get(&params.session_id) {
                return RpcResponse::ok(
                    id,
                    serde_json::json!({
                        "session_id": params.session_id,
                        "message_count": s.session.messages.len(),
                    }),
                );
            }
        }

        // Load from disk.
        match oxicode_session::load_session(&params.session_id, None) {
            Ok(session) => {
                let msg_count = session.messages.len();
                let mut conversation = Conversation::new();
                for msg in &session.messages {
                    conversation.push(msg.clone());
                }
                let state = SessionState {
                    session,
                    conversation,
                    cancel_tx: None,
                    active_perms: Arc::new(Mutex::new(HashMap::new())),
                };
                self.sessions
                    .lock()
                    .await
                    .insert(params.session_id.clone(), state);
                RpcResponse::ok(
                    id,
                    serde_json::json!({"session_id": params.session_id, "message_count": msg_count}),
                )
            }
            Err(e) => RpcResponse::err(
                id,
                error_codes::SESSION_NOT_FOUND,
                format!("Session not found: {e}"),
            ),
        }
    }

    async fn handle_session_list(&self, id: RpcId) -> RpcResponse {
        let sessions = self.sessions.lock().await;
        let list: Vec<serde_json::Value> = sessions
            .values()
            .map(|s| {
                serde_json::json!({
                    "session_id": s.session.id,
                    "message_count": s.session.messages.len(),
                    "model": s.session.model,
                })
            })
            .collect();
        RpcResponse::ok(id, serde_json::json!({"sessions": list}))
    }

    // -----------------------------------------------------------------------
    // Message handling with streaming notifications
    // -----------------------------------------------------------------------

    async fn handle_message_send(&self, id: RpcId, params: serde_json::Value) -> RpcResponse {
        let params: MessageSendParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return RpcResponse::err(
                    id,
                    error_codes::INVALID_PARAMS,
                    format!("Invalid params: {e}"),
                )
            }
        };
        let session_id = params.session_id.clone();

        // Push user message, prepare cancel channel, get the live perms Arc.
        let (cancel_rx, active_perms, msg_count_before) = {
            let mut sessions = self.sessions.lock().await;
            let Some(state) = sessions.get_mut(&session_id) else {
                return RpcResponse::err(id, error_codes::SESSION_NOT_FOUND, "Session not found");
            };
            let user_msg = Message::user(&params.content);
            state.session.push_message(user_msg.clone());
            state.conversation.push(user_msg);
            let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
            state.cancel_tx = Some(cancel_tx);

            // FIX C1: Reset active_perms for this turn and return the Arc.
            *state.active_perms.lock().await = HashMap::new();
            let perms = state.active_perms.clone();
            let count = state.conversation.len();
            (cancel_rx, perms, count)
        };

        // Setup channels for engine → forwarder → IDE notifications.
        let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>(256);
        let (perm_tx, perm_rx) = mpsc::channel::<(String, oneshot::Sender<PermissionResponse>)>(16);

        // FIX C1: Collector writes directly to the session's active_perms Arc.
        let forwarder =
            spawn_event_forwarder(turn_rx, self.notify_tx.clone(), perm_tx, session_id.clone());
        let collector = spawn_perm_collector(perm_rx, active_perms);

        // Clone conversation out for engine execution.
        let mut conversation = {
            let sessions = self.sessions.lock().await;
            sessions[&session_id].conversation.clone()
        };

        // Execute with cancellation support.
        let engine = self.engine.clone();
        let result = tokio::select! {
            res = engine.execute_turn(&mut conversation, Some(&turn_tx)) => res,
            _ = cancel_rx => Err(oxicode_common::OxiError::Other("Request cancelled".into())),
        };

        // Cleanup: close channels, wait for tasks.
        drop(turn_tx);
        let _ = forwarder.await;
        let _ = collector.await;

        // FIX M1: Sync ALL new messages back to session (not just the final one).
        self.sync_session_messages(&session_id, &conversation, msg_count_before)
            .await;
        self.build_message_response(id, &session_id, result).await
    }

    /// FIX M1: Sync all new messages from conversation back into the session.
    /// This includes intermediate tool-result messages, not just the final assistant msg.
    async fn sync_session_messages(
        &self,
        session_id: &str,
        conversation: &Conversation,
        msg_count_before: usize,
    ) {
        let mut sessions = self.sessions.lock().await;
        if let Some(state) = sessions.get_mut(session_id) {
            state.conversation = conversation.clone();
            state.cancel_tx = None;

            // Push all new messages that the engine added during execution.
            let all_msgs = conversation.api_messages();
            if all_msgs.len() > msg_count_before {
                for msg in &all_msgs[msg_count_before..] {
                    state.session.push_message(msg.clone());
                }
            }
            let _ = oxicode_session::save_session(&state.session, None);
        }
    }

    /// Build the final JSON-RPC response after message.send completes.
    async fn build_message_response(
        &self,
        id: RpcId,
        session_id: &str,
        result: oxicode_common::OxiResult<Message>,
    ) -> RpcResponse {
        match result {
            Ok(msg) => {
                let msg_count = {
                    let sessions = self.sessions.lock().await;
                    sessions
                        .get(session_id)
                        .map_or(0, |s| s.session.messages.len())
                };
                let _ = self
                    .notify_tx
                    .send(RpcNotification::new(
                        "session.updated",
                        serde_json::to_value(SessionUpdatedParams {
                            session_id: session_id.to_string(),
                            message_count: msg_count,
                            model: self.model.clone(),
                        })
                        .unwrap_or_default(),
                    ))
                    .await;

                let stop_reason = msg
                    .stop_reason
                    .map_or("end_turn".to_string(), |r| format!("{r:?}").to_lowercase());
                RpcResponse::ok(
                    id,
                    serde_json::json!({
                        "session_id": session_id,
                        "stop_reason": stop_reason,
                        "text": msg.text(),
                    }),
                )
            }
            Err(e) => RpcResponse::err(id, error_codes::INTERNAL_ERROR, e.to_string()),
        }
    }

    async fn handle_message_cancel(&self, id: RpcId, params: serde_json::Value) -> RpcResponse {
        let params: MessageCancelParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return RpcResponse::err(
                    id,
                    error_codes::INVALID_PARAMS,
                    format!("Invalid params: {e}"),
                )
            }
        };
        let mut sessions = self.sessions.lock().await;
        if let Some(state) = sessions.get_mut(&params.session_id) {
            if let Some(cancel_tx) = state.cancel_tx.take() {
                let _ = cancel_tx.send(());
                return RpcResponse::ok(id, serde_json::json!({"cancelled": true}));
            }
            return RpcResponse::ok(
                id,
                serde_json::json!({"cancelled": false, "reason": "no active request"}),
            );
        }
        RpcResponse::err(id, error_codes::SESSION_NOT_FOUND, "Session not found")
    }

    // -----------------------------------------------------------------------
    // Permission bridge — FIX C1: looks up active_perms directly
    // -----------------------------------------------------------------------

    async fn handle_tool_decision(
        &self,
        id: RpcId,
        params: serde_json::Value,
        approve: bool,
    ) -> RpcResponse {
        let params: ToolDecisionParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return RpcResponse::err(
                    id,
                    error_codes::INVALID_PARAMS,
                    format!("Invalid params: {e}"),
                )
            }
        };

        // FIX C1: Look up the permission in the session's active_perms Arc,
        // which is the same Arc that the perm_collector writes to during execution.
        let active_perms = {
            let sessions = self.sessions.lock().await;
            let Some(state) = sessions.get(&params.session_id) else {
                return RpcResponse::err(id, error_codes::SESSION_NOT_FOUND, "Session not found");
            };
            state.active_perms.clone()
        };

        let mut perms = active_perms.lock().await;
        if let Some(reply_tx) = perms.remove(&params.permission_id) {
            let response = match (approve, params.always) {
                (true, true) => PermissionResponse::AlwaysAllow,
                (true, false) => PermissionResponse::AllowOnce,
                (false, true) => PermissionResponse::AlwaysDeny,
                (false, false) => PermissionResponse::Deny,
            };
            let sent = reply_tx.send(response).is_ok();
            RpcResponse::ok(id, serde_json::json!({"acknowledged": sent}))
        } else {
            RpcResponse::err(
                id,
                error_codes::INVALID_PARAMS,
                "Permission request not found or already answered",
            )
        }
    }

    // -----------------------------------------------------------------------
    // Compact + Shutdown
    // -----------------------------------------------------------------------

    async fn handle_compact(&self, id: RpcId, params: serde_json::Value) -> RpcResponse {
        let params: CompactParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return RpcResponse::err(
                    id,
                    error_codes::INVALID_PARAMS,
                    format!("Invalid params: {e}"),
                )
            }
        };
        let mut sessions = self.sessions.lock().await;
        let Some(state) = sessions.get_mut(&params.session_id) else {
            return RpcResponse::err(id, error_codes::SESSION_NOT_FOUND, "Session not found");
        };

        let messages = state.conversation.api_messages().to_vec();
        if messages.len() < 3 {
            return RpcResponse::ok(
                id,
                serde_json::json!({"compacted": false, "reason": "too few messages"}),
            );
        }

        let provider = self.engine.provider_ref().clone();
        let model = self.engine.model();

        match oxicode_context::AutoCompactor::compact(&messages, provider.as_ref(), &model).await {
            Ok(summary_msg) => {
                let before = state.conversation.len();
                state
                    .conversation
                    .replace_messages(vec![summary_msg.clone()]);
                state.session.messages = vec![summary_msg];
                let _ = oxicode_session::save_session(&state.session, None);
                RpcResponse::ok(
                    id,
                    serde_json::json!({"compacted": true, "before": before, "after": 1}),
                )
            }
            Err(e) => RpcResponse::err(
                id,
                error_codes::INTERNAL_ERROR,
                format!("Compact failed: {e}"),
            ),
        }
    }

    /// FIX M2: Shutdown cancels all active turns, then saves all sessions.
    async fn handle_shutdown(&self, id: RpcId) -> RpcResponse {
        let mut sessions = self.sessions.lock().await;
        for state in sessions.values_mut() {
            // Cancel any in-flight turns so engine tasks stop promptly.
            if let Some(cancel_tx) = state.cancel_tx.take() {
                let _ = cancel_tx.send(());
            }
            let _ = oxicode_session::save_session(&state.session, None);
        }
        RpcResponse::ok(id, serde_json::json!({"shutdown": true}))
    }

    /// Check if shutdown was requested (called by server loop after sending response).
    pub fn is_shutdown_response(resp: &RpcResponse) -> bool {
        resp.result
            .as_ref()
            .and_then(|v| v.get("shutdown"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Standalone async helpers (extracted to reduce function length)
// ---------------------------------------------------------------------------

/// Forward TurnEvents from the query engine to JSON-RPC notifications for the IDE.
fn spawn_event_forwarder(
    mut turn_rx: mpsc::Receiver<TurnEvent>,
    notify_tx: mpsc::Sender<RpcNotification>,
    perm_tx: mpsc::Sender<(String, oneshot::Sender<PermissionResponse>)>,
    session_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = turn_rx.recv().await {
            forward_turn_event(&notify_tx, &perm_tx, &session_id, event).await;
        }
    })
}

/// Map a single TurnEvent to the appropriate RPC notification.
#[allow(clippy::too_many_lines)] // event dispatch with per-variant handling
async fn forward_turn_event(
    notify_tx: &mpsc::Sender<RpcNotification>,
    perm_tx: &mpsc::Sender<(String, oneshot::Sender<PermissionResponse>)>,
    session_id: &str,
    event: TurnEvent,
) {
    match event {
        TurnEvent::TextDelta(text) => {
            let _ = notify_tx
                .send(RpcNotification::new(
                    "stream.text",
                    serde_json::to_value(StreamTextParams {
                        session_id: session_id.to_string(),
                        text,
                    })
                    .unwrap_or_default(),
                ))
                .await;
        }
        TurnEvent::TurnStart | TurnEvent::TurnEnd => {
            // No-op for IDE — stream.text covers deltas, response signals completion.
        }
        TurnEvent::ToolUseStart { id, name, input } => {
            let _ = notify_tx
                .send(RpcNotification::new(
                    "tool.start",
                    serde_json::to_value(ToolStartParams {
                        session_id: session_id.to_string(),
                        tool_use_id: id,
                        tool_name: name,
                        input,
                    })
                    .unwrap_or_default(),
                ))
                .await;
        }
        TurnEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let _ = notify_tx
                .send(RpcNotification::new(
                    "tool.result",
                    serde_json::to_value(ToolResultParams {
                        session_id: session_id.to_string(),
                        tool_use_id,
                        content,
                        is_error,
                    })
                    .unwrap_or_default(),
                ))
                .await;
        }
        TurnEvent::PermissionAsk {
            tool_name,
            input_summary,
            prompt,
            reply_tx,
        } => {
            let perm_id = uuid::Uuid::new_v4().to_string();
            let _ = notify_tx
                .send(RpcNotification::new(
                    "permission.ask",
                    serde_json::to_value(PermissionAskParams {
                        session_id: session_id.to_string(),
                        permission_id: perm_id.clone(),
                        tool_name,
                        input_summary,
                        prompt,
                    })
                    .unwrap_or_default(),
                ))
                .await;
            // Send to collector which writes directly to session's active_perms.
            let _ = perm_tx.send((perm_id, reply_tx)).await;
        }
        TurnEvent::Error(msg) => {
            let _ = notify_tx
                .send(RpcNotification::new(
                    "error",
                    serde_json::to_value(ErrorNotificationParams {
                        session_id: Some(session_id.to_string()),
                        message: msg,
                    })
                    .unwrap_or_default(),
                ))
                .await;
        }
        TurnEvent::RateLimited {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        } => {
            tracing::warn!(
                "Rate limited ({attempt}/{max_retries}): {message} — retry in {retry_in_secs:.0}s"
            );
            let _ = notify_tx
                .send(RpcNotification::new(
                    "stream.rate_limited",
                    serde_json::json!({
                        "session_id": session_id,
                        "message": message,
                        "attempt": attempt,
                        "max_retries": max_retries,
                        "retry_in_secs": retry_in_secs,
                    }),
                ))
                .await;
        }
        TurnEvent::ThinkingDelta(_) => {
            // Thinking deltas not forwarded to IDE clients for now.
        }
    }
}

/// Collect permission reply channels from the forwarder into the session's shared map.
fn spawn_perm_collector(
    mut perm_rx: mpsc::Receiver<(String, oneshot::Sender<PermissionResponse>)>,
    active_perms: PermMap,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some((perm_id, reply_tx)) = perm_rx.recv().await {
            active_perms.lock().await.insert(perm_id, reply_tx);
        }
    })
}
