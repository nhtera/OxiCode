use oxicode_api::LlmProvider;
use oxicode_common::{Message, OxiResult};

use crate::{
    auto_compact::AutoCompactor,
    microcompact::microcompact_messages,
    token_counter::TokenCounter,
    truncation::truncate_messages,
};

/// Fraction of model max tokens that triggers a mid-turn reactive compaction.
const REACTIVE_THRESHOLD: f64 = 0.95;

/// Layer-4 defense: emergency compaction triggered mid-turn during streaming.
///
/// Orchestrates L1 → L2 → L3 in sequence under urgency, then reports the result.
#[derive(Debug, Clone, Default)]
pub struct ReactiveCompactor {
    counter: TokenCounter,
}

impl ReactiveCompactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when `current_tokens` exceeds 95 % of `model_max`.
    #[allow(clippy::cast_precision_loss)]
    pub fn should_trigger(current_tokens: usize, model_max: usize) -> bool {
        if model_max == 0 {
            return false;
        }
        current_tokens as f64 / model_max as f64 > REACTIVE_THRESHOLD
    }

    /// Apply L1 + L2 + L3 in sequence and return the compacted messages plus a
    /// human-readable notification string.
    ///
    /// Returns `(compacted_messages, notification_text)`.
    pub async fn compact_mid_turn(
        &mut self,
        messages: &[Message],
        provider: &dyn LlmProvider,
        model: &str,
    ) -> OxiResult<(Vec<Message>, String)> {
        let old_count = messages.len();
        tracing::warn!(
            old_count,
            "L4: reactive compact triggered mid-turn"
        );

        // L1: truncate oldest middle messages.
        // Use a tight budget (80 % of a large window); reactive compact is
        // heuristic — we just want a meaningful reduction, not an exact cut.
        let budget = estimate_l1_budget(messages, &mut self.counter);
        let mut result = truncate_messages(messages, budget, &mut self.counter);

        // L2: microcompact in-place.
        microcompact_messages(&mut result);
        self.counter.clear_cache();

        // L3: LLM summarise; fall through on failure.
        match AutoCompactor::compact(&result, provider, model).await {
            Ok(summary_msg) => {
                tracing::info!("L4→L3: replaced {} messages with summary", result.len());
                result = vec![summary_msg];
            }
            Err(e) => {
                tracing::warn!("L4→L3: auto-compact failed ({e}), keeping L1+L2 result");
            }
        }

        let new_count = result.len();
        let notification = format!(
            "Context compacted mid-turn: {old_count} messages → {new_count}"
        );

        tracing::info!(%notification, "L4: reactive compact complete");
        Ok((result, notification))
    }

    /// Text injected into the conversation when a mid-turn compaction occurs.
    pub fn format_interrupt_notice() -> String {
        "[Context was compacted to stay within limits. \
         Previous conversation has been summarized.]"
            .to_string()
    }
}

/// Heuristic L1 budget: 80 % of the current total token count.
///
/// We do not have model_max here; using the current size as a proxy gives a
/// meaningful reduction target without needing caller-supplied limits.
#[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn estimate_l1_budget(messages: &[Message], counter: &mut TokenCounter) -> usize {
    let total = counter.count_messages(messages);
    (total as f64 * 0.80) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_trigger_above_95_percent() {
        assert!(ReactiveCompactor::should_trigger(9600, 10_000));
        assert!(ReactiveCompactor::should_trigger(10_000, 10_000));
    }

    #[test]
    fn should_not_trigger_below_threshold() {
        assert!(!ReactiveCompactor::should_trigger(9000, 10_000)); // 90 %
        assert!(!ReactiveCompactor::should_trigger(0, 10_000));
    }

    #[test]
    fn should_not_trigger_with_zero_max() {
        assert!(!ReactiveCompactor::should_trigger(1000, 0));
    }

    #[test]
    fn interrupt_notice_non_empty() {
        let notice = ReactiveCompactor::format_interrupt_notice();
        assert!(!notice.is_empty());
        assert!(notice.contains("compacted"));
    }
}
