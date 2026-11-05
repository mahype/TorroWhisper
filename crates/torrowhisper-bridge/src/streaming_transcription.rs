//! Live transcription while a recording is running (#41).
//!
//! One worker thread per recording session takes the new samples of the
//! growing capture buffer on a fixed cadence, resamples them once to 16 kHz,
//! re-transcribes the current decode window with the already-cached Whisper
//! context, stabilizes the hypotheses into committed/pending text and
//! publishes the result into a shared [`StreamingOutput`] slot that the Swift
//! overlay polls via `ow_get_streaming_transcript`.
//!
//! Sliding window (#89): a pass decodes only the audio since the last cut, so
//! its duration stays roughly constant on long takes instead of growing with
//! them. The window moves behind committed text — preferably at a speech
//! pause, during long uninterrupted speech at a segment boundary Whisper
//! reported. The final pass still decodes the whole take.
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
use whisper_rs::{WhisperContext, WhisperState};

use crate::dictation::{
    RecordingBuffer, WHISPER_DYNAMIC_AUDIO_CTX, audio_ctx_for, base_full_params,
    try_whisper_inference_lock,
};
use crate::resampler::StreamResampler;
use crate::transcript_stabilizer::{TranscriptStabilizer, WindowState, token_count};

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
/// The window behind a pause needs less new speech before its first pass, so
/// a short word after a pause ("Danke.") still reaches the preview (#89).
const STREAM_NEXT_WINDOW_MIN_VOICED_MS: u64 = 300;
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
/// Windows longer than this (20 s at 16 kHz) are decoded with segment
/// timestamps so the worker can cut inside continuous speech (#89).
const STREAM_WINDOW_SOFT_LIMIT_SAMPLES: usize = 16_000 * 20;
/// Whisper timestamps are in centiseconds.
const SAMPLES_PER_CENTISECOND: usize = 160;
/// 10 ms frames for the quietest-point search around a segment cut.
const FRAME_SAMPLES: usize = 160;
/// Search ±200 ms around the overlap start for the actual gap.
const CUT_SEARCH_RADIUS_SAMPLES: usize = 3_200;
/// A segment cut starts the next window this far (800 ms) before Whisper's
/// segment end. Segment times can be late by a few hundred ms; cutting right
/// at them clipped the next word, the re-decode then lacked it and
/// contradicted the committed text for good. The overlap re-decodes the last
/// archived words instead; the stabilizer skips them by alignment.
const CUT_OVERLAP_SAMPLES: usize = 12_800;
/// A segment cut leaves at least this many committed words in the window —
/// the anchor the stabilizer aligns the overlapping re-decode on. Four leave
/// room to drop up to two clipped words and still align on two.
const SEGMENT_CUT_ANCHOR_WORDS: usize = 4;
/// A segment cut whose re-decode contradicts the committed text this many
/// passes in a row is undone: the window falls back to its previous start,
/// and the next cut must lie behind the undone one.
const STREAM_CUT_UNDO_PASSES: u32 = 3;
/// With language on auto, the window is only cut once this much speech (4 s)
/// was heard in the first window, where detection runs over the whole take
/// and may still reset the preview; the first cut then pins the language.
/// Short windows misdetect too easily to detect on their own.
const STREAM_LANGUAGE_SETTLE_VOICED_MS: u64 = 4_000;
/// The window behind a pause starts this long before speech resumed, rather
/// than at the pause itself: a long pause is not decoded as silence.
const SPEECH_HEAD_PADDING_MS: u64 = 500;
/// Silence after a voiced run behind a pause that marks the run as a stray
/// sound (a click, a breath) when it stays below the speech gate: the window
/// start then follows the next run instead.
const RESUME_RUN_GAP_MS: u64 = 600;

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
    // Captured samples are resampled exactly once, as they arrive (#89):
    // `source_len` counts the source samples taken from the buffer, `audio16k`
    // holds everything resampled so far.
    let mut source_len: usize = 0;
    let mut new_samples: Vec<f32> = Vec::new();
    let mut resampler: Option<StreamResampler> = None;
    let mut audio16k: Vec<f32> = Vec::new();
    // Sliding window (#89): passes decode `audio16k[window_start..]` only.
    // The window moves behind committed text at a speech pause or, during
    // long uninterrupted speech, at a segment boundary Whisper reported.
    let mut window_start: usize = 0;
    // Voiced samples at the last pause cut: the window behind a pause holds
    // no words yet and passes the speech gate anew.
    let mut voiced_at_cut: usize = 0;
    // After a pause cut, the window start follows speech once it resumes.
    let mut pause_resume: Option<PauseResume> = None;
    // The last segment cut until a pass confirms it (previous window start,
    // stabilizer window, archived words), and how often it was contradicted.
    let mut cut_undo: Option<(usize, WindowState, usize)> = None;
    let mut contradicted_after_cut: u32 = 0;
    // After an undo, a new segment cut must archive more words than this.
    let mut min_cut_consumed: usize = 0;
    let session_started = Instant::now();
    let mut last_pass_started: Option<Instant> = None;
    let mut samples_at_last_pass: usize = 0;
    let mut pass_count: u32 = 0;
    let mut gate_logged = false;
    let mut busy_logged = false;
    let mut first_pass_logged = false;
    let mut inference_warned = false;
    // Language auto-detection runs per pass over the first window (the whole
    // take so far); remembered so a flip can reset the preview. Cuts wait
    // until enough speech settled the detection, and the first cut pins it.
    let mut last_lang_id: Option<i32> = None;
    let mut language_settled = language.is_some();
    let mut cut_happened = false;
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

        // Short lock: take only the samples that arrived since last tick.
        new_samples.clear();
        let (sample_rate, voiced_samples, last_voiced_end) = {
            let Ok(guard) = buffer.lock() else { return };
            guard.copy_new_samples(source_len, &mut new_samples);
            (
                guard.sample_rate(),
                guard.voiced_samples(),
                guard.last_voiced_end(),
            )
        };
        let previous_source_len = source_len;
        source_len += new_samples.len();
        resampler
            .get_or_insert_with(|| StreamResampler::new(sample_rate))
            .push(&new_samples, &mut audio16k);

        if let Some(resume) = pause_resume.as_mut() {
            let tracking = resume.tick(
                &ResumeTick {
                    voiced_samples,
                    voiced_before: last_voiced_total,
                    source_before: previous_source_len,
                    source_len,
                    last_voiced_end,
                    sample_rate,
                    audio16k_len: audio16k.len(),
                },
                &mut window_start,
                &mut voiced_at_cut,
            );
            if !tracking {
                pause_resume = None;
            }
        }

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

        let gate_ms = if window_start == 0 {
            STREAM_FIRST_PASS_MIN_VOICED_MS
        } else {
            STREAM_NEXT_WINDOW_MIN_VOICED_MS
        };
        if !voiced_gate_reached(
            voiced_samples.saturating_sub(voiced_at_cut),
            sample_rate,
            gate_ms,
        ) {
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
            (last_voiced_end + ms_to_samples(SPEECH_TAIL_PADDING_MS, sample_rate)).min(source_len);
        let silence_commit_due = holdback == 0 && !last_pending_empty;
        if !enough_new_audio(samples_at_last_pass, speech_end, sample_rate) && !silence_commit_due {
            continue;
        }
        let speech_end_16k = to_16khz_index(speech_end, sample_rate).min(audio16k.len());
        if speech_end_16k <= window_start {
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

        let window = &audio16k[window_start..speech_end_16k];
        // Auto language: detect freely in the first window (and reset on a
        // flip, below); from the first cut on keep the detected language.
        let language_pinned = cut_happened;
        let pass_language = match (&language, last_lang_id) {
            (Some(language), _) => Some(language.as_str()),
            (None, Some(lang_id)) if language_pinned => whisper_rs::get_lang_str(lang_id),
            (None, _) => None,
        };
        let mut params = base_full_params(
            &settings,
            pass_language,
            n_threads,
            audio_ctx_for(window.len(), WHISPER_DYNAMIC_AUDIO_CTX),
        );
        // The state is reused across passes; without this, tokens of the
        // previous pass would bias the re-decode of the same audio.
        params.set_no_context(true);
        // Long windows (no pause for a while) need segment timestamps to find
        // a cut position inside continuous speech.
        let wants_segments = window.len() > STREAM_WINDOW_SOFT_LIMIT_SAMPLES;
        if wants_segments {
            params.set_no_timestamps(false);
            params.set_single_segment(false);
        }
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
        let pass_result = state.full(params, window);
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
        // This only happens in the first window: cuts wait for the detection
        // to settle, and the first cut pins the language. The final pass is
        // unaffected either way.
        if language.is_none() && !language_pinned {
            let lang_id = state.full_lang_id_from_state();
            if voiced_samples >= ms_to_samples(STREAM_LANGUAGE_SETTLE_VOICED_MS, sample_rate) {
                language_settled = true;
            }
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

        let (hypothesis, segments) = collect_segments(&state);
        let window_secs = window.len() as f32 / 16_000.0;
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
                "streaming pass {pass_count}: window {:.1}s + {window_secs:.1}s audio → {} chars in {:.2}s",
                window_start as f32 / 16_000.0,
                hypothesis.chars().count(),
                pass_started.elapsed().as_secs_f32()
            );
        }

        let stabilized = stabilizer.observe(&hypothesis);
        last_pending_empty = stabilized.pending.is_empty();
        let contradicted = stabilized.contradicted;
        let overlap = stabilized.overlap;
        if let Ok(mut guard) = output.lock() {
            guard.publish(stabilized.committed, stabilized.pending, false);
        }
        if contradicted {
            // A segment cut whose re-decode keeps disagreeing (a clipped or
            // differently decoded word right at the cut) would freeze the
            // preview and let the window grow without bound: undo it.
            if let Some((previous_start, previous_window, consumed)) = cut_undo {
                contradicted_after_cut += 1;
                if contradicted_after_cut >= STREAM_CUT_UNDO_PASSES {
                    stabilizer.restore_window(previous_window);
                    window_start = previous_start;
                    min_cut_consumed = consumed;
                    cut_undo = None;
                    contradicted_after_cut = 0;
                    log::info!(
                        target: "dictation",
                        "streaming segment cut undone after {STREAM_CUT_UNDO_PASSES} contradicting passes, window back at {:.2}s",
                        window_start as f32 / 16_000.0
                    );
                }
            }
            continue;
        }
        cut_undo = None;
        contradicted_after_cut = 0;

        // Move the window behind committed text. Archived words stay on
        // screen; later passes simply no longer decode their audio.
        let window_committed = stabilizer.window_committed_count();
        if language_settled && holdback == 0 && last_pending_empty && window_committed > 0 {
            // Speech pause and everything committed: the next window starts
            // behind the pause, where no word can be cut in half.
            log::debug!(
                target: "dictation",
                "streaming cut boundary: {}",
                stabilizer.window_boundary_words(window_committed)
            );
            stabilizer.archive_window(window_committed, false);
            window_start = speech_end_16k;
            voiced_at_cut = voiced_samples;
            pause_resume = Some(PauseResume::new(window_start));
            min_cut_consumed = 0;
            cut_happened = true;
            log::info!(
                target: "dictation",
                "streaming window cut at pause: {:.2}s, {window_committed} words archived",
                window_start as f32 / 16_000.0
            );
        } else if wants_segments
            && language_settled
            && let Some((consumed, end_cs)) =
                segment_cut(&skip_leading_tokens(&segments, overlap), window_committed)
            && consumed > min_cut_consumed
        {
            // Long speech without a pause: cut behind a fully committed
            // segment. The new window starts a little before Whisper's
            // segment end, in the quietest nearby frame, so no word of the
            // window is clipped; the overlap is skipped by alignment.
            let estimate = window_start + (end_cs.max(0) as usize) * SAMPLES_PER_CENTISECOND;
            let refined = quietest_frame(
                &audio16k,
                estimate.saturating_sub(CUT_OVERLAP_SAMPLES),
                CUT_SEARCH_RADIUS_SAMPLES,
            );
            log::debug!(
                target: "dictation",
                "streaming cut boundary: {}",
                stabilizer.window_boundary_words(consumed)
            );
            cut_undo = Some((window_start, stabilizer.window_state(), consumed));
            stabilizer.archive_window(consumed, true);
            window_start = refined.clamp(window_start + 1, speech_end_16k);
            min_cut_consumed = 0;
            cut_happened = true;
            // No new speech gate here: the window still holds words, and a
            // pause right after this cut must still commit their tail.
            log::info!(
                target: "dictation",
                "streaming window cut at segment: {:.2}s (whisper end {:.2}s), {consumed} words archived",
                window_start as f32 / 16_000.0,
                estimate as f32 / 16_000.0
            );
        }
    }
}

