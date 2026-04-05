//! Storage for extended-thinking blocks produced during a conversation.
//!
//! Keeps a bounded, FIFO queue of `(turn_index, content)` pairs so that
//! thinking output can be retrieved by turn number or as a whole collection.

use std::collections::VecDeque;

/// Bounded store for thinking-block strings produced by the model.
///
/// Older entries are evicted automatically once `max_blocks` is reached,
/// keeping memory usage predictable across long sessions.
#[derive(Debug, Clone)]
pub struct ThinkingBlockStore {
    /// Stored `(turn_index, content)` pairs in insertion order.
    blocks: VecDeque<(usize, String)>,
    /// Maximum number of blocks held before the oldest is dropped.
    max_blocks: usize,
}

impl ThinkingBlockStore {
    /// Create a new store with the given capacity.
    ///
    /// Use `max_blocks = 50` as a sensible default.
    pub fn new(max_blocks: usize) -> Self {
        Self {
            blocks: VecDeque::with_capacity(max_blocks.min(128)),
            max_blocks,
        }
    }

    /// Record a thinking block for `turn`, evicting the oldest entry if full.
    pub fn record(&mut self, turn: usize, content: String) {
        if self.blocks.len() >= self.max_blocks {
            self.blocks.pop_front();
        }
        self.blocks.push_back((turn, content));
    }

    /// Return the most recently recorded block, if any.
    pub fn get_last(&self) -> Option<&(usize, String)> {
        self.blocks.back()
    }

    /// Return a reference to the full deque in insertion order.
    pub fn get_all(&self) -> &VecDeque<(usize, String)> {
        &self.blocks
    }

    /// Return the content for the first block matching `turn`, if any.
    pub fn get_by_turn(&self, turn: usize) -> Option<&str> {
        self.blocks
            .iter()
            .find(|(t, _)| *t == turn)
            .map(|(_, content)| content.as_str())
    }

    /// Current number of stored blocks.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Whether the store currently holds no blocks.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

impl Default for ThinkingBlockStore {
    fn default() -> Self {
        Self::new(50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_get_last() {
        let mut store = ThinkingBlockStore::new(10);
        store.record(0, "first thought".to_string());
        store.record(1, "second thought".to_string());
        let last = store.get_last().unwrap();
        assert_eq!(last.0, 1);
        assert_eq!(last.1, "second thought");
    }

    #[test]
    fn get_by_turn_returns_correct_content() {
        let mut store = ThinkingBlockStore::new(10);
        store.record(3, "turn three".to_string());
        store.record(7, "turn seven".to_string());
        assert_eq!(store.get_by_turn(3), Some("turn three"));
        assert_eq!(store.get_by_turn(7), Some("turn seven"));
        assert_eq!(store.get_by_turn(99), None);
    }

    #[test]
    fn evicts_oldest_when_full() {
        let mut store = ThinkingBlockStore::new(3);
        store.record(0, "a".to_string());
        store.record(1, "b".to_string());
        store.record(2, "c".to_string());
        // This insert should evict turn 0.
        store.record(3, "d".to_string());
        assert_eq!(store.len(), 3);
        assert_eq!(store.get_by_turn(0), None); // evicted
        assert_eq!(store.get_by_turn(3), Some("d"));
    }
}
