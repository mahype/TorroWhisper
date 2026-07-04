//! Local text-to-speech for the chat plugin (#17).
//!
//! A single backend: **Piper** (offline neural voice) run via the prebuilt
//! `sherpa-onnx` CLI as a *subprocess*. Subprocess isolation deliberately
//! mirrors the local-LLM helper — it keeps sherpa's onnxruntime out of the main
//! library, avoiding any native-symbol clash with the whisper/llama ggml stack.
//! The CLI + the selected voice model are downloaded once into the app data dir.
//! Cloud TTS was removed in favour of a fast, fully local pipeline.

use std::{fs, path::PathBuf, process::Command, time::Duration};

use directories::ProjectDirs;

use crate::llm;

/// Normalized 0–1 rate → speech `speed` (0.25–4.0), centered so 0.5 ≈ 1.0×.
fn speed_for(rate: f32) -> f64 {
    let clamped = rate.clamp(0.0, 1.0);
    if clamped <= 0.5 {
        f64::from(0.5 + clamped)
    } else {
        f64::from(1.0 + (clamped - 0.5) * 2.0)
    }
}

// --- Local Piper TTS (sherpa-onnx subprocess) ---

/// Prebuilt sherpa-onnx version + macOS/arm64 CLI bundle (binary + dylibs).
const SHERPA_VERSION: &str = "1.13.2";
const SHERPA_CLI_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/sherpa-onnx-v1.13.2-osx-arm64-shared.tar.bz2";
/// Where the Piper voice tarballs live (dir `vits-piper-<id>`, model `<id>.onnx`).
const PIPER_MODELS_BASE: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/";
/// Default German voice when none is configured (defined in core, the single
/// source shared with `ChatTtsSettings`).
pub const DEFAULT_PIPER_VOICE: &str = donny_core::DEFAULT_PIPER_VOICE;

/// Generous timeout for the one-time asset downloads (CLI ~25 MB, model ~110 MB).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// `~/Library/Application Support/com.getdonny.Donny/tts`.
fn tts_dir() -> Result<PathBuf, String> {
    ProjectDirs::from("com", "getdonny", "Donny")
        .map(|dirs| dirs.config_dir().join("tts"))
        .ok_or_else(|| "Could not resolve the app data directory for TTS.".to_owned())
}

/// Root of the extracted sherpa-onnx CLI bundle.
fn sherpa_root() -> Result<PathBuf, String> {
    Ok(tts_dir()?.join(format!("sherpa-onnx-v{SHERPA_VERSION}-osx-arm64-shared")))
}

/// Extracted model directory for a Piper voice id.
fn model_root(voice: &str) -> Result<PathBuf, String> {
    Ok(tts_dir()?.join(format!("vits-piper-{voice}")))
}

/// True once both the CLI and the given voice model are present on disk.
pub fn piper_ready(voice: &str) -> bool {
    let voice = normalize_voice(voice);
    matches!(
        (sherpa_root(), model_root(voice)),
        (Ok(cli), Ok(model)) if cli.join("bin/sherpa-onnx-offline-tts").exists()
            && model.join(format!("{voice}.onnx")).exists()
    )
}

/// Downloads + extracts the CLI and the requested voice if missing. Idempotent;
/// safe (if slow) to call before the first synthesis.
pub fn prepare_piper(voice: &str) -> Result<(), String> {
    let voice = normalize_voice(voice);
    let dir = tts_dir()?;
    fs::create_dir_all(&dir).map_err(|err| format!("Could not create the TTS directory: {err}"))?;

    let cli = sherpa_root()?;
    if !cli.join("bin/sherpa-onnx-offline-tts").exists() {
        download_and_extract(SHERPA_CLI_URL, &dir)?;
    }
    let model = model_root(voice)?;
    if !model.join(format!("{voice}.onnx")).exists() {
        let url = format!("{PIPER_MODELS_BASE}vits-piper-{voice}.tar.bz2");
        download_and_extract(&url, &dir)?;
    }
    Ok(())
}

/// Synthesizes `text` with a local Piper voice and returns WAV bytes. The text
/// is first normalized for speech (markdown/abbreviations/lists → spoken prose).
pub fn piper_speech(text: &str, voice: &str, rate: f32) -> Result<Vec<u8>, String> {
    let voice = normalize_voice(voice).to_owned();
    let text = crate::speech_text::normalize_for_speech(text);
    let text = text.as_str();
    prepare_piper(&voice)?;

    let cli = sherpa_root()?;
    let model = model_root(&voice)?;
    let bin = cli.join("bin/sherpa-onnx-offline-tts");
    let lib = cli.join("lib");
    let onnx = model.join(format!("{voice}.onnx"));
    let tokens = model.join("tokens.txt");
    let data_dir = model.join("espeak-ng-data");

    // Unique output path so concurrent turns never clobber each other.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let out = tts_dir()?.join(format!("synth-{nanos}.wav"));

    let output = Command::new(&bin)
        // Child is a plain (non-hardened) CLI, so it honours DYLD_LIBRARY_PATH to
        // find its bundled dylibs.
        .env("DYLD_LIBRARY_PATH", &lib)
        .arg(format!("--vits-model={}", onnx.display()))
        .arg(format!("--vits-tokens={}", tokens.display()))
        .arg(format!("--vits-data-dir={}", data_dir.display()))
        .arg(format!("--vits-length-scale={}", length_scale_for(rate)))
        .arg("--num-threads=4")
        .arg(format!("--output-filename={}", out.display()))
        .arg(text)
        .output()
        .map_err(|err| format!("Could not run the local TTS engine: {err}"))?;

    if !output.status.success() {
        let _ = fs::remove_file(&out);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let snippet: String = stderr.chars().take(300).collect();
        return Err(format!("Local TTS failed: {snippet}"));
    }

    let bytes =
        fs::read(&out).map_err(|err| format!("Local TTS audio could not be read: {err}"))?;
    let _ = fs::remove_file(&out);
    Ok(bytes)
}

/// `--vits-length-scale` is the *inverse* of speed (larger = slower), so invert
/// our 0–1 rate via the shared [`speed_for`] mapping (0.5 ≈ 1.0×).
fn length_scale_for(rate: f32) -> f64 {
    1.0 / speed_for(rate)
}

fn normalize_voice(voice: &str) -> &str {
    let trimmed = voice.trim();
    if trimmed.is_empty() {
        DEFAULT_PIPER_VOICE
    } else {
        trimmed
    }
}

/// Downloads a `.tar.bz2` and extracts it into `dest` using the system `tar`
/// (handles bzip2 on macOS, so no extra crates are needed).
fn download_and_extract(url: &str, dest: &std::path::Path) -> Result<(), String> {
    let client = llm::build_http_client_with_timeout(DOWNLOAD_TIMEOUT)?;
    let response = client
        .get(url)
        .send()
        .map_err(|err| format!("TTS asset download failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "TTS asset download returned HTTP {}.",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .map_err(|err| format!("TTS asset could not be read: {err}"))?;

    let tmp = dest.join("download.tar.bz2");
    fs::write(&tmp, &bytes).map_err(|err| format!("TTS asset could not be saved: {err}"))?;
    let status = Command::new("tar")
        .arg("xf")
        .arg(&tmp)
        .arg("-C")
        .arg(dest)
        .status()
        .map_err(|err| format!("Could not extract the TTS asset: {err}"))?;
    let _ = fs::remove_file(&tmp);
    if !status.success() {
        return Err("TTS asset extraction failed.".to_owned());
    }
    Ok(())
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