/// Where the window behind a pause cut starts: just before speech resumes,
/// not at the pause itself, so a long pause is not decoded as silence. A
/// stray sound that stays below the speech gate (a click) and is followed by
/// silence does not pin the start: the next voiced run moves it again, and
/// the sound no longer counts toward the gate.
struct PauseResume {
    /// Window start set by the cut (16 kHz index), the earliest start.
    cut_start: usize,
    /// A voiced run behind the pause started and has not ended yet.
    in_run: bool,
}

struct ResumeTick {
    voiced_samples: usize,
    /// Voiced samples at the previous tick.
    voiced_before: usize,
    /// Source samples before this tick's new ones arrived.
    source_before: usize,
    source_len: usize,
    last_voiced_end: usize,
    sample_rate: u32,
    audio16k_len: usize,
}

impl PauseResume {
    fn new(cut_start: usize) -> Self {
        Self {
            cut_start,
            in_run: false,
        }
    }

    /// Updates `window_start` and the gate's `voiced_at_cut` for one tick.
    /// Returns `false` once speech passed the gate and tracking is done.
    fn tick(
        &mut self,
        tick: &ResumeTick,
        window_start: &mut usize,
        voiced_at_cut: &mut usize,
    ) -> bool {
        let rate = tick.sample_rate;
        if tick.voiced_samples > tick.voiced_before && !self.in_run {
            // A voiced run started within this tick's samples.
            self.in_run = true;
            let resume = tick
                .source_before
                .saturating_sub(ms_to_samples(SPEECH_HEAD_PADDING_MS, rate));
            *window_start = self
                .cut_start
                .max(to_16khz_index(resume, rate).min(tick.audio16k_len));
        }
        let voiced_since_cut = tick.voiced_samples.saturating_sub(*voiced_at_cut);
        if voiced_gate_reached(voiced_since_cut, rate, STREAM_NEXT_WINDOW_MIN_VOICED_MS) {
            return false;
        }
        if self.in_run
            && tick.source_len.saturating_sub(tick.last_voiced_end)
                >= ms_to_samples(RESUME_RUN_GAP_MS, rate)
        {
            // Too short for speech and followed by silence: a stray sound.
            self.in_run = false;
            *voiced_at_cut = tick.voiced_samples;
        }
        true
    }
}

