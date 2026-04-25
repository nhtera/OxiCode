pub mod auto_dream_config;
pub mod auto_dream_service;
pub mod conversation;
pub mod diagnostic_tracker;
pub mod perf_metrics;
pub mod query_engine;
pub mod rewind;
pub mod system_prompt;
pub mod thinking_block_store;
mod tool_dispatch;
pub mod tool_use_summary;
pub mod turn_event;
pub mod vcr_player;
pub mod vcr_recorder;
pub mod vcr_storage;

pub use auto_dream_config::AutoDreamConfig;
pub use auto_dream_service::{AutoDreamService, Suggestion};
pub use conversation::Conversation;
pub use diagnostic_tracker::DiagnosticTracker;
pub use perf_metrics::{PerfMetrics, PerfSummary};
pub use query_engine::{QueryEngine, CONTINUATION_NUDGE, MAX_OUTPUT_TOKENS_RECOVERY};
pub use thinking_block_store::ThinkingBlockStore;
pub use tool_use_summary::ToolUseSummary;
pub use turn_event::{HookMessageKind, HookState, TurnEvent};
pub use vcr_player::VcrPlayer;
pub use vcr_recorder::{VcrEntry, VcrRecorder};
pub use vcr_storage::{
    delete as vcr_delete, list as vcr_list, load as vcr_load, save as vcr_save, vcr_dir,
};
