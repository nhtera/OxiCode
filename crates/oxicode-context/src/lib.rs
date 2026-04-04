/// Token counting heuristic (chars/4, no external tokenizer).
pub mod token_counter;

/// Layer-1: remove oldest middle messages until under token budget.
pub mod truncation;

/// Layer-2: in-place compression of tool results and thinking blocks.
pub mod microcompact;

/// Layer-3: LLM-assisted conversation summarization.
pub mod auto_compact;

/// Layer-1.5: selective tool result removal (between truncation and microcompact).
pub mod snip_compact;

/// Session memory compaction: persistent summaries with boundary tracking.
pub mod session_memory_compact;

/// Post-compact cleanup: restore critical context after compaction.
pub mod post_compact_cleanup;

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
pub use post_compact_cleanup::RestoreContext;
pub use reactive_compact::ReactiveCompactor;
pub use session_memory_compact::SessionMemory;
pub use snip_compact::{SnipConfig, SnipResult};
pub use token_counter::TokenCounter;
pub use truncation::truncate_messages;