/// Joins the decoded segments like `collect_transcript` and returns, per
/// non-empty segment, its stabilizer token count and end timestamp (in
/// centiseconds relative to the window start). An end beyond the start of the
/// following segment is pulled back to that start, so a late end never puts
/// a cut behind words of the next segment.
fn collect_segments(state: &WhisperState) -> (String, Vec<(usize, i64)>) {
    let mut texts = Vec::new();
    let mut bounds: Vec<(usize, i64, i64)> = Vec::new();
    for segment in state.as_iter() {
        let text = segment.to_string().trim().to_owned();
        if text.is_empty() {
            continue;
        }
        bounds.push((
            token_count(&text),
            segment.start_timestamp(),
            segment.end_timestamp(),
        ));
        texts.push(text);
    }
    (texts.join(" "), segment_ends(&bounds))
}

/// `(tokens, start_cs, end_cs)` → `(tokens, end_cs)` with each end capped at
/// the next segment's start.
fn segment_ends(bounds: &[(usize, i64, i64)]) -> Vec<(usize, i64)> {
    bounds
        .iter()
        .enumerate()
        .map(|(index, &(tokens, _, end_cs))| {
            let next_start = bounds.get(index + 1).map_or(end_cs, |next| next.1);
            (tokens, end_cs.min(next_start))
        })
        .collect()
}

