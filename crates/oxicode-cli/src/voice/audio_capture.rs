//! Audio capture via `cpal` crate — microphone input at 16kHz mono.
//!
//! Provides a ring buffer that accumulates audio samples and can be drained
//! in chunks for Whisper API transcription.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream, StreamConfig};

/// Maximum chunk duration in seconds (Whisper limit).
const MAX_CHUNK_SECS: usize = 30;
/// Target sample rate for Whisper API.
const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Audio capture handle — wraps a `cpal` input stream and ring buffer.
pub struct AudioCapture {
    /// Accumulated audio samples (16kHz mono f32).
    buffer: Arc<Mutex<Vec<f32>>>,
    /// Active `cpal` stream (dropped on stop).
    _stream: Stream,
}

impl AudioCapture {
    /// Start capturing audio from the default input device.
    ///
    /// Returns an `AudioCapture` handle or an error if no microphone is found.
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No microphone found. Check audio input settings.".to_string())?;

        let config = StreamConfig {
            channels: 1,
            sample_rate: SampleRate(TARGET_SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(
            TARGET_SAMPLE_RATE as usize * MAX_CHUNK_SECS,
        )));
        let buffer_clone = buffer.clone();

        let err_fn = |err: cpal::StreamError| {
            tracing::error!("Audio capture error: {err}");
        };

        // Determine supported format and build stream accordingly.
        let supported = device
            .default_input_config()
            .map_err(|e| format!("No supported audio config: {e}"))?;

        let stream = match supported.sample_format() {
            SampleFormat::F32 => device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            let max_samples = TARGET_SAMPLE_RATE as usize * MAX_CHUNK_SECS;
                            let remaining = max_samples.saturating_sub(buf.len());
                            let take = data.len().min(remaining);
                            buf.extend_from_slice(&data[..take]);
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build audio stream: {e}"))?,
            SampleFormat::I16 => device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            let max_samples = TARGET_SAMPLE_RATE as usize * MAX_CHUNK_SECS;
                            let remaining = max_samples.saturating_sub(buf.len());
                            let take = data.len().min(remaining);
                            for &sample in &data[..take] {
                                buf.push(f32::from(sample) / f32::from(i16::MAX));
                            }
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build audio stream (i16): {e}"))?,
            other => {
                return Err(format!("Unsupported audio sample format: {other:?}"));
            }
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start audio stream: {e}"))?;

        Ok(Self {
            buffer,
            _stream: stream,
        })
    }

    /// Drain the accumulated audio buffer, returning all samples and clearing it.
    pub fn drain_buffer(&self) -> Vec<f32> {
        if let Ok(mut buf) = self.buffer.lock() {
            std::mem::take(&mut *buf)
        } else {
            Vec::new()
        }
    }

    /// Current buffer length in samples.
    pub fn buffer_len(&self) -> usize {
        self.buffer.lock().map_or(0, |b| b.len())
    }

    /// Check if buffer has enough audio for a meaningful transcription (>0.5s).
    pub fn has_enough_audio(&self) -> bool {
        self.buffer_len() > TARGET_SAMPLE_RATE as usize / 2
    }

    /// Compute RMS amplitude of current buffer (for voice activity detection).
    pub fn rms_amplitude(&self) -> f32 {
        if let Ok(buf) = self.buffer.lock() {
            if buf.is_empty() {
                return 0.0;
            }
            let sum_sq: f32 = buf.iter().map(|s| s * s).sum();
            (sum_sq / buf.len() as f32).sqrt()
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(TARGET_SAMPLE_RATE, 16_000);
        assert_eq!(MAX_CHUNK_SECS, 30);
    }
}
