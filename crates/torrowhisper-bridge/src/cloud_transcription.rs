//! Cloud speech-to-text over the OpenAI `/audio/transcriptions` endpoint.
//!
//! Audio leaves the Mac here by design: the user picked a cloud model in the
//! language-model settings. A failed request is reported as an error and is
//! never retried locally, so the user always knows where their audio went.
//!
//! The multipart body is assembled by hand (instead of reqwest's `multipart`
//! feature) to avoid a new dependency for three text fields and one file.

use std::time::Duration;

use serde_json::Value;
use torrowhisper_core::{CloudTranscriptionModel, CloudTranscriptionProvider};

use crate::llm::{USER_AGENT, build_http_client_with_timeout, keychain};

/// Upper bound for one upload plus transcription. A minute of 16 kHz mono WAV
/// is about 1.9 MB, so this is generous for dictation-sized takes.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(60);

/// Dictionary terms passed as the `prompt` field. Whisper-family models read
/// roughly the first 224 tokens of a prompt; 800 characters stays well inside.
const PROMPT_MAX_CHARS: usize = 800;

const BOUNDARY: &str = "torrowhisper-boundary-7f3a9c2e";

/// Per-request options that come from the app settings, not from the audio.
pub(crate) struct TranscriptionOptions<'a> {
    /// ISO-639-1 code such as `de`. `None` lets the provider detect the language.
    pub language: Option<&'a str>,
    /// Vocabulary hint (dictionary replacement targets). Newlines are flattened.
    pub prompt: Option<&'a str>,
}

/// Transcribes 16 kHz mono `samples` with the selected cloud model.
pub(crate) fn transcribe(
    model: &CloudTranscriptionModel,
    samples_16khz: &[f32],
    options: &TranscriptionOptions<'_>,
) -> Result<String, String> {
    let api_key = keychain::get_api_key(model.provider.llm_backend()).ok_or_else(|| {
        format!(
            "No {} API key is stored. Add one in the language model settings.",
            model.provider.label()
        )
    })?;
    transcribe_with_key(&api_key, model, samples_16khz, options)
}

/// Same as [`transcribe`] but with an explicit key, so tests can run against the
/// live API without touching the Keychain.
pub(crate) fn transcribe_with_key(
    api_key: &str,
    model: &CloudTranscriptionModel,
    samples_16khz: &[f32],
    options: &TranscriptionOptions<'_>,
) -> Result<String, String> {
    let wav = encode_wav_16k_mono(samples_16khz);
    let body = multipart_body(&model.model_name, options, &wav);
    let client = build_http_client_with_timeout(TRANSCRIBE_TIMEOUT)?;
    let url = format!("{}/audio/transcriptions", base_url(model.provider));

    let response = client
        .post(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .bearer_auth(api_key)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(body)
        .send()
        .map_err(|_| "Cloud transcription is not reachable right now.".to_owned())?;

    let status = response.status();
    let response_body = response
        .text()
        .map_err(|_| "Cloud transcription response could not be read.".to_owned())?;
    if !status.is_success() {
        return Err(describe_http_error(status.as_u16()));
    }
    parse_transcript(&response_body)
}

/// Base URL without the trailing `/audio/transcriptions`.
fn base_url(provider: CloudTranscriptionProvider) -> &'static str {
    match provider {
        CloudTranscriptionProvider::OpenAi => "https://api.openai.com/v1",
    }
}

/// Wraps 16 kHz mono float samples in a canonical 44-byte WAV header with
/// 16-bit PCM. Samples outside [-1, 1] are clipped.
pub(crate) fn encode_wav_16k_mono(samples: &[f32]) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 16_000;
    const CHANNELS: u16 = 1;
    const BITS_PER_SAMPLE: u16 = 16;
    let block_align = CHANNELS * BITS_PER_SAMPLE / 8;
    let data_len = (samples.len() * usize::from(block_align)) as u32;

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // audio format: PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn multipart_body(model_name: &str, options: &TranscriptionOptions<'_>, wav: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav.len() + 512);
    push_text_field(&mut body, "model", model_name);
    push_text_field(&mut body, "response_format", "json");
    if let Some(language) = options.language {
        push_text_field(&mut body, "language", language);
    }
    if let Some(prompt) = options.prompt {
        let flat = truncate_chars(&prompt.replace(['\r', '\n'], " "), PROMPT_MAX_CHARS);
        if !flat.trim().is_empty() {
            push_text_field(&mut body, "prompt", flat.trim());
        }
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

fn push_text_field(body: &mut Vec<u8>, name: &str, value: &str) {
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        )
        .as_bytes(),
    );
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Extracts the transcript from a `{"text": "..."}` response. An empty result
/// is an error, matching the local engines' "no text" behaviour.
pub(crate) fn parse_transcript(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "Cloud transcription returned an unreadable response.".to_owned())?;
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if text.is_empty() {
        Err("Cloud transcription recognized no text.".to_owned())
    } else {
        Ok(text)
    }
}

