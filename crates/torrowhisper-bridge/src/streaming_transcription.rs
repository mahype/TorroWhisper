//! Live transcription while a recording is running (#41).
//!
//! One worker thread per recording session snapshots the growing capture
//! buffer on a fixed cadence, re-transcribes it with the already-cached
//! Whisper context, stabilizes the hypotheses into committed/pending text
//! and publishes the result into a shared [`StreamingOutput`] slot that the
//! Swift overlay polls via `ow_get_streaming_transcript`.
//!
//! Concurrency model: passes are sequential by construction (one worker, one
//! loop), so results are inherently latest-wins. Whisper inference is
//! additionally serialized process-wide through `whisper_inference_lock`, and
//! the final post-stop pass `join()`s the session first — streaming and final
//! inference can never overlap.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use torrowhisper_core::{AppSettings, StreamingTranscriptDto};
use whisper_rs::WhisperContext;

use crate::dictation::{
    RecordingBuffer, base_full_params, collect_transcript, resample_to_16khz,
    try_whisper_inference_lock,
};
use crate::transcript_stabilizer::TranscriptStabilizer;

/// Words become committed once they were identical this many passes in a row.
const STREAM_REQUIRED_PASSES: u32 = 2;
/// The last N words always stay pending — Whisper revises the tail longest.
const STREAM_HOLDBACK_WORDS: usize = 4;
/// Loop tick; bounds how fast the worker reacts to the stop flag.
const STREAM_TICK: Duration = Duration::from_millis(150);
/// Minimum gap between the starts of two streaming passes. Slow models simply
/// run back-to-back instead (the loop self-throttles).
const STREAM_PASS_INTERVAL: Duration = Duration::from_millis(500);
/// A first pass only makes sense once actual speech was heard; this gate also
/// keeps silence-only recordings at zero inference (no hallucinated text).
const STREAM_FIRST_PASS_MIN_VOICED_MS: u64 = 600;
/// Skip a pass when almost no new audio arrived since the previous one.
const STREAM_MIN_NEW_AUDIO_MS: u64 = 250;
/// After this much trailing silence the holdback is lifted: the user stopped
/// speaking, Whisper has nothing left to revise, and the dimmed tail should
/// finish committing instead of staying pending forever.
const SILENCE_COMMIT_DELAY: Duration = Duration::from_millis(1_500);
/// Snapshots end this far after the last speech chunk. Whisper never sees the
/// trailing silence beyond it — decoding into silence is where it hallucinates
/// filler ("Vielen Dank", repeated words), which must never reach the preview.
const SPEECH_TAIL_PADDING_MS: u64 = 500;

/// The published live-transcript state. One instance lives on the
/// `DictationController` for the whole process; sessions write into it.
/// `revision` is globally monotonic — it is bumped on every write and never
/// reset, so consumers can discard anything not strictly newer than the last
/// revision they saw, across session boundaries.
pub(crate) struct StreamingOutput {
    revision: u64,
    committed: String,
    pending: String,
    is_final: bool,
}

impl StreamingOutput {
    pub(crate) fn new() -> Self {
        Self {
            revision: 0,
            committed: String::new(),
            pending: String::new(),
            is_final: false,
        }
    }

    fn publish(&mut self, committed: String, pending: String, is_final: bool) {
        self.revision += 1;
        self.committed = committed;
        self.pending = pending;
        self.is_final = is_final;
    }

    fn to_dto(&self) -> StreamingTranscriptDto {
        StreamingTranscriptDto {
            revision: self.revision,
            committed: self.committed.clone(),
            pending: self.pending.clone(),
            is_final: self.is_final,
        }
    }
}

pub(crate) type SharedStreamingOutput = Arc<Mutex<StreamingOutput>>;

/// Clears the text for a new session while keeping the revision monotonic.
pub(crate) fn reset_output(output: &SharedStreamingOutput) {
    if let Ok(mut guard) = output.lock() {
        guard.publish(String::new(), String::new(), false);
    }
}

