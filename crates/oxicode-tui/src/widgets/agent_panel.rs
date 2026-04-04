use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
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
    /// Model used by this agent (e.g. "sonnet", "opus").
    pub model: String,
    /// Tools this agent is restricted from using (empty = no restrictions).
    pub restricted_tools: Vec<String>,
}

/// Panel that lists active/completed background agents with coordinator status.
pub struct AgentPanel<'a> {
    agents: &'a [AgentInfo],
    title: &'a str,
    /// Whether to show the extended coordinator view (tool restrictions).
    show_coordinator: bool,
}

impl<'a> AgentPanel<'a> {
    pub fn new(agents: &'a [AgentInfo]) -> Self {
        Self {
            agents,
            title: "Agents",
            show_coordinator: false,
        }
    }

    /// Override the default panel title.
    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// Enable coordinator status section (shows tool restrictions per agent).
    pub fn with_coordinator(mut self, show: bool) -> Self {
        self.show_coordinator = show;
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

        let mut items: Vec<ListItem> = Vec::new();

        for agent in self.agents {
            // Primary line: name [status] duration
            let label = format!("{} [{}] {}", agent.name, agent.status, agent.duration);
            let mut lines = vec![Line::from(Span::styled(
                label,
                Style::default().fg(status_color(&agent.status)),
            ))];

            // Coordinator detail lines (model + tool restrictions).
            if self.show_coordinator {
                if !agent.model.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("  Model: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            &agent.model,
                            Style::default().fg(Color::Cyan),
                        ),
                    ]));
                }
                if !agent.restricted_tools.is_empty() {
                    let tools_str = agent.restricted_tools.join(", ");
                    lines.push(Line::from(vec![
                        Span::styled("  Restricted: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            tools_str,
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::DIM),
                        ),
                    ]));
                }
            }

            items.push(ListItem::new(lines));
        }

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