/// Maps an HTTP status from the provider to a user-facing message. The response
/// body is deliberately not included: it can echo request details.
pub(crate) fn describe_http_error(status: u16) -> String {
    match status {
        401 | 403 => "The OpenAI API key is invalid or missing.".to_owned(),
        429 => "OpenAI rate limit reached. Try again later.".to_owned(),
        400..=499 => format!("OpenAI rejected the transcription request (HTTP {status})."),
        _ => format!("Cloud transcription is unavailable right now (HTTP {status})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_describes_16khz_mono_pcm16() {
        let wav = encode_wav_16k_mono(&[0.0, 0.5, -0.5]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 6);
    }

    #[test]
    fn wav_clips_out_of_range_samples() {
        let wav = encode_wav_16k_mono(&[2.0, -2.0]);
        assert_eq!(i16::from_le_bytes([wav[44], wav[45]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), -i16::MAX);
    }

    #[test]
    fn multipart_contains_model_language_and_file() {
        let options = TranscriptionOptions {
            language: Some("de"),
            prompt: None,
        };
        let body = multipart_body("gpt-4o-transcribe", &options, &[1, 2, 3]);
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"model\"\r\n\r\ngpt-4o-transcribe"));
        assert!(text.contains("name=\"language\"\r\n\r\nde"));
        assert!(text.contains("name=\"response_format\"\r\n\r\njson"));
        assert!(text.contains("name=\"file\"; filename=\"audio.wav\""));
        assert!(!text.contains("name=\"prompt\""));
        assert!(text.ends_with(&format!("--{BOUNDARY}--\r\n")));
    }

    #[test]
    fn auto_language_omits_language_field() {
        let options = TranscriptionOptions {
            language: None,
            prompt: None,
        };
        let body = multipart_body("whisper-1", &options, &[]);
        assert!(!String::from_utf8_lossy(&body).contains("name=\"language\""));
    }

    #[test]
    fn prompt_is_flattened_and_truncated() {
        let long = format!("Kubernetes\n{}", "x".repeat(2_000));
        let options = TranscriptionOptions {
            language: None,
            prompt: Some(&long),
        };
        let body =
            String::from_utf8_lossy(&multipart_body("whisper-1", &options, &[])).into_owned();
        let start = body.find("name=\"prompt\"\r\n\r\n").unwrap() + "name=\"prompt\"\r\n\r\n".len();
        let value = body[start..].split("\r\n").next().unwrap();
        assert!(value.starts_with("Kubernetes x"));
        assert_eq!(value.chars().count(), PROMPT_MAX_CHARS);
    }

    #[test]
    fn parses_text_and_rejects_empty() {
        assert_eq!(
            parse_transcript(r#"{"text":" Hallo Welt "}"#).unwrap(),
            "Hallo Welt"
        );
        assert!(parse_transcript(r#"{"text":"  "}"#).is_err());
        assert!(parse_transcript(r#"{"error":{}}"#).is_err());
        assert!(parse_transcript("not json").is_err());
    }

    #[test]
    fn maps_http_statuses_without_leaking_body() {
        assert!(describe_http_error(401).contains("invalid or missing"));
        assert!(describe_http_error(429).contains("rate limit"));
        assert!(describe_http_error(400).contains("HTTP 400"));
        assert!(describe_http_error(503).contains("HTTP 503"));
    }

    /// Live check against the OpenAI API. Needs a real key and network, so it is
    /// ignored by default. Run with:
    /// `OPENAI_API_KEY=... cargo test -p torrowhisper-bridge live_openai -- --ignored --nocapture`
    #[test]
    #[ignore = "calls the OpenAI API; needs OPENAI_API_KEY"]
    fn live_openai_transcribes_spoken_sample() {
        let key = std::env::var("OPENAI_API_KEY").expect("set OPENAI_API_KEY");
        let samples = synthesize_speech_16khz("Dies ist ein Test der Cloud Transkription.");
        let options = TranscriptionOptions {
            language: Some("de"),
            prompt: Some("TorroWhisper, Parakeet"),
        };
        for model_name in ["gpt-4o-transcribe", "gpt-4o-mini-transcribe", "whisper-1"] {
            let model = CloudTranscriptionModel {
                provider: CloudTranscriptionProvider::OpenAi,
                model_name: model_name.to_owned(),
            };
            let text = transcribe_with_key(&key, &model, &samples, &options)
                .unwrap_or_else(|err| panic!("{model_name} failed: {err}"));
            println!("{model_name}: {text}");
            assert!(
                text.to_lowercase().contains("test"),
                "{model_name} returned {text:?}"
            );
        }
    }

    /// Generates speech with macOS `say` and converts it to 16 kHz mono PCM16
    /// with `afconvert`, then reads the samples back as f32.
    fn synthesize_speech_16khz(text: &str) -> Vec<f32> {
        let dir = std::env::temp_dir().join("torrowhisper-cloud-test");
        std::fs::create_dir_all(&dir).unwrap();
        let aiff = dir.join("speech.aiff");
        let wav = dir.join("speech.wav");
        let status = std::process::Command::new("say")
            .args(["-o"])
            .arg(&aiff)
            .arg(text)
            .status()
            .unwrap();
        assert!(status.success(), "say failed");
        let status = std::process::Command::new("afconvert")
            .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
            .arg(&aiff)
            .arg(&wav)
            .status()
            .unwrap();
        assert!(status.success(), "afconvert failed");
        let bytes = std::fs::read(&wav).unwrap();
        let data_at = bytes
            .windows(4)
            .position(|window| window == b"data")
            .expect("data chunk");
        bytes[data_at + 8..]
            .chunks_exact(2)
            .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / f32::from(i16::MAX))
            .collect()
    }
}
