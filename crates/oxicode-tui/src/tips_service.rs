//! Tips service — displays helpful tips during idle moments.
//!
//! Tips are shown once each, dismissed by the user, and not repeated.
//! Covers keyboard shortcuts, commands, and workflow suggestions.

use std::collections::HashSet;

/// A helpful tip to display to the user.
#[derive(Debug, Clone)]
pub struct Tip {
    /// Unique identifier for deduplication.
    pub id: &'static str,
    /// Short tip text.
    pub text: &'static str,
    /// Category for grouping.
    pub category: TipCategory,
}

/// Categories of tips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipCategory {
    Shortcut,
    Command,
    Workflow,
    Feature,
}

/// All available tips (static).
const ALL_TIPS: &[Tip] = &[
    Tip {
        id: "esc_cancel",
        text: "Press Esc to cancel the current streaming response.",
        category: TipCategory::Shortcut,
    },
    Tip {
        id: "slash_help",
        text: "Type /help to see all available slash commands.",
        category: TipCategory::Command,
    },
    Tip {
        id: "ctrl_l_clear",
        text: "Use Ctrl+L to clear the screen while keeping history.",
        category: TipCategory::Shortcut,
    },
    Tip {
        id: "compact",
        text: "Use /compact to reduce context size when conversations get long.",
        category: TipCategory::Command,
    },
    Tip {
        id: "session_resume",
        text: "Resume a previous session with: oxicode --session <id>",
        category: TipCategory::Workflow,
    },
    Tip {
        id: "vim_mode",
        text: "Enable vim-style editing with /vim or set vim_mode = true in settings.",
        category: TipCategory::Feature,
    },
    Tip {
        id: "export",
        text: "Export your conversation with /export [filename].",
        category: TipCategory::Command,
    },
    Tip {
        id: "memory_add",
        text: "Save facts with /memory add <text> — they persist across sessions.",
        category: TipCategory::Feature,
    },
    Tip {
        id: "model_switch",
        text: "Switch models mid-session with /model <name> (e.g., /model claude-opus-4).",
        category: TipCategory::Command,
    },
    Tip {
        id: "undo",
        text: "Use /undo to remove the last user+assistant message pair.",
        category: TipCategory::Command,
    },
    Tip {
        id: "shortcuts",
        text: "Press ? or /shortcuts to see all keyboard shortcuts.",
        category: TipCategory::Shortcut,
    },
    Tip {
        id: "search",
        text: "Press / in vim mode to search through message history.",
        category: TipCategory::Shortcut,
    },
    Tip {
        id: "multi_provider",
        text: "OxiCode supports multiple providers: Anthropic, OpenAI, Bedrock, Vertex, Ollama.",
        category: TipCategory::Feature,
    },
    Tip {
        id: "thinking",
        text: "Enable extended thinking with /thinking on for complex reasoning tasks.",
        category: TipCategory::Feature,
    },
    Tip {
        id: "doctor",
        text: "Run /doctor to check system health and dependencies.",
        category: TipCategory::Command,
    },
];

/// Service that tracks shown tips and provides the next unshown tip.
pub struct TipsService {
    /// Set of tip IDs already shown in this session.
    shown: HashSet<&'static str>,
    /// Index for round-robin iteration.
    next_index: usize,
}

impl TipsService {
    pub fn new() -> Self {
        Self {
            shown: HashSet::new(),
            next_index: 0,
        }
    }

    /// Get the next unshown tip, if any remain.
    pub fn next_tip(&mut self) -> Option<&'static Tip> {
        let total = ALL_TIPS.len();
        if self.shown.len() >= total {
            return None; // All tips shown.
        }

        for _ in 0..total {
            let tip = &ALL_TIPS[self.next_index % total];
            self.next_index = (self.next_index + 1) % total;

            if !self.shown.contains(tip.id) {
                return Some(tip);
            }
        }
        None
    }

    /// Mark a tip as shown (dismissed).
    pub fn dismiss(&mut self, tip_id: &'static str) {
        self.shown.insert(tip_id);
    }

    /// How many tips remain unshown.
    pub fn remaining(&self) -> usize {
        ALL_TIPS.len() - self.shown.len()
    }

    /// Total number of available tips.
    pub fn total() -> usize {
        ALL_TIPS.len()
    }

    /// Reset all tips to unshown state.
    pub fn reset(&mut self) {
        self.shown.clear();
        self.next_index = 0;
    }
}

impl Default for TipsService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_tip_returns_tip() {
        let mut svc = TipsService::new();
        let tip = svc.next_tip();
        assert!(tip.is_some());
    }

    #[test]
    fn test_dismiss_prevents_repeat() {
        let mut svc = TipsService::new();
        let tip = svc.next_tip().unwrap();
        let id = tip.id;
        svc.dismiss(id);

        // Next tip should be different.
        let tip2 = svc.next_tip().unwrap();
        assert_ne!(tip2.id, id);
    }

    #[test]
    fn test_all_tips_exhaust() {
        let mut svc = TipsService::new();
        let total = TipsService::total();

        for _ in 0..total {
            let tip = svc.next_tip().unwrap();
            svc.dismiss(tip.id);
        }

        assert!(svc.next_tip().is_none());
        assert_eq!(svc.remaining(), 0);
    }

    #[test]
    fn test_reset() {
        let mut svc = TipsService::new();
        let tip = svc.next_tip().unwrap();
        svc.dismiss(tip.id);
        assert_eq!(svc.remaining(), TipsService::total() - 1);

        svc.reset();
        assert_eq!(svc.remaining(), TipsService::total());
    }

    #[test]
    fn test_tip_categories() {
        assert!(ALL_TIPS.iter().any(|t| t.category == TipCategory::Shortcut));
        assert!(ALL_TIPS.iter().any(|t| t.category == TipCategory::Command));
        assert!(ALL_TIPS.iter().any(|t| t.category == TipCategory::Feature));
    }
}
