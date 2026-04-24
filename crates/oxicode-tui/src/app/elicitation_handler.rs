//! Elicitation overlay key dispatch + channel intake.
//!
//! Mirrors the permission handler pattern: an in-flight `PendingElicitation`
//! owns the dialog state + the oneshot `reply_tx` that unblocks the MCP
//! handler's `blocking_recv()` call.

use crossterm::event::KeyEvent;
use oxicode_mcp::ElicitationEnvelope;

use crate::widgets::{ElicitationDialog, ElicitationOutcome};

use super::{App, PendingElicitation};

impl App {
    /// Promote an envelope from MCP into a pending dialog.
    ///
    /// If another elicitation is already open we decline the new one to avoid
    /// stacking overlays — MCP servers that queue elicitations should serialize
    /// them, matching Claude Code behavior.
    pub(super) fn accept_elicitation_envelope(&mut self, envelope: ElicitationEnvelope) {
        let (request, reply_tx) = envelope;
        if self.pending_elicitation.is_some() {
            let denial = oxicode_mcp::ElicitationResponse {
                id: request.id.clone(),
                approved: false,
                value: String::new(),
            };
            let _ = reply_tx.send(denial);
            tracing::warn!(
                "Refused overlapping elicitation request '{}' (another is active)",
                request.id
            );
            return;
        }
        self.pending_elicitation = Some(PendingElicitation {
            dialog: ElicitationDialog::new(request),
            reply_tx,
        });
    }

    /// Handle key events routed to the elicitation overlay.
    pub(super) fn handle_elicitation_key(&mut self, key: KeyEvent) {
        let Some(ref mut pending) = self.pending_elicitation else {
            return;
        };
        match pending.dialog.handle_key(key) {
            ElicitationOutcome::Continue => {}
            ElicitationOutcome::Complete(response) => {
                if let Some(pending) = self.pending_elicitation.take() {
                    let _ = pending.reply_tx.send(response);
                }
            }
        }
    }
}
