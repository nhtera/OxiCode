/// Core event handler: handle_core_event — drives all state from engine events.
use std::time::Instant;

use crate::events::CoreEvent;
use crate::prompt_suggestions::suggest_prompts;
use crate::widgets::Notification;
use crate::widgets::permission_dialog::RiskLevel;

use super::{ActiveToolCall, HookKind, HookLogEntry};
use super::utils::{display_name_and_summary, is_dangerous_operation};
use super::truncate_hook_content;

impl super::App {
    #[allow(clippy::too_many_lines)]
    pub(super) fn handle_core_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::TextDelta(text) => {
                self.streaming_text.push_str(&text);
                self.streaming_collector.push_delta(&text);
                let new_lines = self.streaming_collector.commit_complete_lines();
                self.streaming_committed_lines.extend(new_lines);
                self.auto_scroll = true;
                // Reset stall timer on each delta (data is flowing).
                self.stall_start = Some(Instant::now());
            }
            CoreEvent::StreamStart => {
                self.is_turn_active = true;
                self.turn_started_at = Some(Instant::now());
                self.stall_start = Some(Instant::now());
                self.last_turn_duration = None; // Clear previous turn's duration.
                self.streaming_text.clear();
                self.streaming_collector.clear();
                self.streaming_committed_lines.clear();
                self.streaming_thinking.clear();
                self.active_tools.clear();
                // Clear retry label when a new stream starts.
                self.retry_status_label.clear();
                // Snapshot message count so draw() can detect when the engine
                // commits the response to state before MessageComplete fires.
                self.pre_streaming_msg_count = self.state_rx.borrow().messages.len();
            }
            CoreEvent::StreamEnd => {
                // Stream finished — finalize remaining buffer but keep committed
                // lines visible until MessageComplete (prevents blank frame flash).
                // Tools may still be running in multi-turn loops.
                let final_lines = self.streaming_collector.finalize();
                self.streaming_committed_lines.extend(final_lines);
                // Clear only the raw buffer and collector, NOT the committed lines.
                self.streaming_text.clear();
                self.streaming_collector.clear();
                self.auto_scroll = true;
            }
            CoreEvent::MessageComplete => {
                // Full turn complete — message now persisted in state_store.messages.
                // Save turn duration before clearing timer.
                self.last_turn_duration = self.turn_started_at.map(|t| t.elapsed());
                // Force message cache update BEFORE clearing streaming state to
                // prevent a blank frame between "streaming visible" and "cached visible".
                {
                    let state = self.state_rx.borrow();
                    // Use a reasonable width; draw() will recalculate with actual terminal width.
                    self.message_cache
                        .update(&state.messages, self.message_cache.cached_width());
                }
                // Now safe to clear all transient streaming state.
                self.streaming_text.clear();
                self.streaming_collector.clear();
                self.streaming_committed_lines.clear();
                self.streaming_thinking.clear();
                self.active_tools.clear();
                self.is_turn_active = false;
                self.turn_started_at = None;
                self.stall_start = None;
                self.auto_scroll = true;
                // Compute context-aware suggestions from latest messages.
                let state = self.state_rx.borrow();
                self.suggestions = suggest_prompts(&state.messages);
            }
            CoreEvent::Error(msg) => {
                // Show error as a toast notification (no tracing::error! to avoid
                // corrupting the TUI — stderr leaks into the alternate screen).
                self.notifications.push(Notification::new(
                    msg,
                    crate::widgets::notification::NotificationLevel::Error,
                ));
                // Clear streaming state on error (engine may not send StreamEnd).
                self.streaming_text.clear();
                self.streaming_collector.clear();
                self.streaming_committed_lines.clear();
                self.streaming_thinking.clear();
                self.active_tools.clear();
                self.is_turn_active = false;
                self.message_queue.clear();
                // Reset turn timer — otherwise the "thinking" indicator would
                // keep displaying a stale elapsed duration after an API error.
                self.turn_started_at = None;
                self.stall_start = None;
            }
            CoreEvent::ToolUseStart { id, name, input } => {
                let (display_name, summary) = display_name_and_summary(&name, &input);
                self.active_tools.push(ActiveToolCall {
                    id,
                    name: display_name,
                    input_summary: summary,
                    raw_input: input,
                    started_at: Instant::now(),
                    result: None,
                });
                self.auto_scroll = true;
            }
            CoreEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                if let Some(tool) = self.active_tools.iter_mut().find(|t| t.id == tool_use_id) {
                    tool.result = Some((content, is_error));
                }
                self.auto_scroll = true;
            }
            CoreEvent::PermissionAsk {
                tool_name,
                input_summary,
                prompt,
                reply_tx,
            } => {
                // Auto-approve bash commands matching a previously approved prefix.
                if tool_name == "bash" || tool_name == "Bash" {
                    let first_word = input_summary
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if self.bash_prefix_allowlist.contains(&first_word) {
                        let _ = reply_tx.send(oxicode_common::PermissionResponse::AllowOnce);
                        self.notifications.push(Notification::new(
                            format!("Auto-approved: {first_word} (prefix allowlist)"),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                        return;
                    }
                }

                // Compute risk level: start from tool name, then escalate if
                // the input contains dangerous patterns (e.g. rm -rf for Bash).
                let mut risk_level = RiskLevel::from_tool(&tool_name);
                if risk_level != RiskLevel::High
                    && is_dangerous_operation(&tool_name, &input_summary)
                {
                    risk_level = RiskLevel::High;
                }
                let kind =
                    crate::widgets::PermissionDialogKind::detect(&tool_name, &input_summary);
                self.pending_permission = Some(super::PendingPermission {
                    tool_name,
                    input_summary,
                    prompt,
                    selected: 0,
                    risk_level,
                    kind,
                    created_at: Instant::now(),
                    reply_tx,
                });
            }
            CoreEvent::RateLimited {
                message: _,
                attempt,
                max_retries,
                retry_in_secs,
            } => {
                let notif_msg = format!(
                    "Rate limited. Retrying in {retry_in_secs:.0}s... ({attempt}/{max_retries})"
                );
                // No tracing::warn! — would corrupt TUI display via stderr.
                self.notifications.push(Notification::new(
                    notif_msg,
                    crate::widgets::notification::NotificationLevel::RateLimit,
                ));
                // Update status bar indicator so rate-limit state is always visible.
                self.retry_status_label =
                    format!("⟳ Rate limited ({attempt}/{max_retries}) …{retry_in_secs:.0}s");
            }
            CoreEvent::Retrying {
                message,
                attempt,
                max_retries,
                retry_in_secs,
            } => {
                let notif_msg =
                    format!("Retrying ({attempt}/{max_retries}) in {retry_in_secs:.0}s: {message}");
                self.notifications.push(Notification::new(
                    notif_msg,
                    crate::widgets::notification::NotificationLevel::Warning,
                ));
                // Update status bar indicator.
                self.retry_status_label =
                    format!("⟳ Retrying {attempt}/{max_retries} …{retry_in_secs:.0}s");
            }
            CoreEvent::ThinkingDelta(text) => {
                // Accumulate thinking text for live display during streaming.
                self.streaming_thinking.push_str(&text);
                self.auto_scroll = true;
            }
            CoreEvent::HookProgress { event, state } => match state.as_str() {
                "running" => self.active_hook = Some(event),
                "completed" => self.active_hook = None,
                _ => {}
            },
            CoreEvent::HookMessage {
                event,
                kind,
                content,
            } => {
                let kind = HookKind::parse(&kind);
                let trimmed = truncate_hook_content(&content, super::MAX_HOOK_LINE_CHARS);
                let display_line = format!("▪ {event}: {trimmed}");
                let level = match kind {
                    HookKind::BlockError => {
                        crate::widgets::notification::NotificationLevel::Warning
                    }
                    _ => crate::widgets::notification::NotificationLevel::Info,
                };
                self.notifications.push(Notification::new(display_line, level));
                self.hook_messages.push(HookLogEntry {
                    event,
                    kind,
                    content: trimmed,
                });
                // Cap log size to avoid unbounded growth across long sessions.
                if self.hook_messages.len() > 500 {
                    self.hook_messages.drain(0..self.hook_messages.len() - 500);
                }
            }
            CoreEvent::SessionResumed { session_id } => {
                // State messages were replaced by the engine. Drop cached renders,
                // rebuild click-to-open map from the new transcript, and snap to
                // bottom so the user sees the newest turn.
                self.message_cache = crate::widgets::MessageRenderCache::new();
                self.hydrate_image_paths_from_session();
                self.scroll_offset = u16::MAX;
                self.auto_scroll = true;
                self.notifications.push(Notification::new(
                    format!("Resumed session {}", &session_id[..8.min(session_id.len())]),
                    crate::widgets::notification::NotificationLevel::Info,
                ));
            }
        }
    }
}
