//! Paste burst detection fallback for terminals without bracketed paste.
//!
//! When bracketed paste is not available, rapid character input (>5 chars
//! within 50ms) is treated as a paste event. The detector buffers characters
//! and flushes them in bulk once the burst ends.

use std::time::{Duration, Instant};

/// Threshold: this many chars within the window triggers paste mode.
const BURST_THRESHOLD: usize = 5;
/// Time window for burst detection.
const BURST_WINDOW: Duration = Duration::from_millis(50);

/// Detects rapid character input as paste events.
pub struct PasteDetector {
    /// Characters buffered during a potential paste burst.
    buffer: String,
    /// Timestamp of the first character in the current burst.
    burst_start: Option<Instant>,
    /// Whether we're currently in a detected paste burst.
    in_burst: bool,
}

impl PasteDetector {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            burst_start: None,
            in_burst: false,
        }
    }

    /// Feed a character into the detector.
    ///
    /// Returns `None` if the character is being buffered (potential paste),
    /// or `Some(text)` if the burst has ended and buffered text should be
    /// inserted at once.
    pub fn feed(&mut self, ch: char) -> Option<String> {
        let now = Instant::now();

        match self.burst_start {
            None => {
                // Start tracking a potential burst.
                self.burst_start = Some(now);
                self.buffer.push(ch);
                None
            }
            Some(start) => {
                if now.duration_since(start) < BURST_WINDOW {
                    // Still within burst window.
                    self.buffer.push(ch);
                    if self.buffer.len() >= BURST_THRESHOLD {
                        self.in_burst = true;
                    }
                    None
                } else {
                    // Burst window expired — flush buffered text and start new.
                    let flushed = self.flush();
                    self.burst_start = Some(now);
                    self.buffer.push(ch);
                    Some(flushed)
                }
            }
        }
    }

    /// Force-flush any buffered characters (called on non-char events or timeout).
    pub fn flush(&mut self) -> String {
        let text = std::mem::take(&mut self.buffer);
        self.burst_start = None;
        self.in_burst = false;
        text
    }

    /// Whether there are buffered characters waiting.
    pub fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// Whether the detector is currently in a paste burst.
    pub fn is_paste(&self) -> bool {
        self.in_burst
    }
}

impl Default for PasteDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn single_char_buffers() {
        let mut d = PasteDetector::new();
        let result = d.feed('a');
        assert!(result.is_none(), "Single char should buffer");
        assert!(d.has_pending());
    }

    #[test]
    fn rapid_chars_detected_as_paste() {
        let mut d = PasteDetector::new();
        for ch in "hello world".chars() {
            d.feed(ch);
        }
        // Should have buffered all chars and detected paste.
        assert!(d.is_paste(), "Rapid chars should trigger paste detection");
        let flushed = d.flush();
        assert_eq!(flushed, "hello world");
    }

    #[test]
    fn slow_typing_not_detected_as_paste() {
        let mut d = PasteDetector::new();
        d.feed('a');
        // Wait longer than the burst window.
        sleep(Duration::from_millis(60));
        let result = d.feed('b');
        // First char should be flushed.
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "a");
        assert!(!d.is_paste());
    }

    #[test]
    fn flush_returns_all_buffered() {
        let mut d = PasteDetector::new();
        d.feed('x');
        d.feed('y');
        d.feed('z');
        let flushed = d.flush();
        assert_eq!(flushed, "xyz");
        assert!(!d.has_pending());
    }
}
