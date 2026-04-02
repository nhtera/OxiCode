use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use oxicode_state::{AppState, StateStore};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use tokio::sync::{mpsc, watch};

use crate::events::{CoreEvent, UiEvent};
use crate::widgets::{
    AgentInfo, AgentPanel, InputBox, MessageView, Notification, NotificationWidget, SplitPane,
    StatusBar, TaskInfo, TaskPanel,
};

/// Main TUI application.
pub struct App {
    state_rx: watch::Receiver<AppState>,
    ui_tx: mpsc::Sender<UiEvent>,
    core_rx: mpsc::Receiver<CoreEvent>,
    input_text: String,
    /// Cursor position as character index (not byte index).
    input_cursor: usize,
    scroll_offset: u16,
    streaming_text: String,
    should_quit: bool,
    /// Manages left/right split layout and ratio.
    split_pane: SplitPane,
    /// Toast notifications rendered as an overlay.
    notifications: Vec<Notification>,
}

impl App {
    pub fn new(
        state_store: &Arc<StateStore>,
        ui_tx: mpsc::Sender<UiEvent>,
        core_rx: mpsc::Receiver<CoreEvent>,
    ) -> Self {
        Self {
            state_rx: state_store.subscribe(),
            ui_tx,
            core_rx,
            input_text: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            streaming_text: String::new(),
            should_quit: false,
            split_pane: SplitPane::new(),
            notifications: Vec::new(),
        }
    }

