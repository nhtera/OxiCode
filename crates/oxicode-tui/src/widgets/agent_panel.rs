use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Widget};

/// Metadata for a single background agent.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    /// Expected values: "running", "completed", "failed".
    pub status: String,
    pub started_at: String,
    pub duration: String,
}

/// Panel that lists active/completed background agents.
pub struct AgentPanel<'a> {
    agents: &'a [AgentInfo],
    title: &'a str,
}

impl<'a> AgentPanel<'a> {
    pub fn new(agents: &'a [AgentInfo]) -> Self {
        Self {
            agents,
            title: "Agents",
        }
    }

    /// Override the default panel title.
    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }
}

/// Map status string to a display color.
fn status_color(status: &str) -> Color {
    match status {
        "running" => Color::Green,
        "failed" => Color::Red,
        _ => Color::DarkGray, // completed or unknown
    }
}

impl Widget for AgentPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::new().title(self.title).borders(Borders::ALL);
        let inner = block.inner(area);
        block.render(area, buf);

        if self.agents.is_empty() {
            let line = Line::from(Span::styled(
                "No active agents",
                Style::default().fg(Color::DarkGray),
            ));
            buf.set_line(inner.x, inner.y, &line, inner.width);
            return;
        }

        let items: Vec<ListItem> = self
            .agents
            .iter()
            .map(|a| {
                let label = format!("{} [{}] {}", a.name, a.status, a.duration);
                ListItem::new(Line::from(Span::styled(
                    label,
                    Style::default().fg(status_color(&a.status)),
                )))
            })
            .collect();

        List::new(items).render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_color_mapping() {
        assert_eq!(status_color("running"), Color::Green);
        assert_eq!(status_color("failed"), Color::Red);
        assert_eq!(status_color("completed"), Color::DarkGray);
        assert_eq!(status_color("unknown"), Color::DarkGray);
    }
}
