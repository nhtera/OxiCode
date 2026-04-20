/// State mutation helpers: status label, interrupt, queued messages, ctrl+c.
use std::time::{Duration, Instant};

use crate::events::UiEvent;
use crate::widgets::Notification;

impl super::App {
    /// Compose the status-bar label shown to the right.
    /// Priority: rate-limit/retry banner > active hook indicator.
    pub(super) fn compose_status_label(&self) -> String {
        if !self.retry_status_label.is_empty() {
            return self.retry_status_label.clone();
        }
        if let Some(hook) = &self.active_hook {
            return format!("⟳ Running {hook} hook…");
        }
        String::new()
    }

    /// Signal an interrupt: set cancel flag directly (engine sees it
    /// immediately even while blocked on streaming) and send InterruptTurn.
    /// Immediately resets is_turn_active so the TUI accepts input without
    /// waiting for the engine's Error/MessageComplete round-trip.
    pub(super) async fn signal_interrupt(&mut self) {
        use std::sync::atomic::Ordering;
        if let Some(flag) = &self.cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        let _ = self.ui_tx.send(UiEvent::InterruptTurn).await;
        self.is_turn_active = false;
        self.turn_started_at = None;
        self.stall_start = None;
        self.message_queue.clear();
    }

    /// If the turn just ended and the message queue has pending input, send the next one.
    pub(super) async fn drain_next_queued_message(&mut self) {
        if self.is_turn_active || self.message_queue.is_empty() {
            return;
        }
        if let Some(queued) = self.message_queue.dequeue() {
            self.is_turn_active = true;
            self.turn_started_at = Some(Instant::now());
            let _ = self
                .ui_tx
                .send(UiEvent::UserInput {
                    text: queued.text,
                    images: queued.images,
                })
                .await;
        }
    }

    /// Handle Ctrl+C: requires double-press to quit (both idle and active).
    /// First press shows hint / interrupts; second press within 2s quits.
    pub(super) async fn handle_ctrl_c(&mut self) {
        if self
            .last_interrupt
            .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
        {
            // Second Ctrl+C within 2s → quit.
            let _ = self.ui_tx.send(UiEvent::Quit).await;
            self.should_quit = true;
            self.ctrl_c_hint_visible = false;
        } else {
            // First Ctrl+C.
            self.last_interrupt = Some(Instant::now());
            if self.is_turn_active {
                self.signal_interrupt().await;
                self.notifications.push(Notification::new(
                    "Interrupting... (Ctrl+C again to quit)".to_string(),
                    crate::widgets::notification::NotificationLevel::Info,
                ));
            }
            self.ctrl_c_hint_visible = true;
        }
    }
}
