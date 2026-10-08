//! Offline, repeatable CPU/Metal comparison for #43. Does not initialize the
//! app runtime, read/write user settings, register hotkeys, or touch audio I/O.

use std::time::Instant;

use serde::Serialize;
use torrowhisper_core::{AppSettings, ModelPreset};
use whisper_rs::{WhisperContext, WhisperContextParameters};

use super::{REFERENCE_TEXT, current_rss_mb, decode_reference_wav, normalized_words};
use crate::dictation::{resample_to_16khz, run_whisper_inference};
use crate::model_manager::{ModelIntegrity, default_model_path, preset_model_integrity};

#[derive(Serialize)]
pub struct Pass {
    pub state_secs: f32,
    pub inference_secs: f32,
    /// Entire shared inference call, including state creation and text collection.
    pub total_secs: f32,
    pub real_time_factor: f32,
    /// Word edit distance / reference word count; may exceed 1 for insertions.
    pub word_error_rate: f32,
    pub transcript: String,
}

#[derive(Serialize)]
pub struct ValidationReport {
    pub model: ModelPreset,
    pub model_filename: String,
    pub whisper_version: String,
    /// Requested backend. Verify actual device selection in captured ggml logs.
    pub backend_requested: &'static str,
    pub threads: u32,
    pub language: Option<String>,
    pub single_segment: bool,
    pub audio_secs: f32,
    pub reference_text: &'static str,
    pub prepare_audio_secs: f32,
    pub load_secs: f32,
    pub load_rss_mib: Option<f32>,
    pub cold: Pass,
    pub warm: Vec<Pass>,
}

/// One configuration per fresh process keeps Metal pipeline initialization out
/// of subsequent configurations' cold measurements. The first pass is retained
/// separately; repetitions reuse the model but create a fresh state, like dictation.
pub fn run(
    model: ModelPreset,
    gpu: bool,
    threads: u32,
    repetitions: u32,
    language: Option<&str>,
    single_segment: bool,
) -> Result<ValidationReport, String> {
    if !(1..=16).contains(&threads) || !(1..=20).contains(&repetitions) {
        return Err("threads must be 1..=16 and repetitions 1..=20".to_owned());
    }
    if gpu && !cfg!(target_os = "macos") {
        return Err("this project's Metal benchmark requires macOS".to_owned());
    }
    if !matches!(preset_model_integrity(model), ModelIntegrity::Valid) {
        return Err(format!(
            "{} is missing or fails integrity verification",
            model.default_filename()
        ));
    }
    let prep = Instant::now();
    let (audio, rate) = decode_reference_wav()?;
    let samples = resample_to_16khz(&audio, rate);
    let prepare_audio_secs = prep.elapsed().as_secs_f32();
    let audio_secs = samples.len() as f32 / 16_000.0;
    let settings = AppSettings {
        whisper_single_segment: single_segment,
        ..AppSettings::default()
    };
    let mut params = WhisperContextParameters::default();
    params.use_gpu(gpu);
    let rss_before = current_rss_mb();
    let load_started = Instant::now();
    let context = WhisperContext::new_with_params(default_model_path(model)?, params)
        .map_err(|e| e.to_string())?;
    let load_secs = load_started.elapsed().as_secs_f32();
    let load_rss_mib = rss_before
        .zip(current_rss_mb())
        .map(|(before, after)| (after - before).max(0.0));
    let measure = || -> Result<Pass, String> {
        let started = Instant::now();
        let result =
            run_whisper_inference(&context, &samples, &settings, language, threads as i32)?;
        let total_secs = started.elapsed().as_secs_f32();
        Ok(Pass {
            state_secs: result.state_secs,
            inference_secs: result.inference_secs,
            total_secs,
            real_time_factor: result.inference_secs / audio_secs,
            word_error_rate: word_error_rate(REFERENCE_TEXT, &result.text),
            transcript: result.text,
        })
    };
    let cold = measure()?;
    let warm = (0..repetitions)
        .map(|_| measure())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ValidationReport {
        model,
        model_filename: model.default_filename().to_owned(),
        whisper_version: whisper_rs::WHISPER_CPP_VERSION.to_owned(),
        backend_requested: if gpu { "metal" } else { "cpu" },
        threads,
        language: language.map(str::to_owned),
        single_segment,
        audio_secs,
        reference_text: REFERENCE_TEXT,
        prepare_audio_secs,
        load_secs,
        load_rss_mib,
        cold,
        warm,
    })
}

/// Ordered word edit distance, unlike the UI benchmark's rough word coverage:
/// extra, missing and reordered words all count as recognition errors.
fn word_error_rate(reference: &str, hypothesis: &str) -> f32 {
    let reference = normalized_words(reference);
    let hypothesis = normalized_words(hypothesis);
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, expected) in reference.iter().enumerate() {
        let mut row = vec![i + 1; hypothesis.len() + 1];
        for (j, actual) in hypothesis.iter().enumerate() {
            row[j + 1] = (previous[j] + usize::from(expected != actual))
                .min(previous[j + 1] + 1)
                .min(row[j] + 1);
        }
        previous = row;
    }
    previous[hypothesis.len()] as f32 / reference.len().max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_errors_include_insertions_deletions_substitutions_and_order() {
        assert_eq!(word_error_rate("Hallo, Welt!", "hallo welt"), 0.0);
        assert_eq!(word_error_rate("eins zwei", "eins"), 0.5);
        assert_eq!(word_error_rate("eins zwei", "eins drei"), 0.5);
        assert_eq!(word_error_rate("eins", "eins zwei drei"), 2.0);
        assert_eq!(word_error_rate("eins zwei", "zwei eins"), 1.0);
    }

    #[test]
    fn invalid_sweeps_fail_before_loading_models() {
        assert!(run(ModelPreset::Tiny, false, 0, 3, Some("de"), false).is_err());
        assert!(run(ModelPreset::Tiny, false, 4, 0, Some("de"), false).is_err());
        assert!(run(ModelPreset::Tiny, false, 17, 3, Some("de"), false).is_err());
        assert!(run(ModelPreset::Tiny, false, 4, 21, Some("de"), false).is_err());
    }
}
