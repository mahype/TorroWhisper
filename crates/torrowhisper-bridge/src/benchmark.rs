//! Whisper model & thread-count benchmark (#43).
//!
//! Transcribes a fixed, embedded reference clip with each locally available
//! model (and, for the active model, across a sweep of thread counts) and
//! reports load time, inference time, real-time factor, memory footprint and a
//! rough quality score. The numbers let the user pick the fastest
//! model/threads for their hardware from measurements instead of assumptions
//! (the Q5_0 vs FP16 question in particular).
//!
//! This runs entirely off the shared `thread_local` runtime: it loads settings
//! from disk and its own fresh `WhisperContext`s, so the FFI entry point is
//! safe to call from a background thread while the app stays responsive.

use std::collections::HashMap;
use std::time::Instant;

use torrowhisper_core::{
    AppSettings, BenchmarkReportDto, BenchmarkRequestDto, BenchmarkRowDto, ModelPreset,
};
use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::dictation::{resample_to_16khz, resolve_thread_count, run_whisper_inference};
use crate::model_manager::{ModelIntegrity, default_model_path, preset_model_integrity};

/// Fixed reference clip (16 kHz mono, 16-bit PCM), generated offline with macOS
/// `say -v Anna` so every run transcribes identical audio and comparisons are
/// reproducible. Embedded into the binary so it behaves the same in dev and in
/// the bundled app (no runtime resource-path resolution).
const REFERENCE_WAV: &[u8] = include_bytes!("../resources/benchmark-de.wav");

/// Ground-truth transcript of [`REFERENCE_WAV`], for the rough quality metric.
const REFERENCE_TEXT: &str = "Das schnelle Diktieren funktioniert am besten, wenn die \
Spracherkennung zuverlässig und ohne Verzögerung arbeitet. Mit lokaler Verarbeitung bleiben \
alle Daten auf dem Gerät, und die Privatsphäre ist jederzeit gewährleistet.";

const REFERENCE_LANGUAGE: &str = "de";
const DEFAULT_THREAD_SWEEP: [u32; 5] = [1, 2, 4, 6, 8];

/// The headline models #43 asks to compare directly: quantized Q5_0 vs FP16
/// Turbo. Always reported (as "not downloaded" when absent) so the comparison
/// is never silently incomplete.
const HEADLINE_MODELS: [ModelPreset; 2] =
    [ModelPreset::LargeV3TurboQ5_0, ModelPreset::LargeV3Turbo];

/// Runs the benchmark and returns the per-run breakdown. `settings` supplies
/// the decoding options (single_segment etc.) and the active model for the
/// thread sweep; `request` can override the thread sweep.
pub fn run_benchmark(
    settings: &AppSettings,
    request: &BenchmarkRequestDto,
) -> Result<BenchmarkReportDto, String> {
    let (raw, sample_rate) = decode_reference_wav()?;
    let samples = resample_to_16khz(&raw, sample_rate);
    if samples.is_empty() {
        return Err("benchmark reference audio decoded to no samples".to_owned());
    }
    let audio_secs = samples.len() as f32 / 16_000.0;
    let reference_words = normalized_words(REFERENCE_TEXT);

    log::info!(
        target: "dictation",
        "benchmark starting: {audio_secs:.1}s reference audio, single_segment={}",
        settings.whisper_single_segment
    );

    let mut rows: Vec<BenchmarkRowDto> = Vec::new();

    // --- Model comparison at the auto thread count ------------------------
    let auto_threads = resolve_thread_count(0);
    for preset in model_candidates() {
        let available = matches!(preset_model_integrity(preset), ModelIntegrity::Valid);
        if !available {
            // Headline models are always listed so Q5_0/FP16 gaps are visible.
            if HEADLINE_MODELS.contains(&preset) {
                rows.push(unavailable_row("model", preset, auto_threads as u32));
            }
            continue;
        }
        match measure_run(
            preset,
            &samples,
            settings,
            auto_threads,
            audio_secs,
            &reference_words,
        ) {
            Ok((row, _context)) => rows.push(row),
            Err(err) => rows.push(error_row("model", preset, auto_threads as u32, err)),
        }
    }

    // --- Thread sweep on the active (or first available headline) model ---
    let sweep_counts: Vec<u32> = if request.thread_counts.is_empty() {
        DEFAULT_THREAD_SWEEP.to_vec()
    } else {
        request.thread_counts.clone()
    };
    if let Some(preset) = sweep_model(settings) {
        match load_context(preset) {
            Ok((context, load_secs, load_rss_mb)) => {
                for (index, &configured) in sweep_counts.iter().enumerate() {
                    let n_threads = resolve_thread_count(configured);
                    match run_whisper_inference(
                        &context,
                        &samples,
                        settings,
                        Some(REFERENCE_LANGUAGE),
                        n_threads,
                    ) {
                        Ok(inference) => {
                            let rtf = inference.inference_secs / audio_secs;
                            let quality = quality_score(&reference_words, &inference.text);
                            // Load cost is attributed to the first sweep row only
                            // (the context is reused across thread counts).
                            rows.push(BenchmarkRowDto {
                                kind: "threads".to_owned(),
                                model_label: preset.label().to_owned(),
                                model_available: true,
                                thread_count: configured,
                                load_secs: if index == 0 { load_secs } else { 0.0 },
                                inference_secs: inference.inference_secs,
                                real_time_factor: rtf,
                                load_rss_mb: if index == 0 { load_rss_mb } else { 0.0 },
                                quality_score: quality,
                                transcript: inference.text,
                                note: String::new(),
                            });
                        }
                        Err(err) => rows.push(error_row("threads", preset, configured, err)),
                    }
                }
            }
            Err(err) => rows.push(error_row("threads", preset, 0, err)),
        }
    }

    log::info!(target: "dictation", "benchmark finished: {} row(s)", rows.len());

    Ok(BenchmarkReportDto {
        audio_secs,
        reference_text: REFERENCE_TEXT.to_owned(),
        rows,
    })
}