    /// Run the TUI event loop.
    pub async fn run(&mut self) -> io::Result<()> {
        // Setup terminal
        terminal::enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let result = self.event_loop(&mut terminal).await;

        // Restore terminal (always, even on error)
        let _ = terminal::disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        // C1 FIX: Single dedicated thread for all crossterm I/O.
        // Sends terminal events over mpsc channel to avoid race conditions.
        let (term_tx, mut term_rx) = mpsc::channel::<Event>(32);
        tokio::task::spawn_blocking(move || loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(ev) = event::read() {
                    if term_tx.blocking_send(ev).is_err() {
                        break;
                    }
                }
            }
        });

        loop {
            self.draw(terminal)?;

            if self.should_quit {
                break;
            }

            tokio::select! {
                Some(ev) = term_rx.recv() => {
                    if let Event::Key(key) = ev {
                        self.handle_key(key).await;
                    }
                }
                Some(core_event) = self.core_rx.recv() => {
                    self.handle_core_event(core_event);
                }
            }
        }

        Ok(())
    }

    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
        let state = self.state_rx.borrow().clone();

        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Status bar
                    Constraint::Min(5),    // Message view
                    Constraint::Length(3), // Input box
                ])
                .split(frame.area());

            // Status bar
            let status_bar =
                StatusBar::new(&state.current_model, &state.total_usage, state.is_streaming);
            frame.render_widget(status_bar, chunks[0]);

            // Content area — optionally split into left (messages) + right (agents/tasks)
            let content_area = chunks[1];
            let (left_area, right_area) = self.split_pane.split(content_area);

            // Message view (left pane, or full area when right pane is hidden)
            let streaming = if state.is_streaming && !self.streaming_text.is_empty() {
                Some(self.streaming_text.as_str())
            } else {
                None
            };
            let message_view = MessageView::new(&state.messages, streaming, self.scroll_offset);
            frame.render_widget(message_view, left_area);

            // Right pane: agent panel (top) + task panel (bottom)
            if let Some(right) = right_area {
                let agent_infos: Vec<AgentInfo> = state
                    .active_agents
                    .iter()
                    .map(|a| AgentInfo {
                        name: a.name.clone(),
                        status: a.status.clone(),
                        started_at: a.started_at.clone(),
                        duration: String::new(),
                    })
                    .collect();

                let task_infos: Vec<TaskInfo> = state
                    .background_tasks
                    .iter()
                    .map(|t| TaskInfo {
                        id: t.id.clone(),
                        task_type: t.task_type.clone(),
                        status: t.status.clone(),
                        command_preview: t.command_preview.clone(),
                    })
                    .collect();

                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(right);

                frame.render_widget(AgentPanel::new(&agent_infos), right_chunks[0]);
                frame.render_widget(TaskPanel::new(&task_infos), right_chunks[1]);
            }

            // Notification toast overlay (drawn on top of content area)
            if !self.notifications.is_empty() {
                let notif_widget =
                    NotificationWidget::new(&self.notifications).with_max_visible(3);
                frame.render_widget(notif_widget, content_area);
            }

            // Input box — convert char cursor to byte offset for widget
            let byte_cursor = char_to_byte_index(&self.input_text, self.input_cursor);
            let input = InputBox::new(&self.input_text, byte_cursor, true);
            frame.render_widget(input, chunks[2]);
        })?;

        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                let _ = self.ui_tx.send(UiEvent::Quit).await;
                self.should_quit = true;
            }
            (_, KeyCode::Enter) => {
                if !self.input_text.is_empty() {
                    let text = std::mem::take(&mut self.input_text);
                    self.input_cursor = 0;
                    let _ = self.ui_tx.send(UiEvent::UserInput(text)).await;
                }
            }
            // H3 FIX: cursor operates on char count, insert at byte offset
            (_, KeyCode::Char(c)) => {
                let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                self.input_text.insert(byte_idx, c);
                self.input_cursor += 1;
            }
            (_, KeyCode::Backspace) => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let byte_idx = char_to_byte_index(&self.input_text, self.input_cursor);
                    self.input_text.remove(byte_idx);
                }
            }
            // Ctrl+Left / Ctrl+Right adjust the split ratio by ±5 %.
            // Must come before the wildcard (_, KeyCode::Left/Right) arms.
            (KeyModifiers::CONTROL, KeyCode::Left) => {
                self.split_pane.adjust_ratio(-5);
            }
            (KeyModifiers::CONTROL, KeyCode::Right) => {
                self.split_pane.adjust_ratio(5);
            }
            (_, KeyCode::Left) => {
                self.input_cursor = self.input_cursor.saturating_sub(1);
            }
            (_, KeyCode::Right) => {
                let char_count = self.input_text.chars().count();
                if self.input_cursor < char_count {
                    self.input_cursor += 1;
                }
            }
            (_, KeyCode::Up) => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            (_, KeyCode::Down) => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            (_, KeyCode::Home) => {
                self.input_cursor = 0;
            }
            (_, KeyCode::End) => {
                self.input_cursor = self.input_text.chars().count();
            }
            // Tab toggles the right split pane (agents + tasks panel).
            (_, KeyCode::Tab) => {
                self.split_pane.toggle_right();
            }
            _ => {}
        }
    }

    fn handle_core_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::TextDelta(text) => {
                self.streaming_text.push_str(&text);
                self.scroll_offset = u16::MAX;
            }
            CoreEvent::StreamStart => {
                self.streaming_text.clear();
            }
            CoreEvent::StreamEnd | CoreEvent::MessageComplete => {
                self.streaming_text.clear();
                self.scroll_offset = u16::MAX;
            }
            CoreEvent::Error(msg) => {
                tracing::error!("Core error: {}", msg);
            }
        }
    }
}

/// Convert a character index to a byte index in a UTF-8 string.
fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_to_byte_index_ascii() {
        assert_eq!(char_to_byte_index("hello", 0), 0);
        assert_eq!(char_to_byte_index("hello", 3), 3);
        assert_eq!(char_to_byte_index("hello", 5), 5);
    }

    #[test]
    fn test_char_to_byte_index_utf8() {
        let s = "héllo"; // é is 2 bytes
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 1); // 'h' at 0, 'é' starts at 1
        assert_eq!(char_to_byte_index(s, 2), 3); // 'l' starts at byte 3
    }

    #[test]
    fn test_char_to_byte_index_emoji() {
        let s = "a😀b";
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 1); // emoji starts at byte 1
        assert_eq!(char_to_byte_index(s, 2), 5); // 'b' starts at byte 5
        assert_eq!(char_to_byte_index(s, 3), 6); // past end
    }
}
