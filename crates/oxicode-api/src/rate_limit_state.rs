//! Unified rate limit state machine with user-facing messages.
//!
//! Parses `anthropic-ratelimit-unified-*` headers into a state machine that
//! produces actionable messages for the user. Falls back to legacy headers
//! when unified headers are absent.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Information parsed from unified rate limit headers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnifiedRateLimitInfo {
    /// Unified status: "allowed", "warning", "exceeded".
    pub status: Option<String>,
    /// When the current limit window resets.
    pub resets_at: Option<DateTime<Utc>>,
    /// The limit claim for the current billing period.
    pub limit_claim: Option<String>,
    /// Overage status: "active", "warning", or absent.
    pub overage_status: Option<String>,
    /// When overage resets.
    pub overage_resets_at: Option<DateTime<Utc>>,
    /// Current utilization as a fraction (0.0 - 1.0+).
    pub utilization: Option<f64>,
}

impl UnifiedRateLimitInfo {
    /// Parse unified headers from a response header map.
    pub fn from_headers(headers: &reqwest::header::HeaderMap) -> Self {
        Self {
            status: header_str(headers, "anthropic-ratelimit-unified-status"),
            resets_at: header_datetime(headers, "anthropic-ratelimit-unified-resets-at"),
            limit_claim: header_str(headers, "anthropic-ratelimit-unified-limit-claim"),
            overage_status: header_str(headers, "anthropic-ratelimit-unified-overage-status"),
            overage_resets_at: header_datetime(
                headers,
                "anthropic-ratelimit-unified-overage-resets-at",
            ),
            utilization: header_f64(headers, "anthropic-ratelimit-unified-utilization"),
        }
    }

    /// Whether any unified headers were present.
    pub fn has_unified(&self) -> bool {
        self.status.is_some() || self.utilization.is_some()
    }
}

/// Rate limit state derived from header evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RateLimitState {
    /// Under limits, normal operation.
    Normal,
    /// Approaching limit. Contains utilization percentage (0-100).
    Warning { utilization_pct: u8 },
    /// At limit, requests rejected. Contains reset time.
    Rejected { resets_at: Option<DateTime<Utc>> },
    /// Overage is active — extra charges apply.
    OverageActive { resets_at: Option<DateTime<Utc>> },
    /// Approaching overage threshold.
    OverageWarning,
}

/// Severity level of a rate limit message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageSeverity {
    Info,
    Warning,
    Error,
}

/// User-facing rate limit message with severity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitMessage {
    pub severity: MessageSeverity,
    pub text: String,
    /// Short label for TUI status bar (e.g., "80%", "LIMIT", "OVERAGE").
    pub status_label: String,
}

/// Evaluate unified rate limit info into a state.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn evaluate(info: &UnifiedRateLimitInfo) -> RateLimitState {
    // Check overage first (highest priority)
    if let Some(ref overage) = info.overage_status {
        match overage.as_str() {
            "active" => {
                return RateLimitState::OverageActive {
                    resets_at: info.overage_resets_at,
                }
            }
            "warning" => return RateLimitState::OverageWarning,
            _ => {}
        }
    }

    // Check unified status
    if let Some(ref status) = info.status {
        match status.as_str() {
            "exceeded" => {
                return RateLimitState::Rejected {
                    resets_at: info.resets_at,
                }
            }
            "warning" => {
                let pct = info
                    .utilization
                    .map_or(80, |u| (u * 100.0).clamp(0.0, 100.0) as u8);
                return RateLimitState::Warning {
                    utilization_pct: pct,
                };
            }
            _ => {}
        }
    }

    // Check utilization threshold (fallback if status not "warning" yet)
    if let Some(util) = info.utilization {
        if util >= 0.80 {
            return RateLimitState::Warning {
                utilization_pct: (util * 100.0).clamp(0.0, 100.0) as u8,
            };
        }
    }

    RateLimitState::Normal
}

