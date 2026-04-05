//! VCR player — replays previously recorded cassette entries in order.
//!
//! Useful for offline testing, demos, and validating integration without
//! making real LLM API calls.

use crate::vcr_recorder::VcrEntry;

/// Plays back a sequence of [`VcrEntry`] records in order.
///
/// Advance through the cassette with [`next`](VcrPlayer::next), reset to the
/// beginning with [`reset`](VcrPlayer::reset).
#[derive(Debug)]
pub struct VcrPlayer {
    /// All entries loaded into this player.
    entries: Vec<VcrEntry>,
    /// Index of the next entry to be returned by [`next`](VcrPlayer::next).
    position: usize,
}

impl VcrPlayer {
    /// Create a new player pre-loaded with `entries`.
    pub fn new(entries: Vec<VcrEntry>) -> Self {
        Self {
            entries,
            position: 0,
        }
    }

    /// Return the next entry and advance the position, or `None` if exhausted.
    pub fn next(&mut self) -> Option<&VcrEntry> {
        let entry = self.entries.get(self.position)?;
        self.position += 1;
        Some(entry)
    }

    /// Whether there are remaining entries to replay.
    pub fn has_more(&self) -> bool {
        self.position < self.entries.len()
    }

    /// Current playback position (0-based index of the *next* entry to be read).
    pub fn position(&self) -> usize {
        self.position
    }

    /// Total number of entries in this cassette.
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// Reset the playback position to the beginning of the cassette.
    pub fn reset(&mut self) {
        self.position = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries(n: usize) -> Vec<VcrEntry> {
        (0..n)
            .map(|i| VcrEntry {
                request_summary: format!("req-{i}"),
                response_summary: format!("resp-{i}"),
                timestamp: "2026-04-05T00:00:00Z".to_string(),
                duration_ms: i as u64 * 10,
            })
            .collect()
    }

    #[test]
    fn plays_entries_in_order() {
        let mut player = VcrPlayer::new(sample_entries(3));
        assert_eq!(player.next().unwrap().request_summary, "req-0");
        assert_eq!(player.next().unwrap().request_summary, "req-1");
        assert_eq!(player.next().unwrap().request_summary, "req-2");
        assert!(player.next().is_none());
    }

    #[test]
    fn has_more_and_total() {
        let mut player = VcrPlayer::new(sample_entries(2));
        assert_eq!(player.total(), 2);
        assert!(player.has_more());
        player.next();
        assert!(player.has_more());
        player.next();
        assert!(!player.has_more());
    }

    #[test]
    fn reset_restarts_playback() {
        let mut player = VcrPlayer::new(sample_entries(2));
        player.next();
        player.next();
        assert_eq!(player.position(), 2);
        player.reset();
        assert_eq!(player.position(), 0);
        assert!(player.has_more());
    }
}