/// Publishes the final post-stop transcript (raw Whisper text, pre-pipeline).
pub(crate) fn publish_final(output: &SharedStreamingOutput, text: &str) {
    if let Ok(mut guard) = output.lock() {
        guard.publish(text.to_owned(), String::new(), true);
    }
}

/// Marks the current text as final without changing it — for error paths
/// where the final pass failed and no better text will ever arrive.
pub(crate) fn mark_final_unchanged(output: &SharedStreamingOutput) {
    if let Ok(mut guard) = output.lock() {
        let committed = guard.committed.clone();
        let pending = guard.pending.clone();
        guard.publish(committed, pending, true);
    }
}

pub(crate) fn snapshot_dto(output: &SharedStreamingOutput) -> StreamingTranscriptDto {
    output
        .lock()
        .map(|guard| guard.to_dto())
        .unwrap_or_else(|_| StreamingTranscriptDto {
            revision: 0,
            committed: String::new(),
            pending: String::new(),
            is_final: false,
        })
}

pub(crate) struct StreamingConfig {
    /// Clone of `ActiveRecording.shared` — the live capture buffer.
    pub buffer: Arc<Mutex<RecordingBuffer>>,
    pub output: SharedStreamingOutput,
    /// Warm context from the model cache. Sessions only start warm; loading a
    /// model belongs to the warmup/final paths.
    pub context: Arc<WhisperContext>,
    pub settings: AppSettings,
    pub language: Option<String>,
}