/// `segments` without their first `skip` tokens — the overlap with archived
/// text the stabilizer skipped, which must not count as committed window
/// words. A segment inside the overlap keeps its end time with zero tokens.
fn skip_leading_tokens(segments: &[(usize, i64)], mut skip: usize) -> Vec<(usize, i64)> {
    segments
        .iter()
        .map(|&(tokens, end_cs)| {
            let skipped = tokens.min(skip);
            skip -= skipped;
            (tokens - skipped, end_cs)
        })
        .collect()
}

/// Where a long window may be cut: behind the largest number of leading
/// segments — never the last one, Whisper still revises it — whose tokens are
/// all committed, with [`SEGMENT_CUT_ANCHOR_WORDS`] committed words left
/// behind them. Returns the archived token count and the end timestamp of the
/// last archived segment, or `None` when no such segment exists.
fn segment_cut(segments: &[(usize, i64)], committed: usize) -> Option<(usize, i64)> {
    let mut best = None;
    let mut cumulative = 0;
    for &(tokens, end_cs) in segments.iter().take(segments.len().saturating_sub(1)) {
        cumulative += tokens;
        if cumulative + SEGMENT_CUT_ANCHOR_WORDS > committed {
            break;
        }
        if cumulative > 0 {
            best = Some((cumulative, end_cs));
        }
    }
    best
}

