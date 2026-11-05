//! Offline, repeatable CPU/Metal comparison for #43. Does not initialize the
//! app runtime, read/write user settings, register hotkeys, or touch audio I/O.

use std::time::Instant;

use serde::Serialize;
use torrowhisper_core::{AppSettings, ModelPreset};
use whisper_rs::WhisperContext;

use super::{
    REFERENCE_SHORT_TEXT, REFERENCE_SHORT_WAV, REFERENCE_TEXT, REFERENCE_WAV, current_rss_mb,
    decode_wav, normalized_words,
};
use crate::dictation::{
    audio_ctx_for, resample_to_16khz, run_whisper_inference, whisper_context_params,
};
use crate::model_manager::{ModelIntegrity, default_model_path, preset_model_integrity};

/// Reference clip of a benchmark run (#88): the long clip measures sustained
/// decoding, the short one a typical quick dictation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Clip {
    Long,
    Short,
}

impl Clip {
    fn wav(self) -> &'static [u8] {
        match self {
            Self::Long => REFERENCE_WAV,
            Self::Short => REFERENCE_SHORT_WAV,
        }
    }

    pub fn reference_text(self) -> &'static str {
        match self {
            Self::Long => REFERENCE_TEXT,
            Self::Short => REFERENCE_SHORT_TEXT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
        }
    }
}

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
    pub clip: Clip,
    pub flash_attn: bool,
    /// Encoder window actually passed to whisper.cpp; `0` = full 30 s window.
    pub audio_ctx: i32,
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
/// `flash_attn` and `dynamic_audio_ctx` select the #88 variant independently of
/// the app's production switches.
#[allow(clippy::too_many_arguments)]
pub fn run(
    model: ModelPreset,
    gpu: bool,
    threads: u32,
    repetitions: u32,
    language: Option<&str>,
    single_segment: bool,
    flash_attn: bool,
    dynamic_audio_ctx: bool,
    clip: Clip,
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
    let (audio, rate) = decode_wav(clip.wav())?;
    let samples = resample_to_16khz(&audio, rate);
    let prepare_audio_secs = prep.elapsed().as_secs_f32();
    let audio_secs = samples.len() as f32 / 16_000.0;
    let audio_ctx = audio_ctx_for(samples.len(), dynamic_audio_ctx);
    let reference_text = clip.reference_text();
    let settings = AppSettings {
        whisper_single_segment: single_segment,
        ..AppSettings::default()
    };
    let mut params = whisper_context_params(flash_attn);
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
        let result = run_whisper_inference(
            &context,
            &samples,
            &settings,
            language,
            threads as i32,
            dynamic_audio_ctx,
        )?;
        let total_secs = started.elapsed().as_secs_f32();
        Ok(Pass {
            state_secs: result.state_secs,
            inference_secs: result.inference_secs,
            total_secs,
            real_time_factor: result.inference_secs / audio_secs,
            word_error_rate: word_error_rate(reference_text, &result.text),
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
        clip,
        flash_attn,
        audio_ctx,
        audio_secs,
        reference_text,
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
        let invalid = |threads, repetitions, clip| {
            run(
                ModelPreset::Tiny,
                false,
                threads,
                repetitions,
                Some("de"),
                false,
                true,
                true,
                clip,
            )
            .is_err()
        };
        assert!(invalid(0, 3, Clip::Long));
        assert!(invalid(4, 0, Clip::Short));
        assert!(invalid(17, 3, Clip::Long));
        assert!(invalid(4, 21, Clip::Short));
    }

    /// The pre-#89 resampler (linear interpolation, no low-pass), kept only
    /// as the baseline for the comparison below.
    fn linear_resample_to_16khz(samples: &[f32], sample_rate: u32) -> Vec<f32> {
        let ratio = 16_000.0 / sample_rate as f64;
        let target_len = (samples.len() as f64 * ratio).round() as usize;
        (0..target_len)
            .map(|index| {
                let source_position = index as f64 / ratio;
                let source_index = source_position.floor() as usize;
                let frac = (source_position - source_index as f64) as f32;
                let current = *samples.get(source_index).unwrap_or(&0.0);
                let next = *samples.get(source_index + 1).unwrap_or(&current);
                current + (next - current) * frac
            })
            .collect()
    }

    /// The final pass must not lose quality with the band-limited resampler
    /// (#89). Ignored by default (needs a local model and a 48 kHz clip):
    /// ```sh
    /// say -v Anna -o /tmp/reference-48k.wav --data-format=LEI16@48000 "<REFERENCE_TEXT>"
    /// OW_TEST_MODEL=~/Library/Application\ Support/com.gettorro.TorroWhisper/models/ggml-large-v3-turbo-q5_0.bin \
    /// OW_TEST_WAV48=/tmp/reference-48k.wav \
    /// cargo test --release -p torrowhisper-bridge final_pass_resampler -- --ignored --nocapture
    /// ```
    /// `OW_TEST_REF_TEXT_FILE` optionally names a file with the clip's script,
    /// for clips other than `REFERENCE_TEXT`.
    #[test]
    #[ignore]
    fn final_pass_resampler_keeps_word_error_rate() {
        use crate::dictation::{WHISPER_DYNAMIC_AUDIO_CTX, WHISPER_FLASH_ATTN};

        let Some(model_path) = std::env::var_os("OW_TEST_MODEL") else {
            eprintln!("OW_TEST_MODEL not set — skipping");
            return;
        };
        let Some(wav_path) = std::env::var_os("OW_TEST_WAV48") else {
            eprintln!("OW_TEST_WAV48 not set — skipping");
            return;
        };
        let bytes = std::fs::read(&wav_path).expect("48 kHz clip readable");
        let (audio, rate) = decode_wav(&bytes).expect("16-bit PCM clip");
        assert_ne!(
            rate, 16_000,
            "the comparison needs a clip that gets resampled"
        );

        let context = WhisperContext::new_with_params(
            std::path::Path::new(&model_path),
            whisper_context_params(WHISPER_FLASH_ATTN),
        )
        .expect("test model loads");
        let settings = AppSettings::default();
        let reference = std::env::var_os("OW_TEST_REF_TEXT_FILE")
            .map(|path| std::fs::read_to_string(path).expect("reference text readable"))
            .unwrap_or_else(|| REFERENCE_TEXT.to_owned());

        let started = Instant::now();
        let linear = linear_resample_to_16khz(&audio, rate);
        let linear_secs = started.elapsed().as_secs_f32();
        let started = Instant::now();
        let filtered = resample_to_16khz(&audio, rate);
        let filtered_secs = started.elapsed().as_secs_f32();
        eprintln!(
            "resampling {:.1}s at {rate} Hz: linear {linear_secs:.4}s, filtered {filtered_secs:.4}s",
            audio.len() as f32 / rate as f32
        );
        assert_eq!(linear.len(), filtered.len());

        for language in [Some("de"), None] {
            let wer = |samples: &[f32]| {
                let inference = run_whisper_inference(
                    &context,
                    samples,
                    &settings,
                    language,
                    4,
                    WHISPER_DYNAMIC_AUDIO_CTX,
                )
                .expect("inference runs");
                (word_error_rate(&reference, &inference.text), inference.text)
            };
            let (old_wer, old_text) = wer(&linear);
            let (new_wer, new_text) = wer(&filtered);
            let label = language.unwrap_or("auto");
            eprintln!("[{label}] linear WER {old_wer:.3}: {old_text}");
            eprintln!("[{label}] filtered WER {new_wer:.3}: {new_text}");
            assert!(
                new_wer <= old_wer,
                "[{label}] filtered resampling raised the WER: {old_wer} → {new_wer}"
            );
        }
    }

    #[test]
    fn clips_carry_their_own_reference_text() {
        assert_eq!(Clip::Long.reference_text(), REFERENCE_TEXT);
        assert_eq!(Clip::Short.reference_text(), REFERENCE_SHORT_TEXT);
        assert_eq!(Clip::Long.name(), "long");
        assert_eq!(Clip::Short.name(), "short");
        assert_eq!(serde_json::to_value(Clip::Short).unwrap(), "short");
    }
}
