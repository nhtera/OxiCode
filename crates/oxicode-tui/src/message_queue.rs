//! Pending message queue — stores user messages typed during active streaming turns.
//!
//! Messages are enqueued when `is_turn_active == true` and auto-drained FIFO
//! after each turn completes (`CoreEvent::MessageComplete`).

use crate::image_paste::PastedImage;

/// A message queued while a turn is active.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub text: String,
    pub images: Vec<PastedImage>,
}

impl QueuedMessage {
    pub fn new(text: String, images: Vec<PastedImage>) -> Self {
        Self { text, images }
    }
}

/// FIFO queue for pending user messages.
#[derive(Debug, Default)]
pub struct MessageQueue {
    items: Vec<QueuedMessage>,
}

impl MessageQueue {
    /// Push a message to the back of the queue.
    pub fn enqueue(&mut self, msg: QueuedMessage) {
        self.items.push(msg);
    }

    /// Pop from front (FIFO order).
    pub fn dequeue(&mut self) -> Option<QueuedMessage> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.items.remove(0))
        }
    }

    /// Peek at the first queued message without removing it.
    pub fn peek(&self) -> Option<&QueuedMessage> {
        self.items.first()
    }

    /// Peek at a specific index without removing.
    pub fn peek_at(&self, index: usize) -> Option<&QueuedMessage> {
        self.items.get(index)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Pop all messages, joining text with newlines. Returns combined text
    /// and all images from every queued message. Used by Arrow Up to edit.
    pub fn pop_all_editable(&mut self) -> (String, Vec<PastedImage>) {
        let mut texts = Vec::with_capacity(self.items.len());
        let mut images = Vec::new();
        for msg in self.items.drain(..) {
            texts.push(msg.text);
            images.extend(msg.images);
        }
        (texts.join("\n"), images)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enqueue_dequeue_fifo() {
        let mut q = MessageQueue::default();
        q.enqueue(QueuedMessage::new("first".into(), vec![]));
        q.enqueue(QueuedMessage::new("second".into(), vec![]));
        assert_eq!(q.len(), 2);
        assert_eq!(q.dequeue().unwrap().text, "first");
        assert_eq!(q.dequeue().unwrap().text, "second");
        assert!(q.is_empty());
    }

    #[test]
    fn test_pop_all_editable_joins_text() {
        let mut q = MessageQueue::default();
        q.enqueue(QueuedMessage::new("line1".into(), vec![]));
        q.enqueue(QueuedMessage::new("line2".into(), vec![]));
        let (text, _images) = q.pop_all_editable();
        assert_eq!(text, "line1\nline2");
        assert!(q.is_empty());
    }

    #[test]
    fn test_clear() {
        let mut q = MessageQueue::default();
        q.enqueue(QueuedMessage::new("msg".into(), vec![]));
        q.clear();
        assert!(q.is_empty());
    }
}