/// Center of the quietest 10 ms frame within `radius` samples around
/// `center` — Whisper's segment times can be off by a few hundred ms, the cut
/// should land in the gap between words. Falls back to `center` (clamped to
/// the audio) when no complete frame fits.
fn quietest_frame(audio: &[f32], center: usize, radius: usize) -> usize {
    let from = center.saturating_sub(radius);
    let to = center.saturating_add(radius).min(audio.len());
    let mut best: Option<(f32, usize)> = None;
    let mut frame_start = from;
    while frame_start + FRAME_SAMPLES <= to {
        let frame = &audio[frame_start..frame_start + FRAME_SAMPLES];
        let energy: f32 = frame.iter().map(|sample| sample * sample).sum();
        if best.is_none_or(|(quietest, _)| energy < quietest) {
            best = Some((energy, frame_start + FRAME_SAMPLES / 2));
        }
        frame_start += FRAME_SAMPLES;
    }
    best.map_or(center.min(audio.len()), |(_, position)| position)
}

/// Source sample index → index in the 16 kHz stream.
fn to_16khz_index(index: usize, sample_rate: u32) -> usize {
    if sample_rate == 0 {
        return 0;
    }
    (index as u64 * 16_000 / u64::from(sample_rate)) as usize
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
/// live, and preview latency matters less than system responsiveness. On
/// Apple Silicon the GPU does the work: two threads were measured no slower
/// than four on Metal (#88), so the preview takes the minimum there.
fn streaming_thread_count() -> i32 {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return 2;
    }
    let cores = thread::available_parallelism()
        .map(|cores| cores.get())
        .unwrap_or(4);
    (cores / 2).clamp(2, 4) as i32
}

fn ms_to_samples(ms: u64, sample_rate: u32) -> usize {
    (sample_rate as u64 * ms / 1000) as usize
}

fn voiced_gate_reached(voiced_samples: usize, sample_rate: u32, min_voiced_ms: u64) -> bool {
    sample_rate > 0 && voiced_samples >= ms_to_samples(min_voiced_ms, sample_rate)
}

