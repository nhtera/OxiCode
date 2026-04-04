use std::path::Path;

use oxicode_api::LlmProvider;
use oxicode_common::{Message, OxiResult};

use crate::{
    auto_compact::AutoCompactor, context_collapse::ContextCollapse,
    microcompact::microcompact_messages,
    post_compact_cleanup::{self, RestoreContext},
    reactive_compact::ReactiveCompactor,
    snip_compact::{self, SnipConfig},
    token_counter::TokenCounter, truncation::truncate_messages,
};

/// Context budget thresholds (fraction of model max tokens).
const L1_THRESHOLD: f64 = 0.80; // truncate oldest messages
const L2_THRESHOLD: f64 = 0.85; // microcompact tool results / thinking
const L3_THRESHOLD: f64 = 0.90; // LLM auto-compact
const CRITICAL_THRESHOLD: f64 = 0.98; // emergency — cannot proceed safely

/// Result of a budget check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetStatus {
    /// Under L1 threshold — no action needed.
    Ok,
    /// 80–85 % full — apply L1 truncation.
    NeedsL1Truncation,
    /// 85–90 % full — apply L2 microcompact.
    NeedsL2Microcompact,
    /// 90–98 % full — apply L3 LLM auto-compact.
    NeedsL3AutoCompact,
    /// ≥ 98 % full — cannot proceed without drastic action.
    Critical,
}

/// Manages context token budgets and applies defense layers as needed.
#[derive(Debug)]
pub struct BudgetManager {
    /// Maximum tokens the model accepts (input context window).
    pub model_max_tokens: usize,
    pub counter: TokenCounter,
}

impl BudgetManager {
    pub fn new(model_max_tokens: usize) -> Self {
        Self {
            model_max_tokens,
            counter: TokenCounter::new(),
        }
    }

    /// Determine which defense layer is needed based on current usage.
    #[allow(clippy::cast_precision_loss)]
    pub fn check_budget(&mut self, messages: &[Message]) -> BudgetStatus {
        if self.model_max_tokens == 0 {
            return BudgetStatus::Critical;
        }
        let used = self.counter.count_messages(messages);
        let ratio = used as f64 / self.model_max_tokens as f64;

        tracing::debug!(
            used,
            max = self.model_max_tokens,
            ratio = format!("{ratio:.2}"),
            "budget check"
        );

        if ratio >= CRITICAL_THRESHOLD {
            BudgetStatus::Critical
        } else if ratio >= L3_THRESHOLD {
            BudgetStatus::NeedsL3AutoCompact
        } else if ratio >= L2_THRESHOLD {
            BudgetStatus::NeedsL2Microcompact
        } else if ratio >= L1_THRESHOLD {
            BudgetStatus::NeedsL1Truncation
        } else {
            BudgetStatus::Ok
        }
    }

    /// Apply defense layers sequentially until usage is below L1 threshold.
    ///
    /// Layers applied per status:
    /// - Ok              → no-op
    /// - NeedsL1         → L1 truncation
    /// - NeedsL2         → L1 + L2 microcompact
    /// - NeedsL3         → L1 + L2 + L3 LLM compact
    /// - Critical        → L1 + L2 + L3 via L4 ReactiveCompactor; if still critical → L5 collapse
    ///
    /// Returns the (potentially compacted) message list.
    pub async fn apply_defense(
        &mut self,
        messages: &[Message],
        provider: &dyn LlmProvider,
        model: &str,
    ) -> OxiResult<Vec<Message>> {
        self.apply_defense_with_dir(messages, provider, model, Path::new("."))
            .await
    }

