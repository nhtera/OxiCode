/// Events flowing from TUI to the core engine.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// User submitted input text.
    UserInput(String),
    /// User requested quit.
    Quit,
    /// Terminal resized.
    Resize(u16, u16),
    /// Scroll up in message view.
    ScrollUp,
    /// Scroll down in message view.
    ScrollDown,
}

/// Events flowing from core engine to TUI for rendering.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// New text delta from streaming response.
    TextDelta(String),
    /// Streaming started.
    StreamStart,
    /// Streaming completed.
    StreamEnd,
    /// Error to display to user.
    Error(String),
    /// New assistant message completed.
    MessageComplete,
}
