use oxicode_common::{Message, Usage};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

/// Centralized application state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub is_streaming: bool,
    pub current_model: String,
    pub total_usage: Usage,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            messages: Vec::new(),
            is_streaming: false,
            current_model: oxicode_common::constants::DEFAULT_MODEL.to_string(),
            total_usage: Usage::default(),
        }
    }
}

/// State store with watch channel for broadcasting updates to subscribers.
pub struct StateStore {
    tx: watch::Sender<AppState>,
    rx: watch::Receiver<AppState>,
}

impl StateStore {
    pub fn new(initial: AppState) -> Self {
        let (tx, rx) = watch::channel(initial);
        Self { tx, rx }
    }

    /// Get a subscriber that receives state updates.
    pub fn subscribe(&self) -> watch::Receiver<AppState> {
        self.rx.clone()
    }

    /// Get current state snapshot.
    pub fn current(&self) -> AppState {
        self.rx.borrow().clone()
    }

    /// Update state via a closure. Broadcasts to all subscribers.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AppState),
    {
        self.tx.send_modify(f);
    }

    /// Add a message to the state.
    pub fn push_message(&self, message: Message) {
        self.update(|state| {
            state.messages.push(message);
        });
    }

    /// Set streaming flag.
    pub fn set_streaming(&self, streaming: bool) {
        self.update(|state| {
            state.is_streaming = streaming;
        });
    }

    /// Accumulate usage from a response.
    pub fn add_usage(&self, usage: &Usage) {
        self.update(|state| {
            state.total_usage.input_tokens += usage.input_tokens;
            state.total_usage.output_tokens += usage.output_tokens;
        });
    }
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new(AppState::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_store_update() {
        let store = StateStore::default();
        assert!(store.current().messages.is_empty());

        store.push_message(Message::user("hello"));
        assert_eq!(store.current().messages.len(), 1);
    }

    #[test]
    fn test_state_store_subscribe() {
        let store = StateStore::default();
        let rx = store.subscribe();

        store.set_streaming(true);
        assert!(rx.borrow().is_streaming);

        store.set_streaming(false);
        assert!(!rx.borrow().is_streaming);
    }

    #[test]
    fn test_add_usage() {
        let store = StateStore::default();
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        store.add_usage(&usage);
        assert_eq!(store.current().total_usage.input_tokens, 100);
        assert_eq!(store.current().total_usage.output_tokens, 50);
    }
}
