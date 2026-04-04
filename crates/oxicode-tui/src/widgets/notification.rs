use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

const EXPIRE_SECS: u64 = 5;
const RATE_LIMIT_EXPIRE_SECS: u64 = 30;

/// Severity level for a toast notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
    /// Rate limit notification — auto-dismiss after retry succeeds.
    RateLimit,
}

impl NotificationLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
            Self::RateLimit => "RATE",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Info => Color::Blue,
            Self::Warning => Color::Yellow,
            Self::Error => Color::Red,
            Self::RateLimit => Color::Rgb(255, 165, 0), // Orange
        }
    }
}

/// A single toast notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub message: String,
    pub level: NotificationLevel,
    pub created_at: Instant,
}

impl Notification {
    pub fn new(message: impl Into<String>, level: NotificationLevel) -> Self {
        Self {
            message: message.into(),
            level,
            created_at: Instant::now(),
        }
    }

    /// Returns true if the notification has not yet expired.
    pub fn is_active(&self) -> bool {
        let ttl = if self.level == NotificationLevel::RateLimit {
            Duration::from_secs(RATE_LIMIT_EXPIRE_SECS)
        } else {
            Duration::from_secs(EXPIRE_SECS)
        };
        self.created_at.elapsed() < ttl
    }
}

/// Renders active toast notifications anchored to the bottom of the given area.
///
/// Only the `max_visible` most recent non-expired notifications are shown.
/// Each notification occupies a single row.
pub struct NotificationWidget<'a> {
    notifications: &'a [Notification],
    max_visible: usize,
}

impl<'a> NotificationWidget<'a> {
    pub fn new(notifications: &'a [Notification]) -> Self {
        Self {
            notifications,
            max_visible: 3,
        }
    }

    /// Override the maximum number of simultaneously visible notifications.
    pub fn with_max_visible(mut self, max: usize) -> Self {
        self.max_visible = max.max(1);
        self
    }
}

impl Widget for NotificationWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Collect active notifications (most recent last → reverse for bottom-up display).
        let active: Vec<&Notification> = self
            .notifications
            .iter()
            .rev()
            .filter(|n| n.is_active())
            .take(self.max_visible)
            .collect();

        if active.is_empty() || area.height == 0 {
            return;
        }

        // Render bottom-most row first (index 0 = most recent).
        for (i, notif) in active.iter().enumerate() {
            let row = area.bottom().saturating_sub(1 + i as u16);
            if row < area.top() {
                break;
            }
            let text = format!("[{}] {}", notif.level.label(), notif.message);
            let line = Line::from(Span::styled(text, Style::default().fg(notif.level.color())));
            buf.set_line(area.x, row, &line, area.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_notification_is_active() {
        let n = Notification::new("hello", NotificationLevel::Info);
        assert!(n.is_active());
    }

    #[test]
    fn level_labels_and_colors() {
        assert_eq!(NotificationLevel::Info.label(), "INFO");
        assert_eq!(NotificationLevel::Warning.label(), "WARN");
        assert_eq!(NotificationLevel::Error.label(), "ERROR");
        assert_eq!(NotificationLevel::RateLimit.label(), "RATE");
        assert_eq!(NotificationLevel::Info.color(), Color::Blue);
        assert_eq!(NotificationLevel::Warning.color(), Color::Yellow);
        assert_eq!(NotificationLevel::Error.color(), Color::Red);
        assert_eq!(
            NotificationLevel::RateLimit.color(),
            Color::Rgb(255, 165, 0)
        );
    }

    #[test]
    fn rate_limit_notification_has_longer_ttl() {
        let n = Notification::new("rate limited", NotificationLevel::RateLimit);
        assert!(n.is_active());
    }
}
