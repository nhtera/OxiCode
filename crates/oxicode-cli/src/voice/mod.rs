//! Voice input module — microphone capture + Whisper STT.
//!
//! Feature-gated behind `--features voice`. Provides:
//! - Audio capture via `cpal` crate
//! - Whisper API transcription
//! - Voice activity detection (simple amplitude threshold)
//! - State machine: Off -> Listening -> Processing -> Off

#[cfg(feature = "voice")]
pub mod audio_capture;
#[cfg(feature = "voice")]
pub mod streaming;
pub mod whisper;

use tokio::sync::mpsc;

/// Voice mode state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    /// Voice input disabled.
    Off,
    /// Actively listening via microphone.
    Listening,
    /// Processing audio through Whisper API.
    Processing,
}

impl VoiceState {
    /// Human-readable label for TUI status bar.
    pub fn label(&self) -> &str {
        match self {
            Self::Off => "off",
            Self::Listening => "listening",
            Self::Processing => "processing",
        }
    }

    /// Whether voice mode is active (listening or processing).
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Handle to a running voice session — controls the capture + transcription pipeline.
pub struct VoiceSession {
    state: VoiceState,
    /// Channel to send transcribed text back to the TUI input.
    #[allow(dead_code)] // Used by start() when voice feature is compiled
    transcript_tx: mpsc::Sender<String>,
    /// Channel to signal the capture task to stop.
    stop_tx: Option<mpsc::Sender<()>>,
}

impl VoiceSession {
    /// Create a new voice session. `transcript_tx` receives transcribed text.
    pub fn new(transcript_tx: mpsc::Sender<String>) -> Self {
        Self {
            state: VoiceState::Off,
            transcript_tx,
            stop_tx: None,
        }
    }

    /// Current voice state.
    pub fn state(&self) -> VoiceState {
        self.state
    }

    /// Start voice capture. Returns error if no microphone or missing API key.
    #[cfg(feature = "voice")]
    pub fn start(&mut self) -> Result<(), String> {
        if self.state.is_active() {
            return Err("Voice already active".to_string());
        }

        // Check for OPENAI_API_KEY before starting capture.
        if std::env::var("OPENAI_API_KEY").is_err() {
            return Err(
                "OPENAI_API_KEY not set. Required for Whisper transcription.".to_string(),
            );
        }

        let (stop_tx, stop_rx) = mpsc::channel::<()>(1);
        let transcript_tx = self.transcript_tx.clone();

        // Spawn the streaming pipeline as a background task.
        tokio::spawn(async move {
            if let Err(e) = streaming::run_voice_pipeline(transcript_tx, stop_rx).await {
                tracing::error!("Voice pipeline error: {e}");
            }
        });

        self.stop_tx = Some(stop_tx);
        self.state = VoiceState::Listening;
        Ok(())
    }

    /// Start voice capture (no-op stub when feature not compiled).
    #[cfg(not(feature = "voice"))]
    pub fn start(&mut self) -> Result<(), String> {
        Err("Voice input not available. Compile with: cargo build --features voice".to_string())
    }

    /// Stop voice capture and return to Off state.
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            // Signal the capture task to stop (drop will also work).
            let _ = tx.try_send(());
        }
        self.state = VoiceState::Off;
    }

    /// Set state to Processing (called when sending audio to Whisper).
    pub fn set_processing(&mut self) {
        if self.state == VoiceState::Listening {
            self.state = VoiceState::Processing;
        }
    }

    /// Set state back to Listening (called after Whisper returns).
    pub fn set_listening(&mut self) {
        if self.state == VoiceState::Processing {
            self.state = VoiceState::Listening;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_state_labels() {
        assert_eq!(VoiceState::Off.label(), "off");
        assert_eq!(VoiceState::Listening.label(), "listening");
        assert_eq!(VoiceState::Processing.label(), "processing");
    }

    #[test]
    fn test_voice_state_is_active() {
        assert!(!VoiceState::Off.is_active());
        assert!(VoiceState::Listening.is_active());
        assert!(VoiceState::Processing.is_active());
    }

    #[test]
    fn test_voice_session_state_transitions() {
        let (tx, _rx) = mpsc::channel(10);
        let mut session = VoiceSession::new(tx);

        assert_eq!(session.state(), VoiceState::Off);
        assert!(!session.state().is_active());

        // Simulate state transitions (without real audio).
        session.state = VoiceState::Listening;
        assert_eq!(session.state(), VoiceState::Listening);

        session.set_processing();
        assert_eq!(session.state(), VoiceState::Processing);

        session.set_listening();
        assert_eq!(session.state(), VoiceState::Listening);

        session.stop();
        assert_eq!(session.state(), VoiceState::Off);
    }

    #[test]
    fn test_voice_session_stop_when_off_is_noop() {
        let (tx, _rx) = mpsc::channel(10);
        let mut session = VoiceSession::new(tx);
        session.stop(); // Should not panic.
        assert_eq!(session.state(), VoiceState::Off);
    }

    #[test]
    fn test_set_processing_only_from_listening() {
        let (tx, _rx) = mpsc::channel(10);
        let mut session = VoiceSession::new(tx);
        // Off -> set_processing should stay Off.
        session.set_processing();
        assert_eq!(session.state(), VoiceState::Off);
    }

    #[test]
    fn test_set_listening_only_from_processing() {
        let (tx, _rx) = mpsc::channel(10);
        let mut session = VoiceSession::new(tx);
        // Off -> set_listening should stay Off.
        session.set_listening();
        assert_eq!(session.state(), VoiceState::Off);
    }
}
