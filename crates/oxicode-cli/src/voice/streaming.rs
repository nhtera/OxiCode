//! Voice streaming pipeline — capture audio, detect voice activity, transcribe.
//!
//! Runs as a background task: captures audio via `cpal`, monitors for speech
//! using simple amplitude-based VAD, and sends chunks to Whisper API.

use tokio::sync::mpsc;

use super::audio_capture::AudioCapture;
use super::whisper;

/// Amplitude threshold for voice activity detection.
/// Samples below this RMS are considered silence.
const VAD_THRESHOLD: f32 = 0.02;

/// Minimum audio duration (seconds) before sending to Whisper.
const MIN_CHUNK_SECS: f32 = 1.0;

/// Maximum silence duration (seconds) before ending a speech segment.
const SILENCE_TIMEOUT_MS: u64 = 1500;

/// Poll interval for checking audio buffer state.
const POLL_INTERVAL_MS: u64 = 200;

/// Run the voice capture + transcription pipeline.
///
/// Captures audio, detects speech segments via VAD, sends to Whisper,
/// and forwards transcribed text through `transcript_tx`.
/// Stops when `stop_rx` receives a signal or is dropped.
pub async fn run_voice_pipeline(
    transcript_tx: mpsc::Sender<String>,
    mut stop_rx: mpsc::Receiver<()>,
) -> Result<(), String> {
    let capture = AudioCapture::start()?;
    tracing::info!("Voice capture started");

    let mut silence_counter: u64 = 0;
    let mut was_speaking = false;

    loop {
        // Check for stop signal (non-blocking).
        if stop_rx.try_recv().is_ok() {
            tracing::info!("Voice pipeline stop requested");
            break;
        }

        let rms = capture.rms_amplitude();
        let is_speaking = rms > VAD_THRESHOLD;

        if is_speaking {
            silence_counter = 0;
            was_speaking = true;
        } else if was_speaking {
            silence_counter += POLL_INTERVAL_MS;
        }

        // End of speech segment: silence timeout reached after speech detected.
        let buffer_secs = capture.buffer_len() as f32 / 16_000.0;
        let should_transcribe =
            was_speaking && silence_counter >= SILENCE_TIMEOUT_MS && buffer_secs >= MIN_CHUNK_SECS;

        // Also transcribe if buffer is getting full (approaching 30s limit).
        let buffer_full = buffer_secs >= 28.0;

        if should_transcribe || buffer_full {
            let samples = capture.drain_buffer();
            if !samples.is_empty() {
                tracing::debug!(
                    samples = samples.len(),
                    duration_secs = buffer_secs,
                    "Sending audio chunk to Whisper"
                );

                match whisper::transcribe(&samples).await {
                    Ok(result) if !result.text.is_empty() => {
                        tracing::info!(
                            text = %result.text,
                            duration = result.duration_secs,
                            "Transcription received"
                        );
                        if transcript_tx.send(result.text).await.is_err() {
                            tracing::warn!("Transcript receiver dropped");
                            break;
                        }
                    }
                    Ok(_) => {
                        tracing::debug!("Empty transcription (silence or noise)");
                    }
                    Err(e) => {
                        tracing::error!("Whisper transcription failed: {e}");
                        // Don't break — keep trying on next chunk.
                    }
                }

                was_speaking = false;
                silence_counter = 0;
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
    }

    tracing::info!("Voice pipeline stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vad_threshold_reasonable() {
        // VAD threshold should be positive and small.
        assert!(VAD_THRESHOLD > 0.0);
        assert!(VAD_THRESHOLD < 0.1);
    }

    #[test]
    fn test_silence_timeout_reasonable() {
        // Silence timeout should be 1-5 seconds.
        assert!(SILENCE_TIMEOUT_MS >= 500);
        assert!(SILENCE_TIMEOUT_MS <= 5000);
    }

    #[test]
    fn test_min_chunk_duration() {
        assert!(MIN_CHUNK_SECS >= 0.5);
        assert!(MIN_CHUNK_SECS <= 5.0);
    }
}
