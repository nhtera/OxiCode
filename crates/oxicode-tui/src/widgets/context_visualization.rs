//! Context visualization widget: token usage bar with color coding.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

/// Context defense layer levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseLayer {
    /// Full context available.
    None,
    /// Summarization active.
    Summarize,
    /// Microcompact mode (aggressive trimming).
    Microcompact,
}

impl DefenseLayer {
    /// Short label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Summarize => "Summarize",
            Self::Microcompact => "Microcompact",
        }
    }
}

/// Token usage context visualization.
pub struct ContextVisualization {
    used_tokens: u32,
    max_tokens: u32,
    defense_layer: DefenseLayer,
}

impl ContextVisualization {
    pub fn new(used_tokens: u32, max_tokens: u32) -> Self {
        Self {
            used_tokens,
            max_tokens,
            defense_layer: DefenseLayer::None,
        }
    }

    /// Set the active context defense layer.
    pub fn with_defense_layer(mut self, layer: DefenseLayer) -> Self {
        self.defense_layer = layer;
        self
    }
}

/// Map usage percentage to a color.
fn usage_color(pct: f64) -> Color {
    if pct < 50.0 {
        Color::Green
    } else if pct < 80.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

impl Widget for ContextVisualization {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default().title(" Context ").borders(Borders::ALL);
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width < 10 {
            return;
        }

        let pct = if self.max_tokens > 0 {
            f64::from(self.used_tokens) / f64::from(self.max_tokens) * 100.0
        } else {
            0.0
        };

        // Bar: [████████░░░░░] 65% (130K/200K tokens)
        let bar_width = (inner.width as usize).saturating_sub(2);
        let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
        let empty = bar_width.saturating_sub(filled);

        let bar_filled = "\u{2588}".repeat(filled);
        let bar_empty = "\u{2591}".repeat(empty);
        let color = usage_color(pct);

        let used_k = self.used_tokens / 1000;
        let max_k = self.max_tokens / 1000;

        let bar_line = Line::from(vec![
            Span::raw("["),
            Span::styled(&bar_filled, Style::default().fg(color)),
            Span::styled(&bar_empty, Style::default().fg(Color::DarkGray)),
            Span::raw("] "),
            Span::styled(
                format!("{pct:.0}% ({used_k}K/{max_k}K)"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]);
        buf.set_line(inner.x, inner.y, &bar_line, inner.width);

        // Defense layer indicator.
        if inner.height > 1 {
            let layer_line = Line::from(Span::styled(
                format!("Defense: {}", self.defense_layer.label()),
                Style::default().fg(Color::Cyan),
            ));
            buf.set_line(inner.x, inner.y + 1, &layer_line, inner.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_color_green() {
        assert_eq!(usage_color(30.0), Color::Green);
    }

    #[test]
    fn test_usage_color_yellow() {
        assert_eq!(usage_color(60.0), Color::Yellow);
    }

    #[test]
    fn test_usage_color_red() {
        assert_eq!(usage_color(90.0), Color::Red);
    }

    #[test]
    fn test_defense_layer_labels() {
        assert_eq!(DefenseLayer::None.label(), "None");
        assert_eq!(DefenseLayer::Summarize.label(), "Summarize");
        assert_eq!(DefenseLayer::Microcompact.label(), "Microcompact");
    }
}