/// Loads a model, runs one inference, and packs the result into a row. Returns
/// the context too so callers may reuse it (the thread sweep does not, but the
/// model comparison discards it to free memory before the next model).
fn measure_run(
    preset: ModelPreset,
    samples: &[f32],
    settings: &AppSettings,
    n_threads: i32,
    audio_secs: f32,
    reference_words: &[String],
) -> Result<(BenchmarkRowDto, WhisperContext), String> {
    let (context, load_secs, load_rss_mb) = load_context(preset)?;
    let inference = run_whisper_inference(
        &context,
        samples,
        settings,
        Some(REFERENCE_LANGUAGE),
        n_threads,
    )?;
    let rtf = inference.inference_secs / audio_secs;
    let quality = quality_score(reference_words, &inference.text);
    let row = BenchmarkRowDto {
        kind: "model".to_owned(),
        model_label: preset.display_label().to_owned(),
        model_available: true,
        thread_count: n_threads as u32,
        load_secs,
        inference_secs: inference.inference_secs,
        real_time_factor: rtf,
        load_rss_mb,
        quality_score: quality,
        transcript: inference.text,
        note: String::new(),
    };
    Ok((row, context))
}

/// Loads a fresh `WhisperContext` for `preset`, measuring load time and the
/// resident-memory increase it caused.
fn load_context(preset: ModelPreset) -> Result<(WhisperContext, f32, f32), String> {
    let path = default_model_path(preset)?;
    let path_str = path.to_string_lossy().to_string();
    let rss_before = current_rss_mb();
    let started = Instant::now();
    let context = WhisperContext::new_with_params(&path_str, WhisperContextParameters::default())
        .map_err(|err| format!("model could not be loaded: {err}"))?;
    let load_secs = started.elapsed().as_secs_f32();
    let load_rss_mb = current_rss_mb().map_or(0.0, |after| {
        rss_before.map_or(0.0, |before| (after - before).max(0.0))
    });
    Ok((context, load_secs, load_rss_mb))
}

fn current_rss_mb() -> Option<f32> {
    crate::diagnostics::process_stats().map(|stats| stats.resident_bytes as f32 / (1024.0 * 1024.0))
}

/// Headline models first, then the remaining presets — deduplicated.
fn model_candidates() -> Vec<ModelPreset> {
    let mut candidates: Vec<ModelPreset> = HEADLINE_MODELS.to_vec();
    for preset in ModelPreset::ALL {
        if !candidates.contains(&preset) {
            candidates.push(preset);
        }
    }
    candidates
}

/// The model to sweep thread counts on: the active model when downloaded, else
/// the first available headline model, else nothing.
fn sweep_model(settings: &AppSettings) -> Option<ModelPreset> {
    if matches!(
        preset_model_integrity(settings.local_model),
        ModelIntegrity::Valid
    ) {
        return Some(settings.local_model);
    }
    HEADLINE_MODELS
        .into_iter()
        .find(|&preset| matches!(preset_model_integrity(preset), ModelIntegrity::Valid))
}

fn unavailable_row(kind: &str, preset: ModelPreset, thread_count: u32) -> BenchmarkRowDto {
    BenchmarkRowDto {
        kind: kind.to_owned(),
        model_label: preset.display_label().to_owned(),
        model_available: false,
        thread_count,
        load_secs: 0.0,
        inference_secs: 0.0,
        real_time_factor: 0.0,
        load_rss_mb: 0.0,
        quality_score: 0.0,
        transcript: String::new(),
        note: "not downloaded — skipped".to_owned(),
    }
}

fn error_row(kind: &str, preset: ModelPreset, thread_count: u32, err: String) -> BenchmarkRowDto {
    BenchmarkRowDto {
        kind: kind.to_owned(),
        model_label: preset.display_label().to_owned(),
        model_available: true,
        thread_count,
        load_secs: 0.0,
        inference_secs: 0.0,
        real_time_factor: 0.0,
        load_rss_mb: 0.0,
        quality_score: 0.0,
        transcript: String::new(),
        note: err,
    }
}

