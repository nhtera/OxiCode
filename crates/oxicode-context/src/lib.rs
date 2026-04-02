/// Token counting heuristic (chars/4, no external tokenizer).
pub mod token_counter;

/// Layer-1: remove oldest middle messages until under token budget.
pub mod truncation;

/// Layer-2: in-place compression of tool results and thinking blocks.
pub mod microcompact;

/// Layer-3: LLM-assisted conversation summarization.
pub mod auto_compact;

/// Budget manager: tracks thresholds and orchestrates L1-L5 defenses.
pub mod budget;

/// Layer-4: mid-turn emergency reactive compaction triggered during streaming.
pub mod reactive_compact;

/// Layer-5: last-resort context collapse from working directory state.
pub mod context_collapse;

// Convenient re-exports for callers.
pub use auto_compact::AutoCompactor;
pub use budget::{BudgetManager, BudgetStatus};
pub use context_collapse::ContextCollapse;
pub use microcompact::microcompact_messages;
pub use reactive_compact::ReactiveCompactor;
pub use token_counter::TokenCounter;
pub use truncation::truncate_messages;
