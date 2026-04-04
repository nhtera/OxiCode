//! Memory freshness warnings.
//!
//! Appends age-based caveats to memory content before injection into
//! the system prompt. Memories older than 1 day get an age annotation;
//! those older than 7 days also get a "verify before using" warning.

use std::time::{Duration, SystemTime};

/// Age threshold beyond which memories get an age annotation.
const STALE_THRESHOLD: Duration = Duration::from_secs(24 * 60 * 60); // 1 day

/// Age threshold beyond which memories get a "verify" warning.
const VERIFY_THRESHOLD: Duration = Duration::from_secs(7 * 24 * 60 * 60); // 7 days

/// Generate a freshness warning suffix for a memory based on its age.
///
/// Returns `None` if the memory is fresh (< 1 day old).
/// Returns `Some("(X days ago)")` for 1-7 day old memories.
/// Returns `Some("(X days ago — verify before using)")` for older ones.
pub fn freshness_warning(created_at: SystemTime) -> Option<String> {
    let age = SystemTime::now()
        .duration_since(created_at)
        .unwrap_or(Duration::ZERO);

    if age < STALE_THRESHOLD {
        return None;
    }

    let days = age.as_secs() / (24 * 60 * 60);
    let unit = if days == 1 { "day" } else { "days" };

    if age >= VERIFY_THRESHOLD {
        Some(format!("({days} {unit} ago \u{2014} verify before using)"))
    } else {
        Some(format!("({days} {unit} ago)"))
    }
}

/// Apply freshness warning to memory content, returning annotated version.
///
/// If no warning needed (memory is fresh), returns content unchanged.
pub fn annotate_memory(content: &str, created_at: SystemTime) -> String {
    match freshness_warning(created_at) {
        Some(warning) => format!("{content} {warning}"),
        None => content.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time_days_ago(days: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60)
    }

    #[test]
    fn fresh_memory_no_warning() {
        let now = SystemTime::now();
        assert!(freshness_warning(now).is_none());
    }

    #[test]
    fn one_day_old_gets_age() {
        let warning = freshness_warning(time_days_ago(1)).unwrap();
        assert!(warning.contains("1 day ago"));
        assert!(!warning.contains("verify"));
    }

    #[test]
    fn three_days_old_gets_plural_age() {
        let warning = freshness_warning(time_days_ago(3)).unwrap();
        assert!(warning.contains("3 days ago"));
        assert!(!warning.contains("verify"));
    }

    #[test]
    fn seven_days_old_gets_verify_warning() {
        let warning = freshness_warning(time_days_ago(7)).unwrap();
        assert!(warning.contains("7 days ago"));
        assert!(warning.contains("verify before using"));
    }

    #[test]
    fn thirty_days_old_gets_verify_warning() {
        let warning = freshness_warning(time_days_ago(30)).unwrap();
        assert!(warning.contains("30 days ago"));
        assert!(warning.contains("verify before using"));
    }

    #[test]
    fn annotate_fresh_memory_unchanged() {
        let content = "Use snake_case for variables";
        let result = annotate_memory(content, SystemTime::now());
        assert_eq!(result, content);
    }

    #[test]
    fn annotate_old_memory_appends_warning() {
        let content = "Prefer Rust for CLI tools";
        let result = annotate_memory(content, time_days_ago(10));
        assert!(result.starts_with(content));
        assert!(result.contains("10 days ago"));
        assert!(result.contains("verify before using"));
    }

    #[test]
    fn annotate_stale_memory_appends_age() {
        let content = "Use axum for HTTP";
        let result = annotate_memory(content, time_days_ago(3));
        assert!(result.starts_with(content));
        assert!(result.contains("3 days ago"));
        assert!(!result.contains("verify"));
    }

    #[test]
    fn future_time_no_warning() {
        // If created_at is in the future (clock skew), treat as fresh.
        let future = SystemTime::now() + Duration::from_secs(3600);
        assert!(freshness_warning(future).is_none());
    }
}