/// Fraction of reference words recognised, as a rough (word-bag) quality
/// signal. Not a WER — it ignores order and insertions — but enough to spot a
/// model that mangles the text versus one that reproduces it.
fn quality_score(reference_words: &[String], hypothesis: &str) -> f32 {
    if reference_words.is_empty() {
        return 0.0;
    }
    let mut counts: HashMap<String, i32> = HashMap::new();
    for word in normalized_words(hypothesis) {
        *counts.entry(word).or_insert(0) += 1;
    }
    let mut matched = 0usize;
    for word in reference_words {
        if let Some(remaining) = counts.get_mut(word)
            && *remaining > 0
        {
            *remaining -= 1;
            matched += 1;
        }
    }
    matched as f32 / reference_words.len() as f32
}

fn normalized_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Minimal PCM-WAV decoder for the embedded reference: scans RIFF chunks for
/// `fmt ` + `data`, requires 16-bit PCM, and downmixes to mono. Kept
/// self-contained to avoid adding a WAV crate for one fixed asset.
fn decode_reference_wav() -> Result<(Vec<f32>, u32), String> {
    let bytes = REFERENCE_WAV;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("benchmark reference is not a RIFF/WAVE file".to_owned());
    }

    let mut pos = 12;
    let mut channels: u16 = 1;
    let mut sample_rate: u32 = 16_000;
    let mut bits_per_sample: u16 = 16;
    let mut data: Option<&[u8]> = None;

    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body_start = pos + 8;
        let body_end = (body_start + size).min(bytes.len());

        match chunk_id {
            b"fmt " if body_end - body_start >= 16 => {
                channels = u16::from_le_bytes([bytes[body_start + 2], bytes[body_start + 3]]);
                sample_rate = u32::from_le_bytes([
                    bytes[body_start + 4],
                    bytes[body_start + 5],
                    bytes[body_start + 6],
                    bytes[body_start + 7],
                ]);
                bits_per_sample =
                    u16::from_le_bytes([bytes[body_start + 14], bytes[body_start + 15]]);
            }
            b"data" => data = Some(&bytes[body_start..body_end]),
            _ => {}
        }

        // RIFF chunks are word-aligned: an odd size carries a pad byte.
        pos = body_end + (size & 1);
    }

    if bits_per_sample != 16 {
        return Err(format!(
            "benchmark reference must be 16-bit PCM (got {bits_per_sample}-bit)"
        ));
    }
    let data = data.ok_or_else(|| "benchmark reference has no data chunk".to_owned())?;
    let channels = channels.max(1) as usize;

    let interleaved: Vec<f32> = data
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32_768.0)
        .collect();

    let mono = if channels <= 1 {
        interleaved
    } else {
        interleaved
            .chunks(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
            .collect()
    };

    Ok((mono, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_wav_decodes_to_expected_length() {
        let (samples, sample_rate) = decode_reference_wav().expect("embedded reference decodes");
        assert_eq!(sample_rate, 16_000);
        // ~13.6s of 16 kHz audio; assert a sane, non-empty range.
        assert!(
            samples.len() > 16_000 * 5,
            "reference should be several seconds"
        );
        assert!(samples.iter().all(|s| (-1.0..=1.0).contains(s)));
    }

    #[test]
    fn quality_score_is_one_for_exact_reference() {
        let reference = normalized_words(REFERENCE_TEXT);
        assert!((quality_score(&reference, REFERENCE_TEXT) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn quality_score_partial_and_empty() {
        let reference = normalized_words("das schnelle diktieren");
        assert!((quality_score(&reference, "das diktieren") - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(quality_score(&reference, ""), 0.0);
        assert_eq!(quality_score(&[], "irgendwas"), 0.0);
    }

    #[test]
    fn metal_detection_accepts_both_formats() {
        use crate::dictation::whisper_metal_compiled_in;
        // whisper.cpp 1.8.x
        assert!(whisper_metal_compiled_in(
            "WHISPER : COREML = 0 | Metal : EMBED_LIBRARY = 1 | CPU : NEON = 1 |"
        ));
        // older format
        assert!(whisper_metal_compiled_in("WHISPER : METAL = 1 | NEON = 1"));
        // CPU-only build
        assert!(!whisper_metal_compiled_in(
            "WHISPER : COREML = 0 | CPU : NEON = 1 |"
        ));
    }

    #[test]
    fn model_candidates_lead_with_headline_and_are_unique() {
        let candidates = model_candidates();
        assert_eq!(candidates[0], ModelPreset::LargeV3TurboQ5_0);
        assert_eq!(candidates[1], ModelPreset::LargeV3Turbo);
        let mut deduped = candidates.clone();
        deduped.dedup();
        assert_eq!(deduped.len(), candidates.len());
        assert_eq!(candidates.len(), ModelPreset::ALL.len());
    }
}
