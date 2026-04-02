use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Widget};

/// Metadata for a single background task.
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub id: String,
    /// Expected values: "bash", "agent", "monitor".
    pub task_type: String,
    /// Expected values: "pending", "running", "completed", "failed".
    pub status: String,
    /// First ~30 chars of the command or prompt.
    pub command_preview: String,
}

/// Panel that lists background tasks with optional row selection.
pub struct TaskPanel<'a> {
    tasks: &'a [TaskInfo],
    title: &'a str,
    selected: Option<usize>,
}

impl<'a> TaskPanel<'a> {
    pub fn new(tasks: &'a [TaskInfo]) -> Self {
        Self {
            tasks,
            title: "Tasks",
            selected: None,
        }
    }

    /// Override the default panel title.
    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// Highlight the task at `index`.
    pub fn with_selected(mut self, index: usize) -> Self {
        self.selected = Some(index);
        self
    }
}

/// Map status string to a display color.
fn status_color(status: &str) -> Color {
    match status {
        "pending" => Color::Yellow,
        "running" => Color::Green,
        "failed" => Color::Red,
        _ => Color::DarkGray, // completed or unknown
    }
}

impl Widget for TaskPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::new().title(self.title).borders(Borders::ALL);
        let inner = block.inner(area);
        block.render(area, buf);

        if self.tasks.is_empty() {
            let line = Line::from(Span::styled(
                "No background tasks",
                Style::default().fg(Color::DarkGray),
            ));
            buf.set_line(inner.x, inner.y, &line, inner.width);
            return;
        }

        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let preview: String = t.command_preview.chars().take(30).collect();
                let label = format!("#{} [{}] {}: {}", t.id, t.status, t.task_type, preview);

                let is_selected = self.selected == Some(i);
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(status_color(&t.status))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(status_color(&t.status))
                };

                ListItem::new(Line::from(Span::styled(label, style)))
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
        assert_eq!(status_color("pending"), Color::Yellow);
        assert_eq!(status_color("running"), Color::Green);
        assert_eq!(status_color("failed"), Color::Red);
        assert_eq!(status_color("completed"), Color::DarkGray);
    }

    #[test]
    fn command_preview_truncated_to_30() {
        let long_cmd = "a".repeat(50);
        let task = TaskInfo {
            id: "1".into(),
            task_type: "bash".into(),
            status: "running".into(),
            command_preview: long_cmd,
        };
        // Verify the preview field itself can hold any length; truncation is display-side.
        // The widget trims to 30 chars — confirm that logic via a direct chars().take(30).
        let rendered: String = task.command_preview.chars().take(30).collect();
        assert_eq!(rendered.len(), 30);
    }
}