    /// Same as `apply_defense` but accepts an explicit `working_dir` for L5
    /// context collapse (exposed for callers that know the project root).
    pub async fn apply_defense_with_dir(
        &mut self,
        messages: &[Message],
        provider: &dyn LlmProvider,
        model: &str,
        working_dir: &Path,
    ) -> OxiResult<Vec<Message>> {
        let status = self.check_budget(messages);

        match status {
            BudgetStatus::Ok => {
                tracing::debug!("budget ok — no defense needed");
                Ok(messages.to_vec())
            }

            BudgetStatus::NeedsL1Truncation => {
                let budget = self.l1_budget();
                let mut result = truncate_messages(messages, budget, &mut self.counter);
                // L1.5: snip old tool results.
                snip_compact::snip_compact(&mut result, &SnipConfig::default());
                Ok(result)
            }

            BudgetStatus::NeedsL2Microcompact => {
                // L1 + L1.5 + L2.
                let budget = self.l1_budget();
                let mut result = truncate_messages(messages, budget, &mut self.counter);
                snip_compact::snip_compact(&mut result, &SnipConfig::default());
                microcompact_messages(&mut result);
                self.counter.clear_cache();
                Ok(result)
            }

            BudgetStatus::NeedsL3AutoCompact => {
                // L1 + L1.5 + L2 + L3 + post-compact restore.
                let budget = self.l1_budget();
                let mut result = truncate_messages(messages, budget, &mut self.counter);
                snip_compact::snip_compact(&mut result, &SnipConfig::default());
                microcompact_messages(&mut result);
                self.counter.clear_cache();

                // Extract context before compaction for post-compact restore.
                let recent_tools =
                    post_compact_cleanup::extract_recent_tools(&result, 5);
                let restore_ctx = RestoreContext {
                    working_dir: Some(working_dir.to_string_lossy().to_string()),
                    recent_tools,
                    ..Default::default()
                };

                match AutoCompactor::compact(&result, provider, model).await {
                    Ok(summary_msg) => {
                        tracing::info!("L3: replaced {} messages with summary", result.len());
                        let mut compacted = vec![summary_msg];
                        post_compact_cleanup::post_compact_restore(
                            &mut compacted,
                            &restore_ctx,
                        );
                        Ok(compacted)
                    }
                    Err(e) => {
                        tracing::warn!("L3: auto-compact failed ({e}), using L1+L2 result");
                        Ok(result)
                    }
                }
            }

            BudgetStatus::Critical => {
                // L4: ReactiveCompactor — runs L1+L2+L3 under urgency.
                tracing::warn!("budget Critical — escalating to L4 reactive compact");
                let mut reactive = ReactiveCompactor::new();
                let (l4_result, notification) =
                    reactive.compact_mid_turn(messages, provider, model).await?;
                tracing::info!(%notification, "L4 complete");

                // Re-check: if still critical after L4, fall back to L5 collapse.
                let post_l4_tokens = self.counter.count_messages(&l4_result);
                if ReactiveCompactor::should_trigger(post_l4_tokens, self.model_max_tokens) {
                    tracing::warn!(
                        post_l4_tokens,
                        model_max = self.model_max_tokens,
                        "still critical after L4 — invoking L5 context collapse"
                    );
                    ContextCollapse::collapse(working_dir, &l4_result)
                } else {
                    Ok(l4_result)
                }
            }
        }
    }

    /// Token budget target for L1 truncation (80 % of max).
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
    fn l1_budget(&self) -> usize {
        (self.model_max_tokens as f64 * L1_THRESHOLD) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msgs(n: usize, chars_each: usize) -> Vec<Message> {
        (0..n)
            .map(|_| Message::user("a".repeat(chars_each)))
            .collect()
    }

    #[test]
    fn status_ok_when_low_usage() {
        let mut mgr = BudgetManager::new(10_000);
        let msgs = make_msgs(2, 40); // ~10 tokens each
        assert_eq!(mgr.check_budget(&msgs), BudgetStatus::Ok);
    }

    #[test]
    fn status_l1_at_80_percent() {
        // 80 tokens used out of 100 max → L1
        let mut mgr = BudgetManager::new(100);
        // Each msg: 40 chars / 4 = 10 tokens + 4 overhead = 14; 6 msgs = 84 tokens → ~84 %
        let msgs = make_msgs(6, 40);
        let status = mgr.check_budget(&msgs);
        assert!(
            matches!(
                status,
                BudgetStatus::NeedsL1Truncation
                    | BudgetStatus::NeedsL2Microcompact
                    | BudgetStatus::NeedsL3AutoCompact
                    | BudgetStatus::Critical
            ),
            "expected some defense needed, got {status:?}"
        );
    }

    #[test]
    fn status_critical_near_limit() {
        let mut mgr = BudgetManager::new(10);
        // way over budget
        let msgs = make_msgs(5, 400);
        assert_eq!(mgr.check_budget(&msgs), BudgetStatus::Critical);
    }

    #[test]
    fn l1_budget_is_80_percent() {
        let mgr = BudgetManager::new(1000);
        assert_eq!(mgr.l1_budget(), 800);
    }
}