/// Build a user-facing message for the given state.
pub fn build_message(state: &RateLimitState, model: &str) -> RateLimitMessage {
    match state {
        RateLimitState::Normal => RateLimitMessage {
            severity: MessageSeverity::Info,
            text: String::new(),
            status_label: String::new(),
        },
        RateLimitState::Warning { utilization_pct } => RateLimitMessage {
            severity: MessageSeverity::Warning,
            text: format!(
                "Approaching rate limit ({utilization_pct}% used). Consider using a smaller model."
            ),
            status_label: format!("{utilization_pct}%"),
        },
        RateLimitState::Rejected { resets_at } => {
            let reset_str = resets_at
                .map(format_duration_until)
                .unwrap_or_else(|| "soon".to_string());
            RateLimitMessage {
                severity: MessageSeverity::Error,
                text: format!(
                    "Rate limited on {model}. Resets in {reset_str}. Try a smaller model or wait."
                ),
                status_label: "LIMIT".to_string(),
            }
        }
        RateLimitState::OverageActive { resets_at } => {
            let reset_str = resets_at
                .map(format_duration_until)
                .unwrap_or_else(|| "end of billing period".to_string());
            RateLimitMessage {
                severity: MessageSeverity::Error,
                text: format!("Overage active — extra charges apply. Resets {reset_str}."),
                status_label: "OVERAGE".to_string(),
            }
        }
        RateLimitState::OverageWarning => RateLimitMessage {
            severity: MessageSeverity::Warning,
            text: "Approaching overage threshold. Extra charges may apply soon.".to_string(),
            status_label: "OVG WARN".to_string(),
        },
    }
}

/// Format duration from now until target as human-readable (e.g., "5m", "2h 30m").
fn format_duration_until(target: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = target.signed_duration_since(now);
    let total_secs = diff.num_seconds().max(0);

    if total_secs < 60 {
        format!("{total_secs}s")
    } else if total_secs < 3600 {
        format!("{}m", total_secs / 60)
    } else {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        if mins > 0 {
            format!("{hours}h {mins}m")
        } else {
            format!("{hours}h")
        }
    }
}

// ── Header helpers ──

fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(ToString::to_string)
}

fn header_datetime(headers: &reqwest::header::HeaderMap, name: &str) -> Option<DateTime<Utc>> {
    let val = headers.get(name)?.to_str().ok()?;
    val.parse::<DateTime<Utc>>().ok().or_else(|| {
        DateTime::parse_from_rfc2822(val)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    })
}