/// Handle to a running streaming worker. Dropping it signals the worker to
/// stop and detaches it (the worker exits within one tick / one abort);
/// [`StreamingSession::join`] additionally waits for it — the hard guarantee
/// the final pass relies on.
pub(crate) struct StreamingSession {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl StreamingSession {
    pub(crate) fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub(crate) fn join(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for StreamingSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn spawn(config: StreamingConfig) -> StreamingSession {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let handle = thread::Builder::new()
        .name("streaming-transcription".to_owned())
        .spawn(move || worker_loop(config, worker_stop))
        .map_err(|err| {
            log::warn!(target: "dictation", "streaming worker could not start: {err}");
            err
        })
        .ok();
    StreamingSession { stop, handle }
}

fn worker_loop(config: StreamingConfig, stop: Arc<AtomicBool>) {
    let StreamingConfig {
        buffer,
        output,
        context,
        settings,
        language,
    } = config;

    // One state per session, reused across passes — recreating it every
    // 750 ms would reallocate the KV caches (noticeable on large models).
    let mut state = match context.create_state() {
        Ok(state) => state,
        Err(err) => {
            log::warn!(target: "dictation", "streaming state could not be created: {err}");
            return;
        }
    };

    let n_threads = streaming_thread_count();
    let mut stabilizer = TranscriptStabilizer::new(STREAM_HOLDBACK_WORDS, STREAM_REQUIRED_PASSES);
    let mut audio: Vec<f32> = Vec::new();
    let session_started = Instant::now();
    let mut last_pass_started: Option<Instant> = None;
    let mut samples_at_last_pass: usize = 0;
    let mut pass_count: u32 = 0;
    let mut gate_logged = false;
    let mut busy_logged = false;
    let mut first_pass_logged = false;
    let mut inference_warned = false;
    // Language auto-detection runs per pass (over the growing buffer) when no
    // language is configured; remembered so a flip can reset the preview.
    let mut last_lang_id: Option<i32> = None;
    // Trailing-silence tracking for the holdback lift.
    let mut last_voiced_total: usize = 0;
    let mut voiced_changed_at = Instant::now();
    // True once the stabilizer reported an empty pending tail — lets the
    // silence-commit pass stop retrying when everything is committed.
    let mut last_pending_empty = true;

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        thread::sleep(STREAM_TICK);
        if stop.load(Ordering::Relaxed) {
            return;
        }

        // Short lock: append only the samples that arrived since last tick.
        let (sample_rate, voiced_samples, last_voiced_end) = {
            let Ok(guard) = buffer.lock() else { return };
            guard.copy_new_samples(audio.len(), &mut audio);
            (
                guard.sample_rate(),
                guard.voiced_samples(),
                guard.last_voiced_end(),
            )
        };

        if voiced_samples != last_voiced_total {
            last_voiced_total = voiced_samples;
            voiced_changed_at = Instant::now();
        }
        // While speech keeps coming, the last words stay pending (Whisper
        // revises the freshest words the longest); once the user pauses,
        // lift the holdback so the stable tail finishes committing.
        let holdback = if voiced_changed_at.elapsed() >= SILENCE_COMMIT_DELAY {
            0
        } else {
            STREAM_HOLDBACK_WORDS
        };
        stabilizer.set_holdback(holdback);

        if !voiced_gate_reached(voiced_samples, sample_rate) {
            continue;
        }
        if !gate_logged {
            gate_logged = true;
            log::info!(
                target: "dictation",
                "streaming gate opened {:.2}s into the take ({} ms voiced)",
                session_started.elapsed().as_secs_f32(),
                voiced_samples as u64 * 1_000 / sample_rate.max(1) as u64
            );
        }
        if let Some(last) = last_pass_started
            && last.elapsed() < STREAM_PASS_INTERVAL
        {
            continue;
        }
        // Snapshots end shortly after the last heard speech: Whisper must not
        // decode into trailing silence (hallucination material). During a
        // pause the input therefore freezes — passes stop once nothing new
        // arrived, EXCEPT while a lifted holdback still has pending text to
        // commit (identical input stabilizes it within a pass or two).
        let speech_end =
            (last_voiced_end + ms_to_samples(SPEECH_TAIL_PADDING_MS, sample_rate)).min(audio.len());
        let silence_commit_due = holdback == 0 && !last_pending_empty;
        if !enough_new_audio(samples_at_last_pass, speech_end, sample_rate) && !silence_commit_due {
            continue;
        }

        // Never queue behind another inference (e.g. the final pass of a
        // just-cancelled dictation, possibly paying the one-time Metal graph
        // compile): the pass would publish a stale snapshot much later. Skip
        // and retry next tick with fresh audio instead.
        let Some(inference_guard) = try_whisper_inference_lock() else {
            if !busy_logged {
                busy_logged = true;
                log::info!(
                    target: "dictation",
                    "streaming pass deferred: another whisper inference is running"
                );
            }
            continue;
        };

        last_pass_started = Some(Instant::now());
        samples_at_last_pass = speech_end;

        // v1 prototype: re-transcribe the whole take (up to the end of
        // speech) each pass — latest-wins degrades gracefully on long takes;
        // the sliding window is follow-up.
        let mono_16khz = resample_to_16khz(&audio[..speech_end], sample_rate);
        if mono_16khz.is_empty() {
            continue;
        }

        let mut params = base_full_params(&settings, language.as_deref(), n_threads);
        // The state is reused across passes; without this, tokens of the
        // previous pass would bias the re-decode of the same audio.
        params.set_no_context(true);
        // Raw abort wiring instead of whisper-rs 0.16's `set_abort_callback_safe`:
        // its trampoline casts the double-boxed closure back with the wrong
        // type and reads garbage — passes then abort spuriously with encode
        // error -6. `stop` outlives every pass (the worker owns an Arc clone),
        // so handing ggml the raw AtomicBool pointer is sound.
        unsafe {
            params.set_abort_callback(Some(abort_when_stopped));
            params.set_abort_callback_user_data(Arc::as_ptr(&stop) as *mut std::ffi::c_void);
        }

        let pass_started = Instant::now();
        let pass_result = state.full(params, &mono_16khz);
        drop(inference_guard);

        match pass_result {
            Err(_) if stop.load(Ordering::Relaxed) => return, // expected abort
            Err(err) => {
                if !inference_warned {
                    inference_warned = true;
                    log::warn!(
                        target: "dictation",
                        "streaming pass failed (continuing latest-wins): {err}"
                    );
                }
                continue;
            }
            Ok(()) => {}
        }

        // With language on auto, the first short passes can misdetect the
        // language (e.g. English on 1-2 s of German) — Whisper then emits a
        // translation, and the stabilizer would freeze that wrong-language
        // text forever. Detection converges as the buffer grows, so on a flip
        // we throw the preview away and rebuild from the current hypothesis.
        // The final pass is unaffected either way.
        if language.is_none() {
            let lang_id = state.full_lang_id_from_state();
            if let Some(previous) = last_lang_id
                && previous != lang_id
            {
                stabilizer =
                    TranscriptStabilizer::new(STREAM_HOLDBACK_WORDS, STREAM_REQUIRED_PASSES);
                log::info!(
                    target: "dictation",
                    "streaming language flipped {} → {}, preview reset",
                    whisper_rs::get_lang_str(previous).unwrap_or("?"),
                    whisper_rs::get_lang_str(lang_id).unwrap_or("?")
                );
            }
            last_lang_id = Some(lang_id);
        }

        let hypothesis = collect_transcript(&state);
        pass_count += 1;
        if !first_pass_logged {
            first_pass_logged = true;
            log::info!(
                target: "dictation",
                "first streaming transcript after {:.2}s ({} chars, {} threads)",
                session_started.elapsed().as_secs_f32(),
                hypothesis.chars().count(),
                n_threads
            );
        } else {
            log::debug!(
                target: "dictation",
                "streaming pass {pass_count}: {:.1}s audio → {} chars in {:.2}s",
                mono_16khz.len() as f32 / 16_000.0,
                hypothesis.chars().count(),
                pass_started.elapsed().as_secs_f32()
            );
        }

        let stabilized = stabilizer.observe(&hypothesis);
        last_pending_empty = stabilized.pending.is_empty();
        if let Ok(mut guard) = output.lock() {
            guard.publish(stabilized.committed, stabilized.pending, false);
        }
    }
}

/// ggml abort callback for streaming passes: aborts the in-flight inference as
/// soon as the session's stop flag is set (recording stopped/cancelled), so
/// the final pass never waits behind a streaming pass.
unsafe extern "C" fn abort_when_stopped(user_data: *mut std::ffi::c_void) -> bool {
    let stop = unsafe { &*(user_data as *const AtomicBool) };
    stop.load(Ordering::Relaxed)
}

/// Streaming passes deliberately use fewer threads than the final pass: they
/// run while the UI, the audio callback and the user's foreground app are all
/// live, and preview latency matters less than system responsiveness.
fn streaming_thread_count() -> i32 {
    let cores = thread::available_parallelism()
        .map(|cores| cores.get())
        .unwrap_or(4);
    (cores / 2).clamp(2, 4) as i32
}

fn ms_to_samples(ms: u64, sample_rate: u32) -> usize {
    (sample_rate as u64 * ms / 1000) as usize
}

fn voiced_gate_reached(voiced_samples: usize, sample_rate: u32) -> bool {
    sample_rate > 0 && voiced_samples >= ms_to_samples(STREAM_FIRST_PASS_MIN_VOICED_MS, sample_rate)
}

fn enough_new_audio(previous_len: usize, current_len: usize, sample_rate: u32) -> bool {
    current_len.saturating_sub(previous_len)
        >= ms_to_samples(STREAM_MIN_NEW_AUDIO_MS, sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voiced_gate_requires_600ms_of_speech() {
        let rate = 48_000;
        let gate = ms_to_samples(STREAM_FIRST_PASS_MIN_VOICED_MS, rate);
        assert_eq!(gate, 28_800);
        assert!(!voiced_gate_reached(gate - 1, rate));
        assert!(voiced_gate_reached(gate, rate));
        assert!(!voiced_gate_reached(usize::MAX, 0), "zero rate never gates");
    }

    #[test]
    fn new_audio_gate_requires_250ms() {
        let rate = 16_000;
        assert!(!enough_new_audio(10_000, 10_000 + 3_999, rate));
        assert!(enough_new_audio(10_000, 10_000 + 4_000, rate));
    }

    #[test]
    fn streaming_thread_count_stays_modest() {
        let threads = streaming_thread_count();
        assert!((2..=4).contains(&threads));
    }

    /// Full pipeline over real synthesized speech: WAV → RecordingBuffer →
    /// worker passes → stabilizer → monotonic revisions → stop/join. Ignored
    /// by default (needs a local Whisper model and ~20-40 s). Run with:
    /// ```sh
    /// OW_TEST_MODEL=~/Library/Application\ Support/TorroWhisper/models/ggml-large-v3-turbo-q5_0.bin \
    /// OW_TEST_WAV=/tmp/speech-de.wav \
    /// cargo test -p torrowhisper-bridge streaming_end_to_end -- --ignored --nocapture
    /// ```
    /// Generate the WAV with:
    /// `say -v Anna -o /tmp/speech-de.wav --data-format=LEI16@16000 "<German text>"`
    #[test]
    #[ignore]
    fn streaming_end_to_end_with_synthesized_speech() {
        use crate::dictation::RecordingBuffer;
        use whisper_rs::WhisperContextParameters;

        struct StderrLogger;
        impl log::Log for StderrLogger {
            fn enabled(&self, _metadata: &log::Metadata) -> bool {
                true
            }
            fn log(&self, record: &log::Record) {
                eprintln!("[{}] {}", record.level(), record.args());
            }
            fn flush(&self) {}
        }
        static STDERR_LOGGER: StderrLogger = StderrLogger;
        let _ = log::set_logger(&STDERR_LOGGER);
        log::set_max_level(log::LevelFilter::Debug);

        let Some(model_path) = std::env::var_os("OW_TEST_MODEL") else {
            eprintln!("OW_TEST_MODEL not set — skipping");
            return;
        };
        let Some(wav_path) = std::env::var_os("OW_TEST_WAV") else {
            eprintln!("OW_TEST_WAV not set — skipping");
            return;
        };

        let samples = read_wav_lei16_mono(std::path::Path::new(&wav_path));
        assert!(
            samples.len() > 16_000 * 5,
            "need at least 5 s of test speech"
        );

        let context = Arc::new(
            WhisperContext::new_with_params(
                std::path::Path::new(&model_path),
                WhisperContextParameters::default(),
            )
            .expect("test model loads"),
        );

        let buffer = Arc::new(Mutex::new(RecordingBuffer::new(16_000, false, 0.014, 900)));
        let output: SharedStreamingOutput = Arc::new(Mutex::new(StreamingOutput::new()));
        reset_output(&output);

        // OW_TEST_LANG=auto exercises the auto-detection path (language flips
        // may legitimately reset the preview, so the strict committed-prefix
        // assertion below is skipped); default is a fixed "de".
        let test_language = match std::env::var("OW_TEST_LANG").as_deref() {
            Ok("auto") => None,
            Ok(other) => Some(other.to_owned()),
            Err(_) => Some("de".to_owned()),
        };
        let strict_monotonicity = test_language.is_some();

        let session = spawn(StreamingConfig {
            buffer: buffer.clone(),
            output: output.clone(),
            context,
            settings: torrowhisper_core::AppSettings::default(),
            language: test_language,
        });

        // Feed at ~2x real time in 100 ms ticks and collect every new revision.
        // (The first pass pays the Metal state cold-start, so the feed must
        // not outrun the worker completely.)
        let (event_tx, _event_rx) = std::sync::mpsc::channel();
        let chunk_len = 16_000 / 10 * 2;
        let mut fed = 0usize;
        let mut snapshots: Vec<StreamingTranscriptDto> = vec![snapshot_dto(&output)];
        let deadline = Instant::now() + Duration::from_secs(90);
        while fed < samples.len() && Instant::now() < deadline {
            let end = (fed + chunk_len).min(samples.len());
            buffer
                .lock()
                .unwrap()
                .push_chunk(&samples[fed..end], &event_tx);
            fed = end;
            thread::sleep(Duration::from_millis(100));
            let dto = snapshot_dto(&output);
            if dto.revision > snapshots.last().unwrap().revision {
                snapshots.push(dto);
            }
        }
        // Trailing silence: keep feeding zero-audio like a live microphone
        // would, so the silence-commit path runs (holdback lifts, the dimmed
        // tail finishes committing). Then settle until revisions stop
        // advancing, max 45 s — the budget is generous because the first pass
        // pays one-time Metal initialization on top of the inference itself.
        let silence_chunk = vec![0.0_f32; chunk_len];
        let settle_deadline = Instant::now() + Duration::from_secs(45);
        let mut last_advance = Instant::now();
        while Instant::now() < settle_deadline
            && last_advance.elapsed() < Duration::from_secs(10)
        {
            buffer
                .lock()
                .unwrap()
                .push_chunk(&silence_chunk, &event_tx);
            thread::sleep(Duration::from_millis(100));
            let dto = snapshot_dto(&output);
            if dto.revision > snapshots.last().unwrap().revision {
                snapshots.push(dto);
                last_advance = Instant::now();
            }
        }
        session.join();

        let last = snapshots.last().unwrap();
        let full_text = format!("{} {}", last.committed, last.pending).to_lowercase();
        eprintln!("streaming revisions: {:?}", snapshots.iter().map(|s| s.revision).collect::<Vec<_>>());
        eprintln!("final streaming text: {full_text}");

        let revisions: Vec<u64> = snapshots.iter().map(|s| s.revision).collect();
        assert!(
            revisions.windows(2).all(|pair| pair[0] < pair[1]),
            "revisions must be strictly increasing: {revisions:?}"
        );
        assert!(
            last.revision >= 3,
            "expected several streaming passes, got revision {}",
            last.revision
        );
        assert!(
            full_text.contains("transkription"),
            "recognized text should mention the dictated word: {full_text}"
        );
        assert!(
            last.pending.is_empty(),
            "after trailing silence the whole tail must commit, still pending: '{}'",
            last.pending
        );

        // The core display invariant: committed text only ever grows (unless
        // a language flip legitimately reset the preview in auto mode).
        if strict_monotonicity {
            let mut previous = String::new();
            for snapshot in &snapshots {
                assert!(
                    snapshot.committed.starts_with(&previous),
                    "committed must be append-only: '{previous}' → '{}'",
                    snapshot.committed
                );
                previous = snapshot.committed.clone();
            }
        }
    }

    /// Minimal WAV reader for the test fixture: 16-bit little-endian mono PCM.
    fn read_wav_lei16_mono(path: &std::path::Path) -> Vec<f32> {
        let bytes = std::fs::read(path).expect("test wav readable");
        let data_start = bytes
            .windows(4)
            .position(|window| window == b"data")
            .expect("wav data chunk")
            + 8;
        bytes[data_start..]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32_768.0)
            .collect()
    }

    #[test]
    fn output_revision_is_monotonic_across_reset_and_final() {
        let output: SharedStreamingOutput = Arc::new(Mutex::new(StreamingOutput::new()));
        assert_eq!(snapshot_dto(&output).revision, 0);

        reset_output(&output);
        let after_reset = snapshot_dto(&output);
        assert_eq!(after_reset.revision, 1);
        assert!(after_reset.committed.is_empty());
        assert!(!after_reset.is_final);

        output
            .lock()
            .unwrap()
            .publish("hello".to_owned(), "world".to_owned(), false);
        publish_final(&output, "hello world");
        let final_dto = snapshot_dto(&output);
        assert_eq!(final_dto.revision, 3);
        assert_eq!(final_dto.committed, "hello world");
        assert!(final_dto.pending.is_empty());
        assert!(final_dto.is_final);

        // A new session resets text but keeps counting upward.
        reset_output(&output);
        assert_eq!(snapshot_dto(&output).revision, 4);
    }
}
