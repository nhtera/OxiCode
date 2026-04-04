//! Whisper API client — transcribe audio via OpenAI's speech-to-text API.
//!
//! Sends WAV-encoded audio to `POST /v1/audio/transcriptions` and returns text.
//! Requires `OPENAI_API_KEY` environment variable.

use reqwest::multipart;

/// OpenAI Whisper transcription endpoint.
const WHISPER_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

/// Transcription result from Whisper API.
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    /// Transcribed text.
    pub text: String,
    /// Duration of the audio in seconds (approximate).
    pub duration_secs: f32,
}

/// Transcribe audio samples (16kHz mono f32) via Whisper API.
///
/// Encodes samples as WAV, uploads to Whisper, returns transcribed text.
/// Returns `Err` if `OPENAI_API_KEY` is not set or the API call fails.
pub async fn transcribe(samples: &[f32]) -> Result<TranscriptionResult, String> {
    let api_key =
        std::env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;

    if samples.is_empty() {
        return Err("No audio samples to transcribe".to_string());
    }

    #[allow(clippy::cast_precision_loss)]
    let duration_secs = samples.len() as f32 / 16_000.0;
    let wav_data = encode_wav_16khz_mono(samples);

    let part = multipart::Part::bytes(wav_data)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("Failed to create multipart: {e}"))?;

    let form = multipart::Form::new()
        .text("model", "whisper-1")
        .text("response_format", "json")
        .part("file", part);

    let client = reqwest::Client::new();
    let resp = client
        .post(WHISPER_URL)
        .bearer_auth(&api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Whisper API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_else(|_| "unknown".to_string());
        return Err(format!("Whisper API error ({status}): {body}"));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Whisper response: {e}"))?;

    let text = json["text"].as_str().unwrap_or("").trim().to_string();

    Ok(TranscriptionResult {
        text,
        duration_secs,
    })
}

/// Encode f32 samples as 16-bit PCM WAV (16kHz, mono).
fn encode_wav_16khz_mono(samples: &[f32]) -> Vec<u8> {
    let sample_rate: u32 = 16_000;
    let bits_per_sample: u16 = 16;
    let channels: u16 = 1;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_size = (samples.len() * 2) as u32;
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + samples.len() * 2);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt sub-chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // sub-chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data sub-chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    // Convert f32 [-1.0, 1.0] to i16.
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_val = (clamped * f32::from(i16::MAX)) as i16;
        buf.extend_from_slice(&i16_val.to_le_bytes());
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_wav_header() {
        let samples = vec![0.0f32; 16_000]; // 1 second of silence
        let wav = encode_wav_16khz_mono(&samples);

        // Check RIFF header.
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");

        // Data starts at byte 44.
        assert_eq!(&wav[36..40], b"data");

        // Total size: 44 header + 16000 samples * 2 bytes = 32044.
        assert_eq!(wav.len(), 44 + 16_000 * 2);
    }

    #[test]
    fn test_encode_wav_samples() {
        let samples = vec![0.5, -0.5, 1.0, -1.0, 0.0];
        let wav = encode_wav_16khz_mono(&samples);

        // Data bytes start at offset 44.
        let data = &wav[44..];
        assert_eq!(data.len(), 10); // 5 samples * 2 bytes

        // First sample: 0.5 * 32767 ~ 16383
        let s0 = i16::from_le_bytes([data[0], data[1]]);
        assert!((s0 - 16383).abs() <= 1);

        // Second sample: -0.5 * 32767 ~ -16383
        let s1 = i16::from_le_bytes([data[2], data[3]]);
        assert!((s1 + 16383).abs() <= 1);
    }

    #[test]
    fn test_transcription_result() {
        let result = TranscriptionResult {
            text: "hello world".to_string(),
            duration_secs: 2.5,
        };
        assert_eq!(result.text, "hello world");
        assert!((result.duration_secs - 2.5).abs() < f32::EPSILON);
    }
}