fn enough_new_audio(previous_len: usize, current_len: usize, sample_rate: u32) -> bool {
    current_len.saturating_sub(previous_len) >= ms_to_samples(STREAM_MIN_NEW_AUDIO_MS, sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voiced_gate_requires_600ms_of_speech() {
        let rate = 48_000;
        let gate = ms_to_samples(STREAM_FIRST_PASS_MIN_VOICED_MS, rate);
        assert_eq!(gate, 28_800);
        let first = STREAM_FIRST_PASS_MIN_VOICED_MS;
        assert!(!voiced_gate_reached(gate - 1, rate, first));
        assert!(voiced_gate_reached(gate, rate, first));
        assert!(
            !voiced_gate_reached(usize::MAX, 0, first),
            "zero rate never gates"
        );
        // Behind a pause, a short word is enough.
        assert!(voiced_gate_reached(
            14_400,
            rate,
            STREAM_NEXT_WINDOW_MIN_VOICED_MS
        ));
    }

    #[test]
    fn new_audio_gate_requires_250ms() {
        let rate = 16_000;
        assert!(!enough_new_audio(10_000, 10_000 + 3_999, rate));
        assert!(enough_new_audio(10_000, 10_000 + 4_000, rate));
    }

    #[test]
    fn segment_cut_takes_the_largest_committed_prefix_before_the_last_segment() {
        let segments = [(3, 250), (4, 610), (5, 900), (2, 1_100)];
        assert_eq!(segment_cut(&segments, 7), Some((3, 250)));
        assert_eq!(segment_cut(&segments, 11), Some((7, 610)));
        assert_eq!(segment_cut(&segments, 16), Some((12, 900)));
        // The last segment never goes, even when fully committed.
        assert_eq!(segment_cut(&segments, 30), Some((12, 900)));
        // Four committed words must stay behind the cut as an anchor.
        assert_eq!(segment_cut(&segments, 10), Some((3, 250)));
        assert_eq!(segment_cut(&segments, 6), None);
    }

    #[test]
    fn overlap_tokens_do_not_count_as_committed() {
        let segments = [(3, 250), (4, 610), (5, 900)];
        assert_eq!(skip_leading_tokens(&segments, 0), segments.to_vec());
        assert_eq!(
            skip_leading_tokens(&segments, 5),
            vec![(0, 250), (2, 610), (5, 900)]
        );
        // 5 overlap tokens + 4 committed: only the second segment's two
        // committed words (and the anchor) are behind the first cut point.
        let discounted = skip_leading_tokens(&segments, 5);
        assert_eq!(segment_cut(&discounted, 6), Some((2, 610)));
        assert_eq!(segment_cut(&discounted, 5), None);
    }

    #[test]
    fn segment_cut_needs_a_committed_segment_and_a_follower() {
        assert_eq!(segment_cut(&[(5, 400)], 5), None, "single segment");
        assert_eq!(
            segment_cut(&[(5, 400), (3, 700)], 4),
            None,
            "first not committed"
        );
        assert_eq!(segment_cut(&[], 10), None);
        // Empty leading segments carry no words to archive.
        assert_eq!(segment_cut(&[(0, 100), (3, 400)], 3), None);
    }

    /// Drives [`PauseResume`] at 16 kHz with 150 ms ticks: `voiced` marks
    /// the ticks whose audio counted as speech.
    fn run_resume(cut_start: usize, voiced: &[bool]) -> (usize, usize, bool) {
        let tick_len = 2_400;
        let mut resume = PauseResume::new(cut_start);
        let mut window_start = cut_start;
        let mut voiced_at_cut = 0;
        let (mut voiced_samples, mut source_len, mut last_voiced_end) = (0, cut_start, 0);
        let mut tracking = true;
        for &is_voiced in voiced {
            let voiced_before = voiced_samples;
            let source_before = source_len;
            source_len += tick_len;
            if is_voiced {
                voiced_samples += tick_len;
                last_voiced_end = source_len;
            }
            tracking = resume.tick(
                &ResumeTick {
                    voiced_samples,
                    voiced_before,
                    source_before,
                    source_len,
                    last_voiced_end,
                    sample_rate: 16_000,
                    audio16k_len: source_len,
                },
                &mut window_start,
                &mut voiced_at_cut,
            );
            if !tracking {
                break;
            }
        }
        (window_start, voiced_at_cut, tracking)
    }

    #[test]
    fn window_after_a_pause_starts_before_the_speech() {
        // 20 ticks (3 s) of silence, then speech.
        let mut ticks = vec![false; 20];
        ticks.extend([true; 3]);
        let (start, _, tracking) = run_resume(16_000, &ticks);
        // Speech began at 16_000 + 20 * 2_400 = 64_000; minus 500 ms.
        assert_eq!(start, 64_000 - 8_000);
        assert!(!tracking, "two voiced ticks pass the 300 ms gate");
    }

    #[test]
    fn stray_click_in_a_pause_does_not_pin_the_window_start() {
        // A click (one voiced tick), 1.5 s silence, then speech.
        let mut ticks = vec![false; 5];
        ticks.push(true);
        ticks.extend([false; 10]);
        ticks.extend([true; 3]);
        let (start, voiced_at_cut, _) = run_resume(0, &ticks);
        // Speech began at 16 * 2_400 = 38_400; minus 500 ms.
        assert_eq!(start, 38_400 - 8_000);
        assert_eq!(
            voiced_at_cut, 2_400,
            "the click no longer counts toward the gate"
        );
    }

    #[test]
    fn window_start_never_moves_before_the_cut() {
        let (start, _, _) = run_resume(50_000, &[true, true]);
        assert_eq!(start, 50_000);
    }

    #[test]
    fn segment_ends_are_capped_at_the_next_start() {
        let bounds = [(3, 0, 420), (4, 380, 900), (2, 950, 1_200)];
        assert_eq!(segment_ends(&bounds), vec![(3, 380), (4, 900), (2, 1_200)]);
        assert!(segment_ends(&[]).is_empty());
    }

    #[test]
    fn quietest_frame_finds_the_gap_between_tones() {
        let tone =
            |len: usize| -> Vec<f32> { (0..len).map(|i| (i as f32 * 0.3).sin() * 0.5).collect() };
        let mut audio = tone(8_000);
        audio.extend(vec![0.0; 800]); // 50 ms gap at 8_000..8_800
        audio.extend(tone(8_000));
        // Whisper's estimate is 150 ms off; the cut lands in the gap.
        let position = quietest_frame(&audio, 8_400 + 2_400, CUT_SEARCH_RADIUS_SAMPLES);
        assert!((8_000..8_800).contains(&position), "cut at {position}");
    }

    #[test]
    fn quietest_frame_stays_inside_the_audio() {
        let audio = vec![0.1; 1_000];
        assert!(quietest_frame(&audio, 0, CUT_SEARCH_RADIUS_SAMPLES) < audio.len());
        assert!(quietest_frame(&audio, 999, CUT_SEARCH_RADIUS_SAMPLES) < audio.len());
        // Center beyond the end and too little audio for one frame.
        assert_eq!(quietest_frame(&audio[..100], 5_000, 3_200), 100);
        assert_eq!(quietest_frame(&[], 10, 3_200), 0);
    }

    #[test]
    fn source_index_maps_to_16khz() {
        assert_eq!(to_16khz_index(48_000, 48_000), 16_000);
        assert_eq!(to_16khz_index(44_100, 44_100), 16_000);
        assert_eq!(to_16khz_index(5, 16_000), 5);
        assert_eq!(to_16khz_index(100, 0), 0);
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
    /// Generate the WAV with (any sample rate; 48 kHz exercises the resampler
    /// like a real microphone):
    /// `say -v Anna -o /tmp/speech-de.wav --data-format=LEI16@48000 "<German text>"`
    #[test]
    #[ignore]
    fn streaming_end_to_end_with_synthesized_speech() {
        use crate::dictation::{RecordingBuffer, WHISPER_FLASH_ATTN, whisper_context_params};

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

        let (samples, sample_rate) = read_wav_lei16_mono(std::path::Path::new(&wav_path));
        let rate = sample_rate as usize;
        assert!(samples.len() > rate * 5, "need at least 5 s of test speech");
        let clip_secs = samples.len() / rate;

        let context = Arc::new(
            WhisperContext::new_with_params(
                std::path::Path::new(&model_path),
                whisper_context_params(WHISPER_FLASH_ATTN),
            )
            .expect("test model loads"),
        );

        let buffer = Arc::new(Mutex::new(RecordingBuffer::new(
            sample_rate,
            false,
            0.014,
            900,
        )));
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
        let chunk_len = rate / 10 * 2;
        let mut fed = 0usize;
        let mut snapshots: Vec<StreamingTranscriptDto> = vec![snapshot_dto(&output)];
        let deadline = Instant::now() + Duration::from_secs(clip_secs as u64 / 2 + 30);
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
        while Instant::now() < settle_deadline && last_advance.elapsed() < Duration::from_secs(10) {
            buffer.lock().unwrap().push_chunk(&silence_chunk, &event_tx);
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
        eprintln!(
            "streaming revisions: {:?}",
            snapshots.iter().map(|s| s.revision).collect::<Vec<_>>()
        );
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
    /// Returns the samples and the sample rate from the `fmt ` chunk.
    fn read_wav_lei16_mono(path: &std::path::Path) -> (Vec<f32>, u32) {
        let bytes = std::fs::read(path).expect("test wav readable");
        let fmt_start = bytes
            .windows(4)
            .position(|window| window == b"fmt ")
            .expect("wav fmt chunk")
            + 8;
        let sample_rate = u32::from_le_bytes(
            bytes[fmt_start + 4..fmt_start + 8]
                .try_into()
                .expect("fmt sample rate"),
        );
        let data_start = bytes
            .windows(4)
            .position(|window| window == b"data")
            .expect("wav data chunk")
            + 8;
        let samples = bytes[data_start..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair) as f32 / 32_768.0)
            .collect();
        (samples, sample_rate)
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
