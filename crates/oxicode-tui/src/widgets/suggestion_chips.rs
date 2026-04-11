//! Suggestion chips widget — renders context-aware follow-up prompts
//! as selectable numbered chips in a single row.

use crate::prompt_suggestions::PromptSuggestion;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

/// A row of suggestion chips: `[1: Run tests] [2: Review changes]`.
pub struct SuggestionChips<'a> {
    suggestions: &'a [PromptSuggestion],
}

impl<'a> SuggestionChips<'a> {
    pub fn new(suggestions: &'a [PromptSuggestion]) -> Self {
        Self { suggestions }
    }
}

impl Widget for SuggestionChips<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.suggestions.is_empty() || area.width < 10 || area.height == 0 {
            return;
        }

        let mut spans: Vec<Span<'_>> = Vec::new();
        spans.push(Span::styled(" ", Style::default()));

        for (i, suggestion) in self.suggestions.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            // Number prefix: bold accent.
            spans.push(Span::styled(
                format!(" {}:", i + 1),
                Style::default()
                    .fg(crate::render::STATUS_CYAN)
                    .add_modifier(Modifier::BOLD),
            ));
            // Label text: warm cream on subtle dark background.
            spans.push(Span::styled(
                format!(" {} ", suggestion.label),
                Style::default()
                    .fg(crate::render::CHROME_TEXT)
                    .bg(Color::Rgb(40, 34, 30)),
            ));
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_suggestions_renders_nothing() {
        let area = Rect::new(0, 0, 60, 1);
        let mut buf = Buffer::empty(area);
        SuggestionChips::new(&[]).render(area, &mut buf);
        // All cells should be empty/space.
        let content: String = (0..60)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert!(content.trim().is_empty());
    }

    #[test]
    fn renders_chip_labels() {
        let suggestions = vec![
            PromptSuggestion {
                label: "Run tests".to_string(),
                prompt: "Run the tests.".to_string(),
            },
            PromptSuggestion {
                label: "Review".to_string(),
                prompt: "Review changes.".to_string(),
            },
        ];
        let area = Rect::new(0, 0, 60, 1);
        let mut buf = Buffer::empty(area);
        SuggestionChips::new(&suggestions).render(area, &mut buf);
        let content: String = (0..60)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert!(content.contains("Run tests"));
        assert!(content.contains("Review"));
    }

    #[test]
    fn narrow_terminal_no_panic() {
        let suggestions = vec![PromptSuggestion {
            label: "Test".to_string(),
            prompt: "Test.".to_string(),
        }];
        let area = Rect::new(0, 0, 5, 1);
        let mut buf = Buffer::empty(area);
        // Should not panic even on narrow terminal.
        SuggestionChips::new(&suggestions).render(area, &mut buf);
    }
}