fn header_f64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<f64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn make_unified_headers(status: &str, util: f64) -> UnifiedRateLimitInfo {
        UnifiedRateLimitInfo {
            status: Some(status.to_string()),
            utilization: Some(util),
            ..Default::default()
        }
    }

    // ── State evaluation tests ──

    #[test]
    fn normal_when_no_headers() {
        let info = UnifiedRateLimitInfo::default();
        assert_eq!(evaluate(&info), RateLimitState::Normal);
    }

    #[test]
    fn normal_when_low_utilization() {
        let info = make_unified_headers("allowed", 0.5);
        assert_eq!(evaluate(&info), RateLimitState::Normal);
    }

    #[test]
    fn warning_when_status_warning() {
        let info = make_unified_headers("warning", 0.85);
        assert_eq!(
            evaluate(&info),
            RateLimitState::Warning {
                utilization_pct: 85
            }
        );
    }

    #[test]
    fn warning_when_utilization_high() {
        let info = UnifiedRateLimitInfo {
            status: Some("allowed".to_string()),
            utilization: Some(0.92),
            ..Default::default()
        };
        assert_eq!(
            evaluate(&info),
            RateLimitState::Warning {
                utilization_pct: 92
            }
        );
    }

    #[test]
    fn rejected_when_exceeded() {
        let reset = Utc::now() + chrono::Duration::minutes(5);
        let info = UnifiedRateLimitInfo {
            status: Some("exceeded".to_string()),
            resets_at: Some(reset),
            ..Default::default()
        };
        let state = evaluate(&info);
        assert!(matches!(state, RateLimitState::Rejected { .. }));
    }

    #[test]
    fn overage_active() {
        let info = UnifiedRateLimitInfo {
            overage_status: Some("active".to_string()),
            overage_resets_at: Some(Utc::now() + chrono::Duration::hours(24)),
            ..Default::default()
        };
        assert!(matches!(
            evaluate(&info),
            RateLimitState::OverageActive { .. }
        ));
    }

    #[test]
    fn overage_warning() {
        let info = UnifiedRateLimitInfo {
            overage_status: Some("warning".to_string()),
            ..Default::default()
        };
        assert_eq!(evaluate(&info), RateLimitState::OverageWarning);
    }

    #[test]
    fn overage_takes_priority_over_status() {
        let info = UnifiedRateLimitInfo {
            status: Some("exceeded".to_string()),
            overage_status: Some("active".to_string()),
            ..Default::default()
        };
        // Overage should win
        assert!(matches!(
            evaluate(&info),
            RateLimitState::OverageActive { .. }
        ));
    }

    // ── Message tests ──

    #[test]
    fn normal_message_empty() {
        let msg = build_message(&RateLimitState::Normal, "claude-sonnet-4");
        assert!(msg.text.is_empty());
        assert_eq!(msg.severity, MessageSeverity::Info);
    }

    #[test]
    fn warning_message_includes_pct() {
        let msg = build_message(
            &RateLimitState::Warning {
                utilization_pct: 85,
            },
            "claude-sonnet-4",
        );
        assert!(msg.text.contains("85%"));
        assert_eq!(msg.severity, MessageSeverity::Warning);
        assert_eq!(msg.status_label, "85%");
    }

    #[test]
    fn rejected_message_includes_model() {
        let msg = build_message(
            &RateLimitState::Rejected { resets_at: None },
            "claude-sonnet-4",
        );
        assert!(msg.text.contains("claude-sonnet-4"));
        assert!(msg.text.contains("Rate limited"));
        assert_eq!(msg.severity, MessageSeverity::Error);
        assert_eq!(msg.status_label, "LIMIT");
    }

    #[test]
    fn overage_message() {
        let msg = build_message(
            &RateLimitState::OverageActive { resets_at: None },
            "claude-sonnet-4",
        );
        assert!(msg.text.contains("Overage"));
        assert!(msg.text.contains("extra charges"));
        assert_eq!(msg.status_label, "OVERAGE");
    }

    #[test]
    fn overage_warning_message() {
        let msg = build_message(&RateLimitState::OverageWarning, "claude-sonnet-4");
        assert!(msg.text.contains("overage threshold"));
        assert_eq!(msg.severity, MessageSeverity::Warning);
    }

    // ── Header parsing tests ──

    #[test]
    fn parse_unified_headers_full() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-unified-status",
            HeaderValue::from_static("warning"),
        );
        headers.insert(
            "anthropic-ratelimit-unified-utilization",
            HeaderValue::from_static("0.85"),
        );
        headers.insert(
            "anthropic-ratelimit-unified-resets-at",
            HeaderValue::from_static("2026-04-04T12:00:00Z"),
        );
        headers.insert(
            "anthropic-ratelimit-unified-limit-claim",
            HeaderValue::from_static("tier-4"),
        );

        let info = UnifiedRateLimitInfo::from_headers(&headers);
        assert_eq!(info.status.as_deref(), Some("warning"));
        assert_eq!(info.utilization, Some(0.85));
        assert!(info.resets_at.is_some());
        assert_eq!(info.limit_claim.as_deref(), Some("tier-4"));
        assert!(info.has_unified());
    }

    #[test]
    fn parse_unified_headers_empty() {
        let headers = HeaderMap::new();
        let info = UnifiedRateLimitInfo::from_headers(&headers);
        assert!(!info.has_unified());
        assert!(info.status.is_none());
    }

    #[test]
    fn parse_overage_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-unified-overage-status",
            HeaderValue::from_static("active"),
        );
        headers.insert(
            "anthropic-ratelimit-unified-overage-resets-at",
            HeaderValue::from_static("2026-05-01T00:00:00Z"),
        );

        let info = UnifiedRateLimitInfo::from_headers(&headers);
        assert_eq!(info.overage_status.as_deref(), Some("active"));
        assert!(info.overage_resets_at.is_some());
    }

    // ── Duration formatting ──

    #[test]
    fn format_duration_seconds() {
        let target = Utc::now() + chrono::Duration::seconds(30);
        let s = format_duration_until(target);
        assert!(s.ends_with('s'));
    }

    #[test]
    fn format_duration_minutes() {
        let target = Utc::now() + chrono::Duration::minutes(5);
        let s = format_duration_until(target);
        assert!(s.contains('m'));
    }

    #[test]
    fn format_duration_hours() {
        let target = Utc::now() + chrono::Duration::hours(2) + chrono::Duration::minutes(15);
        let s = format_duration_until(target);
        assert!(s.contains('h'));
    }

    #[test]
    fn format_duration_past_returns_zero() {
        let target = Utc::now() - chrono::Duration::minutes(5);
        let s = format_duration_until(target);
        assert_eq!(s, "0s");
    }
}
