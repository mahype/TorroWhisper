//! Cloud text-to-speech for the chat plugin (#17).
//!
//! Runs entirely in Rust so the API key is read the same reliable way as the
//! LLM cloud providers (straight from the Keychain) instead of from Swift — the
//! Swift-side Keychain read proved unreliable across ad-hoc-signed builds. The
//! synthesized audio (MP3) is handed back over the FFI and played by Swift.

use open_whisper_core::LlmBackendKind;
use serde_json::json;

use crate::llm;

const SPEECH_URL: &str = "https://api.openai.com/v1/audio/speech";
/// `tts-1` is broadly available and supports every voice + `speed` + `mp3`.
const SPEECH_MODEL: &str = "tts-1";

/// Synthesizes `text` with OpenAI's TTS and returns the MP3 bytes. Errors carry
/// the real reason (missing key / HTTP status + body) so failures are visible
/// in the log instead of a silent fallback.
pub fn openai_speech(text: &str, voice: &str, rate: f32) -> Result<Vec<u8>, String> {
    let key = llm::keychain::get_api_key(LlmBackendKind::OpenAi)
        .ok_or_else(|| "OpenAI API key is not configured (add it in Cloud models).".to_owned())?;
    let client = llm::build_http_client()?;
    let voice = if voice.trim().is_empty() {
        "alloy"
    } else {
        voice.trim()
    };

    let response = client
        .post(SPEECH_URL)
        .bearer_auth(&key)
        .json(&json!({
            "model": SPEECH_MODEL,
            "input": text,
            "voice": voice,
            "response_format": "mp3",
            "speed": speed_for(rate),
        }))
        .send()
        .map_err(|err| format!("OpenAI TTS request failed: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        let snippet: String = body.chars().take(300).collect();
        return Err(format!("OpenAI TTS returned HTTP {status}: {snippet}"));
    }

    let bytes = response
        .bytes()
        .map_err(|err| format!("OpenAI TTS audio could not be read: {err}"))?;
    Ok(bytes.to_vec())
}

/// Normalized 0–1 rate → OpenAI `speed` (0.25–4.0), centered so 0.5 ≈ 1.0×.
fn speed_for(rate: f32) -> f64 {
    let clamped = rate.clamp(0.0, 1.0);
    if clamped <= 0.5 {
        f64::from(0.5 + clamped)
    } else {
        f64::from(1.0 + (clamped - 0.5) * 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_is_centered_and_clamped() {
        assert!((speed_for(0.5) - 1.0).abs() < 1e-6); // middle → 1.0x
        assert!((speed_for(0.0) - 0.5).abs() < 1e-6); // slowest offered
        assert!((speed_for(1.0) - 2.0).abs() < 1e-6); // fastest offered
        assert!((speed_for(2.0) - 2.0).abs() < 1e-6); // out-of-range clamps
        assert!(speed_for(-1.0) >= 0.25); // never below OpenAI's floor
    }
}
