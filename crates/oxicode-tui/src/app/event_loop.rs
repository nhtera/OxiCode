/// Event loop: run(), spawn_terminal_event_listener(), event_loop().
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event,
};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::Backend;
use ratatui::Terminal;
use tokio::sync::mpsc;

impl super::App {
    /// Run the TUI event loop.
    pub async fn run(&mut self) -> io::Result<()> {
        // Install panic hook to restore terminal on panic.
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
            let _ = io::stdout().execute(DisableMouseCapture);
            let _ = io::stdout().execute(LeaveAlternateScreen);
            original_hook(info);
        }));

        // Setup terminal
        terminal::enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        io::stdout().execute(EnableMouseCapture)?;
        io::stdout().execute(EnableBracketedPaste)?;
        let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let (mut term_rx, term_stop) = Self::spawn_terminal_event_listener();
        let result = self.event_loop(&mut terminal, &mut term_rx).await;

        // Signal polling thread to stop, then restore terminal.
        term_stop.store(true, Ordering::Relaxed);

        // Restore terminal (always, even on error)
        let _ = terminal::disable_raw_mode();
        let _ = io::stdout().execute(DisableMouseCapture);
        let _ = io::stdout().execute(DisableBracketedPaste);
        let _ = io::stdout().execute(LeaveAlternateScreen);

        // Restore original panic hook.
        let _ = std::panic::take_hook();

        result
    }

    /// Spawn a dedicated crossterm event thread and return its receiver + stop flag.
    pub(super) fn spawn_terminal_event_listener() -> (mpsc::Receiver<Event>, Arc<AtomicBool>) {
        let (term_tx, term_rx) = mpsc::channel::<Event>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop = stop_flag.clone();
        tokio::task::spawn_blocking(move || {
            while !stop.load(Ordering::Relaxed) {
                if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    if let Ok(ev) = event::read() {
                        if term_tx.blocking_send(ev).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        (term_rx, stop_flag)
    }

    pub(super) async fn event_loop(
        &mut self,
        terminal: &mut Terminal<impl Backend>,
        term_rx: &mut mpsc::Receiver<Event>,
    ) -> io::Result<()> {
        use oxicode_common::PermissionResponse;

        // Draw the initial frame before entering the event loop.
        self.draw(terminal)?;

        loop {
            if self.should_quit {
                break;
            }

            // Dynamic tick: 50ms during active streaming for smooth spinner,
            // 100ms when idle (notifications, permission countdown).
            // Also tick while the agent-generator is in flight so its spinner animates.
            let agent_generating = self.agent_gen_cancel.is_some();
            let tick_ms = if self.is_turn_active { 50 } else { 100 };
            let needs_tick = self.is_turn_active
                || !self.notifications.is_empty()
                || self.pending_permission.is_some()
                || agent_generating;

            tokio::select! {
                Some(ev) = term_rx.recv() => {
                    match ev {
                        Event::Key(key) => self.handle_key(key).await,
                        Event::Mouse(mouse) => self.handle_mouse(mouse),
                        Event::Paste(text) => self.handle_paste(&text),
                        _ => {}
                    }
                    self.draw(terminal)?;
                }
                result = self.core_rx.recv() => {
                    if let Some(core_event) = result {
                        self.handle_core_event(core_event);
                        // Drain all pending core events before redrawing (batch updates).
                        while let Ok(ev) = self.core_rx.try_recv() {
                            self.handle_core_event(ev);
                        }
                        // Auto-send next queued message after turn completes.
                        self.drain_next_queued_message().await;
                        self.draw(terminal)?;
                    } else {
                        // Engine channel closed — quit gracefully.
                        self.should_quit = true;
                    }
                }
                Some(msg) = self.agent_gen_rx.recv() => {
                    self.handle_agent_generate_msg(msg);
                    self.draw(terminal)?;
                }
                // Tick for spinner animation, notification expiry, and permission countdown.
                () = tokio::time::sleep(Duration::from_millis(tick_ms)), if needs_tick => {
                    // Auto-deny permission if countdown expired (30s).
                    if let Some(ref perm) = self.pending_permission {
                        if perm.created_at.elapsed().as_secs() >= 30 {
                            if let Some(perm) = self.pending_permission.take() {
                                let _ = perm.reply_tx.send(PermissionResponse::Deny);
                            }
                            self.notifications.push(crate::widgets::Notification::new(
                                "Permission auto-denied (timeout)".to_string(),
                                crate::widgets::notification::NotificationLevel::Warning,
                            ));
                        }
                    }
                    self.draw(terminal)?;
                }
            }
        }

        Ok(())
    }
}
