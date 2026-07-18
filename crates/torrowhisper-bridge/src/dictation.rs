use std::{
    collections::VecDeque,
    f32::consts::PI,
    path::PathBuf,
    sync::{
        Arc, Mutex, Once,
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use crate::audio_export;
use crate::model_manager::validated_model_path;
use crate::streaming_transcription::{
    self, SharedStreamingOutput, StreamingConfig, StreamingOutput, StreamingSession,
};
use cpal::{
    Device, FromSample, I24, Sample, SampleFormat, SizedSample, Stream, SupportedStreamConfig, U24,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use torrowhisper_core::{
    AppSettings, SYSTEM_DEFAULT_DEVICE_LABEL, StreamingTranscriptDto, TriggerMode,
};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

const CUE_NOTE_GAP_MS: u32 = 18;
const CUE_VOLUME: f32 = 0.12;
const RECORDING_START_NOTES: [(f32, u32); 2] = [(523.25, 60), (659.25, 92)];
const RECORDING_STOP_NOTES: [(f32, u32); 2] = [(659.25, 54), (523.25, 98)];
const RECORDING_CANCEL_NOTES: [(f32, u32); 3] = [(523.25, 50), (392.00, 60), (329.63, 110)];

pub enum DictationOutcome {
    Status(String),
    /// A finished Whisper transcript, plus the whisper-side latency breakdown
    /// and the anchors (`stop_instant`) the runtime needs to complete the
    /// per-dictation timing report once post-processing and insertion are done
    /// (#43). `timing` is `None` only when no measurement was seeded (defensive).
    TranscriptReady {
        transcript: String,
        timing: Option<DictationTiming>,
    },
    /// A dictation step failed and the user should see it (recording could
    /// not start, transcription errored, worker thread died). Unlike
    /// `Status`, this feeds the error indicator in the UI.
    Error(String),
    /// The recording is being saved to disk under this base name; the runtime
    /// should write the transcript under the same base name once it is ready,
    /// so audio and text files stay paired.
    PendingTranscriptSave(String),
}

/// Whisper-side latency of one dictation plus the wall-clock anchor the runtime
/// needs to finish the report (#43). Created when recording stops, completed by
/// the whisper worker, and handed to the runtime via `TranscriptReady`.
pub struct DictationTiming {
    /// The moment recording stopped; the runtime derives `total_after_stop`
    /// from this once the transcript is delivered.
    pub stop_instant: Instant,
    /// Length of the captured audio (basis for the real-time factor).
    pub audio_secs: f32,
    /// Model load time on this dictation — `0.0` on a warm cache hit.
    pub whisper_load_secs: f32,
    /// Resample of the captured audio to 16 kHz mono.
    pub resample_secs: f32,
    /// `WhisperContext::create_state`.
    pub state_secs: f32,
    /// Whisper inference (`state.full`).
    pub inference_secs: f32,
}

/// Whisper worker output: the transcript plus the stages the worker measured
/// itself (resample / state creation / inference).
struct TranscriptionOutput {
    text: String,
    resample_secs: f32,
    state_secs: f32,
    inference_secs: f32,
}

/// Timing anchors known at recording-stop time, held on the controller until
/// the whisper worker's result arrives and the two halves are merged.
struct TimingSeed {
    stop_instant: Instant,
    audio_secs: f32,
    whisper_load_secs: f32,
}

/// A completed background warmup: the model path and its loaded context.
type WarmupResult = Result<(PathBuf, Arc<WhisperContext>), String>;

pub struct DictationController {
    available_input_devices: Vec<String>,
    recording: Option<ActiveRecording>,
    transcription_rx: Option<Receiver<Result<TranscriptionOutput, String>>>,
    /// Timing anchors for the in-flight transcription, merged with the worker's
    /// stage timings when its result arrives.
    pending_timing_seed: Option<TimingSeed>,
    /// In-flight background model preload (#43). Delivers a loaded context that
    /// `poll` installs into `model_cache`, so the first dictation after startup
    /// or a model switch is already warm.
    warmup_rx: Option<Receiver<WarmupResult>>,
    model_cache: Option<ModelCache>,
    /// Live-transcription worker for the current recording (#41). `Some` only
    /// while a recording with live preview runs; taken (and joined by the
    /// final-pass thread) at stop, stop-flagged and detached on cancel/error.
    streaming: Option<StreamingSession>,
    /// The published live-transcript slot. Lives for the whole process so the
    /// overlay keeps its text through the final pass, and so the revision
    /// counter stays globally monotonic across sessions.
    streaming_output: SharedStreamingOutput,
    /// True between "recording with live preview stopped" and "final transcript
    /// arrived" — tells `poll` to publish the final text as `is_final`.
    streaming_finalize_pending: bool,
    dictation_blocked_at: Option<Instant>,
    active_input_device_name: String,
    last_mic_switch_message: String,
    mic_switch_event_count: u64,
}

impl DictationController {
    pub fn new() -> Self {
        Self {
            available_input_devices: Vec::new(),
            recording: None,
            transcription_rx: None,
            pending_timing_seed: None,
            warmup_rx: None,
            model_cache: None,
            streaming: None,
            streaming_output: Arc::new(Mutex::new(StreamingOutput::new())),
            streaming_finalize_pending: false,
            dictation_blocked_at: None,
            active_input_device_name: String::new(),
            last_mic_switch_message: String::new(),
            mic_switch_event_count: 0,
        }
    }

    pub fn active_input_device_name(&self) -> &str {
        &self.active_input_device_name
    }

    pub fn last_mic_switch_message(&self) -> &str {
        &self.last_mic_switch_message
    }

    pub fn mic_switch_event_count(&self) -> u64 {
        self.mic_switch_event_count
    }

    pub fn mark_blocked_now(&mut self) {
        self.dictation_blocked_at = Some(Instant::now());
    }

    pub fn clear_blocked(&mut self) {
        self.dictation_blocked_at = None;
    }

    pub fn is_blocked(&self, now: Instant, ttl: Duration) -> bool {
        match self.dictation_blocked_at {
            Some(at) => now.saturating_duration_since(at) < ttl,
            None => false,
        }
    }

    pub fn refresh_input_devices(&mut self, settings: &mut AppSettings) -> Vec<String> {
        let mut messages = Vec::new();

        match discover_input_devices() {
            Ok(devices) => {
                self.available_input_devices = devices;
                if self.available_input_devices.is_empty() {
                    messages.push("No input device found.".to_owned());
                    return messages;
                }

                if settings.input_device_name.trim().is_empty() {
                    settings.input_device_name = system_default_label().to_owned();
                }

                let resolved = resolve_input_device_name(settings, &self.available_input_devices);
                if settings.input_device_name != resolved {
                    settings.input_device_name = resolved.clone();
                }
                self.active_input_device_name = resolved;
            }
            Err(err) => messages.push(format!("Input devices could not be loaded: {err}")),
        }

        messages
    }

    pub fn available_input_devices(&self) -> &[String] {
        &self.available_input_devices
    }

    pub fn handle_device_change(&mut self, settings: &mut AppSettings) -> Option<MicSwitchEvent> {
        let new_list = match discover_input_devices() {
            Ok(list) => list,
            Err(_) => return None,
        };
        self.available_input_devices = new_list;

        let previous_active = self.active_input_device_name.clone();
        let new_active = resolve_input_device_name(settings, &self.available_input_devices);

        if new_active == previous_active && settings.input_device_name == new_active {
            return None;
        }

        if !settings.auto_switch_mic_on_hotplug && self.recording.is_none() {
            self.active_input_device_name = new_active.clone();
            settings.input_device_name = new_active;
            return None;
        }

        let swap_error = self
            .recording
            .as_mut()
            .and_then(|recording| recording.swap_device(&new_active).err());

        if let Some(err) = swap_error {
            self.stop_streaming();
            self.recording.take();
            self.last_mic_switch_message =
                format!("Mic switch failed: {err}. Recording stopped — please restart.");
            self.mic_switch_event_count = self.mic_switch_event_count.wrapping_add(1);
            self.active_input_device_name = new_active.clone();
            settings.input_device_name = new_active.clone();
            return Some(MicSwitchEvent {
                from: previous_active,
                to: new_active,
                was_recording: true,
                message: self.last_mic_switch_message.clone(),
            });
        }

        let was_recording = self.recording.is_some();
        self.active_input_device_name = new_active.clone();
        settings.input_device_name = new_active.clone();

        if new_active == previous_active {
            return None;
        }

        self.mic_switch_event_count = self.mic_switch_event_count.wrapping_add(1);
        let message = if previous_active.is_empty() {
            format!("Microphone active: {new_active}.")
        } else {
            format!("Microphone '{previous_active}' unavailable — using '{new_active}'.")
        };
        self.last_mic_switch_message = message.clone();

        Some(MicSwitchEvent {
            from: previous_active,
            to: new_active,
            was_recording,
            message,
        })
    }

    pub fn clear_mic_switch_message(&mut self) {
        self.last_mic_switch_message.clear();
    }

    pub fn summary(&self) -> String {
        let recording = if self.recording.is_some() {
            "Recording active"
        } else {
            "Recording inactive"
        };
        let transcription = if self.transcription_rx.is_some() {
            "Transcription in progress"
        } else {
            "no ongoing transcription"
        };
        format!("{recording}, {transcription}")
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn is_transcribing(&self) -> bool {
        self.transcription_rx.is_some()
    }

    pub fn current_levels(&self) -> Vec<f32> {
        self.recording
            .as_ref()
            .map(ActiveRecording::levels_snapshot)
            .unwrap_or_default()
    }

    pub fn invalidate_model_cache(&mut self) {
        self.model_cache = None;
    }

    /// True while a background model preload is in flight (#43).
    pub fn is_model_warming(&self) -> bool {
        self.warmup_rx.is_some()
    }

    /// Kicks off a background preload of the active model so the first dictation
    /// after startup or a model switch is already warm (#43). No-op when the
    /// model is already cached, a warmup is already running, or the model file
    /// is missing (a real dictation would surface that error instead).
    pub fn start_model_warmup(&mut self, settings: &AppSettings) {
        if self.warmup_rx.is_some() {
            return;
        }
        let Ok(model_path) = validated_model_path(settings) else {
            return;
        };
        if let Some(cache) = &self.model_cache
            && cache.path == model_path
        {
            return;
        }

        log_whisper_backend_info_once();
        let (tx, rx) = mpsc::channel();
        let path_for_thread = model_path.clone();
        let label = settings.local_model.display_label().to_owned();
        let settings_for_thread = settings.clone();
        thread::spawn(move || {
            let started = Instant::now();
            let path_string = path_for_thread.to_string_lossy().to_string();
            let result = WhisperContext::new_with_params(
                &path_string,
                WhisperContextParameters::default(),
            )
            .map(|context| {
                log::info!(
                    target: "dictation",
                    "model warmup complete in {:.1}s ({label})",
                    started.elapsed().as_secs_f32()
                );
                let context = Arc::new(context);
                warm_inference_graph(&context, &settings_for_thread);
                (path_for_thread, context)
            })
            .map_err(|err| {
                log::warn!(target: "dictation", "model warmup failed ({label}): {err}");
                format!("Whisper model warmup failed: {err}")
            });
            let _ = tx.send(result);
        });
        self.warmup_rx = Some(rx);
    }

    /// Installs a completed warmup context into the cache. Called from `poll`.
    fn drain_warmup(&mut self, settings: &AppSettings) {
        let Some(rx) = &self.warmup_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok((path, context))) => {
                self.warmup_rx = None;
                // Only install if still relevant (model may have changed while
                // warming) and nothing newer is cached.
                let already_current = self
                    .model_cache
                    .as_ref()
                    .is_some_and(|cache| cache.path == path);
                if !already_current {
                    self.model_cache = Some(ModelCache { path, context });
                }
                // Late spawn (#41): a dictation begun while the model was still
                // warming skipped its live preview; start it now that the
                // context is here.
                if self.recording.is_some()
                    && self.streaming.is_none()
                    && live_transcription_active(settings)
                    && let Ok(model_path) = validated_model_path(settings)
                {
                    self.start_streaming_session(settings, &model_path);
                }
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                self.warmup_rx = None;
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Starts the live-transcription worker for the running recording (#41).
    /// Sessions only start on a warm model cache — loading models is the job
    /// of the background warmup and the final pass. A cold first take simply
    /// skips the preview (`drain_warmup` late-spawns it once the context
    /// arrives).
    fn start_streaming_session(&mut self, settings: &AppSettings, model_path: &PathBuf) {
        if self.streaming.is_some() {
            return;
        }
        let Some(buffer) = self
            .recording
            .as_ref()
            .map(|recording| recording.shared.clone())
        else {
            return;
        };
        let Some(context) = self
            .model_cache
            .as_ref()
            .filter(|cache| cache.path == *model_path)
            .map(|cache| cache.context.clone())
        else {
            log::info!(
                target: "dictation",
                "live transcription skipped: model not warmed up yet"
            );
            return;
        };

        self.streaming = Some(streaming_transcription::spawn(StreamingConfig {
            buffer,
            output: self.streaming_output.clone(),
            context,
            settings: settings.clone(),
            language: normalized_language(&settings.transcription_language),
        }));
    }

    /// Stops and detaches the live-preview worker. For paths where no final
    /// pass follows (cancel, stream errors) — the worker exits within one
    /// tick / one aborted inference on its own.
    fn stop_streaming(&mut self) {
        if let Some(session) = self.streaming.take() {
            session.request_stop();
        }
    }

    /// Snapshot of the live transcript for the polling FFI endpoint.
    pub fn streaming_transcript(&self) -> StreamingTranscriptDto {
        streaming_transcription::snapshot_dto(&self.streaming_output)
    }

    pub fn handle_hotkey(
        &mut self,
        settings: &AppSettings,
        pressed: bool,
    ) -> Vec<DictationOutcome> {
        match settings.trigger_mode {
            TriggerMode::PushToTalk => {
                if pressed {
                    match self.start_recording(settings) {
                        Ok(message) => vec![DictationOutcome::Status(message)],
                        Err(err) => vec![DictationOutcome::Error(err)],
                    }
                } else {
                    match self.stop_recording_and_transcribe(
                        settings,
                        "key released",
                        RecordingCue::Stop,
                    ) {
                        Ok(outcomes) => outcomes,
                        Err(err) => vec![DictationOutcome::Error(err)],
                    }
                }
            }
            TriggerMode::Toggle => {
                if !pressed {
                    return Vec::new();
                }

                if self.is_recording() {
                    match self.stop_recording_and_transcribe(
                        settings,
                        "toggle stopped",
                        RecordingCue::Stop,
                    ) {
                        Ok(outcomes) => outcomes,
                        Err(err) => vec![DictationOutcome::Error(err)],
                    }
                } else {
                    match self.start_recording(settings) {
                        Ok(message) => vec![DictationOutcome::Status(message)],
                        Err(err) => vec![DictationOutcome::Error(err)],
                    }
                }
            }
        }
    }

    pub fn start_recording(&mut self, settings: &AppSettings) -> Result<String, String> {
        if self.recording.is_some() {
            return Ok("Recording already in progress.".to_owned());
        }

        // Same validation as ensure_whisper_context, so a recording never
        // starts when the later transcription would fail to find the model.
        let model_path = match validated_model_path(settings) {
            Ok(path) => path,
            Err(err) => {
                self.mark_blocked_now();
                return Err(format!("Recording blocked: {err}"));
            }
        };

        let resolved_name = resolve_input_device_name(settings, &self.available_input_devices);
        let (recording, used_name) = ActiveRecording::start(settings, &resolved_name)?;
        self.recording = Some(recording);
        self.active_input_device_name = used_name.clone();
        self.clear_blocked();
        // Every session starts with a cleared live-transcript slot, whether or
        // not a worker spawns — the overlay must never show a previous take.
        streaming_transcription::reset_output(&self.streaming_output);
        if live_transcription_active(settings) {
            self.start_streaming_session(settings, &model_path);
        }
        play_recording_cue(RecordingCue::Start);
        log::info!(
            target: "dictation",
            "recording started via '{used_name}' (vad: {})",
            settings.vad_enabled
        );

        Ok(format!(
            "Recording started via '{}'{}.",
            used_name,
            if settings.vad_enabled {
                ", silence stop active"
            } else {
                ", manual stop active"
            }
        ))
    }

    pub fn stop_recording_and_transcribe(
        &mut self,
        settings: &AppSettings,
        reason: &str,
        cue: RecordingCue,
    ) -> Result<Vec<DictationOutcome>, String> {
        // Flag the live-preview worker first: its abort callback ends any
        // in-flight streaming pass while we do the post-stop bookkeeping, and
        // the final-pass thread joins it below before its own inference.
        let streaming = self.streaming.take();
        if let Some(session) = &streaming {
            session.request_stop();
        }
        self.streaming_finalize_pending = false;

        let Some(recording) = self.recording.take() else {
            return Ok(Vec::new());
        };

        // Anchor the "total after stop" stopwatch at the very moment the user
        // stopped recording, before any post-stop work (#43).
        let stop_instant = Instant::now();
        let audio = recording.finish()?;
        play_recording_cue(cue);
        if audio.samples.is_empty() || audio.duration < Duration::from_millis(200) {
            return Ok(vec![DictationOutcome::Status(
                "Recording was too short or empty.".to_owned(),
            )]);
        }

        // Optional on-disk save (never for cancelled dictations). The MP3 is
        // encoded off-thread from a clone of the samples; the matching base name
        // is handed to the runtime so the transcript file is written alongside.
        let mut outcomes: Vec<DictationOutcome> = Vec::new();
        if !matches!(cue, RecordingCue::Cancel) && audio_export::saving_enabled(settings) {
            let base = audio_export::base_name(now_unix_secs());
            if let Some(mp3_path) = audio_export::audio_destination(settings, &base) {
                let samples = audio.samples.clone();
                let rate = audio.sample_rate;
                thread::spawn(
                    move || match audio_export::write_mp3(&samples, rate, &mp3_path) {
                        Ok(()) => {
                            log::info!(target: "dictation", "saved recording to {}", mp3_path.display())
                        }
                        Err(err) => log::warn!(target: "dictation", "MP3 export failed: {err}"),
                    },
                );
            }
            outcomes.push(DictationOutcome::PendingTranscriptSave(base));
        }

        let (context, whisper_load_secs) = self.ensure_whisper_context(settings)?;
        let language = normalized_language(&settings.transcription_language);
        let app_settings = settings.clone();
        let (tx, rx) = mpsc::channel();

        let audio_duration = audio.duration;
        log::info!(
            target: "dictation",
            "recording stopped ({reason}): {:.1}s audio, starting transcription with '{}'",
            audio_duration.as_secs_f32(),
            settings.local_model.display_label()
        );

        self.streaming_finalize_pending = streaming.is_some();
        thread::spawn(move || {
            // Hard no-overlap guarantee: wait for the streaming worker to
            // finish (its current pass aborts via the stop flag, so this is
            // quick) before the final inference starts.
            if let Some(session) = streaming {
                session.join();
            }
            let started = Instant::now();
            let result =
                transcribe_with_whisper(context, &app_settings, audio, language.as_deref());
            match &result {
                Ok(output) => log::info!(
                    target: "dictation",
                    "transcription finished in {:.2}s ({} chars from {:.1}s audio; \
                     resample {:.2}s, state {:.2}s, inference {:.2}s)",
                    started.elapsed().as_secs_f32(),
                    output.text.chars().count(),
                    audio_duration.as_secs_f32(),
                    output.resample_secs,
                    output.state_secs,
                    output.inference_secs
                ),
                Err(err) => log::error!(
                    target: "dictation",
                    "transcription failed after {:.2}s: {err}",
                    started.elapsed().as_secs_f32()
                ),
            }
            let _ = tx.send(result);
        });

        self.transcription_rx = Some(rx);
        self.pending_timing_seed = Some(TimingSeed {
            stop_instant,
            audio_secs: audio_duration.as_secs_f32(),
            whisper_load_secs,
        });

        outcomes.push(DictationOutcome::Status(format!(
            "Recording stopped ({reason}). Local transcription in progress."
        )));
        Ok(outcomes)
    }

    pub fn cancel_recording(&mut self) -> bool {
        self.stop_streaming();
        if self.recording.take().is_some() {
            play_recording_cue(RecordingCue::Cancel);
            true
        } else {
            false
        }
    }

    pub fn abandon_transcription(&mut self) -> bool {
        self.transcription_rx.take().is_some()
    }

    pub fn poll(&mut self, settings: &mut AppSettings) -> Vec<DictationOutcome> {
        let mut outcomes = Vec::new();

        self.drain_warmup(settings);

        let pending_recording_event = self
            .recording
            .as_mut()
            .and_then(ActiveRecording::poll_event);
        match pending_recording_event {
            Some(RecordingEvent::SilenceDetected) => {
                match self.stop_recording_and_transcribe(
                    settings,
                    "silence detected",
                    RecordingCue::Stop,
                ) {
                    Ok(new_outcomes) => outcomes.extend(new_outcomes),
                    Err(err) => outcomes.push(DictationOutcome::Error(err)),
                }
            }
            Some(RecordingEvent::StreamError(err)) => {
                let switch = self.handle_device_change(settings);
                match switch {
                    Some(event) => {
                        outcomes.push(DictationOutcome::Status(event.message));
                    }
                    None => {
                        self.stop_streaming();
                        self.recording.take();
                        outcomes.push(DictationOutcome::Error(err));
                    }
                }
            }
            None => {}
        }

        if let Some(rx) = &self.transcription_rx {
            match rx.try_recv() {
                Ok(Ok(output)) => {
                    self.transcription_rx = None;
                    if std::mem::take(&mut self.streaming_finalize_pending) {
                        // The overlay shows the raw Whisper text as final;
                        // insertion still uses the post-processed pipeline
                        // output via the outcomes below.
                        streaming_transcription::publish_final(
                            &self.streaming_output,
                            &output.text,
                        );
                    }
                    let timing = self.pending_timing_seed.take().map(|seed| DictationTiming {
                        stop_instant: seed.stop_instant,
                        audio_secs: seed.audio_secs,
                        whisper_load_secs: seed.whisper_load_secs,
                        resample_secs: output.resample_secs,
                        state_secs: output.state_secs,
                        inference_secs: output.inference_secs,
                    });
                    outcomes.push(DictationOutcome::Status(
                        "Local transcription complete.".to_owned(),
                    ));
                    outcomes.push(DictationOutcome::TranscriptReady {
                        transcript: output.text,
                        timing,
                    });
                }
                Ok(Err(err)) => {
                    self.transcription_rx = None;
                    self.pending_timing_seed = None;
                    if std::mem::take(&mut self.streaming_finalize_pending) {
                        streaming_transcription::mark_final_unchanged(&self.streaming_output);
                    }
                    outcomes.push(DictationOutcome::Error(err));
                }
                Err(TryRecvError::Disconnected) => {
                    // The worker thread died without sending a result — most
                    // likely a panic inside whisper. The panic hook has the
                    // details in the log.
                    self.transcription_rx = None;
                    self.pending_timing_seed = None;
                    if std::mem::take(&mut self.streaming_finalize_pending) {
                        streaming_transcription::mark_final_unchanged(&self.streaming_output);
                    }
                    outcomes.push(DictationOutcome::Error(
                        "Transcription worker stopped unexpectedly.".to_owned(),
                    ));
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        outcomes
    }

    /// Returns the whisper context for the active model plus the time spent
    /// loading it — `0.0` on a warm cache hit, the real load duration on a miss.
    /// The load time is reported separately from inference so a cold first
    /// dictation is never mistaken for slow transcription (#43).
    fn ensure_whisper_context(
        &mut self,
        settings: &AppSettings,
    ) -> Result<(Arc<WhisperContext>, f32), String> {
        let model_path = validated_model_path(settings)?;

        if let Some(cache) = &self.model_cache
            && cache.path == model_path
        {
            return Ok((cache.context.clone(), 0.0));
        }

        log_whisper_backend_info_once();
        let model_path_string = model_path.to_string_lossy().to_string();
        let load_started = Instant::now();
        let context = WhisperContext::new_with_params(
            &model_path_string,
            WhisperContextParameters::default(),
        )
        .map_err(|err| {
            log::error!(
                target: "dictation",
                "whisper model load failed ({model_path_string}): {err}"
            );
            format!("Whisper model could not be loaded: {err}")
        })?;
        let load_secs = load_started.elapsed().as_secs_f32();
        log::info!(
            target: "dictation",
            "whisper model loaded in {load_secs:.1}s ({model_path_string})"
        );

        let context = Arc::new(context);
        self.model_cache = Some(ModelCache {
            path: model_path,
            context: context.clone(),
        });

        Ok((context, load_secs))
    }
}

struct ModelCache {
    path: PathBuf,
    context: Arc<WhisperContext>,
}

struct ActiveRecording {
    stream: Option<Stream>,
    event_tx: Sender<RecordingEvent>,
    event_rx: Receiver<RecordingEvent>,
    shared: Arc<Mutex<RecordingBuffer>>,
    started_at: Instant,
    sample_rate: u32,
    current_device_name: String,
}

impl ActiveRecording {
    fn start(settings: &AppSettings, resolved_device_name: &str) -> Result<(Self, String), String> {
        let (device, used_name) = select_input_device_for_recording(resolved_device_name)?;
        let config = device
            .default_input_config()
            .map_err(|err| format!("Input configuration could not be loaded: {err}"))?;

        let sample_rate = config.sample_rate();
        let channels = config.channels() as usize;
        let (event_tx, event_rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(RecordingBuffer::new(
            sample_rate,
            settings.vad_enabled,
            settings.vad_threshold,
            settings.vad_silence_ms,
        )));

        let stream =
            build_input_stream(&device, &config, channels, shared.clone(), event_tx.clone())?;
        stream
            .play()
            .map_err(|err| format!("Audio recording could not be started: {err}"))?;

        Ok((
            Self {
                stream: Some(stream),
                event_tx,
                event_rx,
                shared,
                started_at: Instant::now(),
                sample_rate,
                current_device_name: used_name.clone(),
            },
            used_name,
        ))
    }

    fn swap_device(&mut self, new_resolved_name: &str) -> Result<(), String> {
        if new_resolved_name == self.current_device_name {
            return Ok(());
        }

        self.stream.take();

        let (device, used_name) = select_input_device_for_recording(new_resolved_name)?;
        let config = device
            .default_input_config()
            .map_err(|err| format!("Input configuration could not be loaded: {err}"))?;

        if config.sample_rate() != self.sample_rate {
            return Err(format!(
                "Sample rate mismatch: recording at {} Hz, '{}' offers {} Hz",
                self.sample_rate,
                used_name,
                config.sample_rate()
            ));
        }

        let channels = config.channels() as usize;
        let stream = build_input_stream(
            &device,
            &config,
            channels,
            self.shared.clone(),
            self.event_tx.clone(),
        )?;
        stream
            .play()
            .map_err(|err| format!("Audio recording could not be restarted: {err}"))?;

        self.stream = Some(stream);
        self.current_device_name = used_name;
        Ok(())
    }

    fn poll_event(&mut self) -> Option<RecordingEvent> {
        self.event_rx.try_recv().ok()
    }

    fn levels_snapshot(&self) -> Vec<f32> {
        self.shared
            .lock()
            .map(|guard| guard.levels_snapshot())
            .unwrap_or_default()
    }

    fn finish(self) -> Result<RecordedAudio, String> {
        let duration = self.started_at.elapsed();
        let mut guard = self
            .shared
            .lock()
            .map_err(|_| "Recording buffer could not be read.".to_owned())?;
        Ok(guard.finish(duration))
    }
}

#[derive(Debug, Clone)]
pub struct MicSwitchEvent {
    pub from: String,
    pub to: String,
    pub was_recording: bool,
    pub message: String,
}

pub(crate) enum RecordingEvent {
    SilenceDetected,
    StreamError(String),
}

const LEVEL_HISTORY_CAPACITY: usize = 120;
/// Upper bound for the tracked noise floor — also its start value, so the
/// floor only ever falls toward the microphone's real silence level.
const NOISE_FLOOR_CEILING: f32 = 0.02;

pub(crate) struct RecordingBuffer {
    samples: Vec<f32>,
    sample_rate: u32,
    vad_enabled: bool,
    vad_threshold: f32,
    silence_limit_samples: usize,
    silence_run_samples: usize,
    voice_detected: bool,
    silence_notification_sent: bool,
    /// Total samples whose chunk counted as speech for the live preview — a
    /// cheap "how much actual speech was heard" counter. Gates the first
    /// streaming pass (#41) so silence-only takes never run inference.
    voiced_samples: usize,
    /// Sample index just past the most recent speech chunk. The streaming
    /// worker truncates its snapshots here (+ a little padding) so Whisper
    /// never decodes into trailing silence — that is where it hallucinates
    /// filler ("Vielen Dank", repeated words) which must never commit.
    last_voiced_end: usize,
    /// Fast-fall / slow-rise tracker of the quietest recent chunk RMS. Lets
    /// the streaming gate adapt to the microphone's actual noise floor
    /// instead of relying on the absolute VAD threshold alone.
    noise_floor: f32,
    level_history: VecDeque<f32>,
}

impl RecordingBuffer {
    pub(crate) fn new(
        sample_rate: u32,
        vad_enabled: bool,
        vad_threshold: f32,
        vad_silence_ms: u32,
    ) -> Self {
        let silence_limit_samples =
            ((sample_rate as u64 * vad_silence_ms as u64) / 1000).max(1) as usize;

        Self {
            samples: Vec::new(),
            sample_rate,
            vad_enabled,
            vad_threshold,
            silence_limit_samples,
            silence_run_samples: 0,
            voice_detected: false,
            silence_notification_sent: false,
            voiced_samples: 0,
            last_voiced_end: 0,
            noise_floor: NOISE_FLOOR_CEILING,
            level_history: VecDeque::with_capacity(LEVEL_HISTORY_CAPACITY),
        }
    }

    pub(crate) fn push_chunk(&mut self, chunk: &[f32], event_tx: &Sender<RecordingEvent>) {
        if chunk.is_empty() {
            return;
        }

        self.samples.extend_from_slice(chunk);

        let rms = root_mean_square(chunk);
        if self.level_history.len() == LEVEL_HISTORY_CAPACITY {
            self.level_history.pop_front();
        }
        self.level_history.push_back(rms);

        // Track the quietest recent level: drop instantly, recover slowly.
        self.noise_floor = if rms < self.noise_floor {
            rms
        } else {
            (self.noise_floor * 1.02).min(NOISE_FLOOR_CEILING)
        };
        if self.is_voiced_for_streaming(rms) {
            self.voiced_samples += chunk.len();
            self.last_voiced_end = self.samples.len();
        }

        if rms >= self.vad_threshold {
            self.voice_detected = true;
            self.silence_run_samples = 0;
            self.silence_notification_sent = false;
            return;
        }

        if self.vad_enabled && self.voice_detected {
            self.silence_run_samples += chunk.len();
            if self.silence_run_samples >= self.silence_limit_samples
                && !self.silence_notification_sent
            {
                self.silence_notification_sent = true;
                let _ = event_tx.send(RecordingEvent::SilenceDetected);
            }
        }
    }

    fn levels_snapshot(&self) -> Vec<f32> {
        self.level_history.iter().copied().collect()
    }

    /// Speech test for the live-preview gate (#41). The auto-stop VAD
    /// threshold alone proved far too strict on low-gain microphones — quiet
    /// but perfectly transcribable speech hovered below it and the preview
    /// stayed on "Listening…" for a minute. A chunk counts as speech when it
    /// crosses a fraction of the VAD threshold OR clearly rises above the
    /// recording's own noise floor, whichever is more permissive. The
    /// auto-stop VAD keeps using the strict threshold unchanged.
    ///
    /// Calibration: 5× the floor with a 0.0035 minimum — 3×/0.002 proved too
    /// permissive in the field (ordinary room ambience counted as speech and
    /// the worker ran inference on silence).
    fn is_voiced_for_streaming(&self, rms: f32) -> bool {
        const MIN_VOICED_RMS: f32 = 0.0035;
        let above_absolute = rms >= self.vad_threshold * 0.35;
        let above_floor = rms >= (self.noise_floor * 5.0).max(MIN_VOICED_RMS);
        above_absolute || above_floor
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn voiced_samples(&self) -> usize {
        self.voiced_samples
    }

    pub(crate) fn last_voiced_end(&self) -> usize {
        self.last_voiced_end
    }

    /// Appends `samples[from..]` to `into` — the streaming worker's
    /// incremental snapshot, so the callback-shared lock is only held for the
    /// new tail. A `from` beyond the current length is a no-op (after
    /// `finish()` the samples are taken and the buffer is empty).
    pub(crate) fn copy_new_samples(&self, from: usize, into: &mut Vec<f32>) {
        if let Some(new_samples) = self.samples.get(from..) {
            into.extend_from_slice(new_samples);
        }
    }

    fn finish(&mut self, duration: Duration) -> RecordedAudio {
        let last_voiced_end = self.last_voiced_end;
        RecordedAudio {
            samples: std::mem::take(&mut self.samples),
            sample_rate: self.sample_rate,
            duration,
            last_voiced_end,
        }
    }
}

struct RecordedAudio {
    samples: Vec<f32>,
    sample_rate: u32,
    duration: Duration,
    /// Sample index (at `sample_rate`) just past the last chunk that counted as
    /// speech. The final pass decodes only up to here (+ a short padding) so it
    /// never runs Whisper over the trailing silence — that is where the model
    /// hallucinates subtitle-style filler ("Vielen Dank", "Bis zum nächsten Mal
    /// im ZDF"). Zero means no speech was ever detected; then the whole buffer
    /// is kept (better a rare hallucination than dropping a quiet real take).
    last_voiced_end: usize,
}

fn build_input_stream(
    device: &Device,
    config: &SupportedStreamConfig,
    channels: usize,
    shared: Arc<Mutex<RecordingBuffer>>,
    event_tx: Sender<RecordingEvent>,
) -> Result<Stream, String> {
    let stream_config = config.config();
    let error_sender = event_tx.clone();
    let error_callback = move |err| {
        let _ = error_sender.send(RecordingEvent::StreamError(format!(
            "Audio error in stream: {err}"
        )));
    };

    match config.sample_format() {
        SampleFormat::F32 => device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _| handle_input_data_f32(data, channels, &shared, &event_tx),
                error_callback,
                None,
            )
            .map_err(|err| err.to_string()),
        SampleFormat::F64 => device
            .build_input_stream(
                &stream_config,
                move |data: &[f64], _| {
                    handle_input_data_iter(
                        data.iter().copied().map(|sample| sample as f32),
                        channels,
                        &shared,
                        &event_tx,
                    )
                },
                error_callback,
                None,
            )
            .map_err(|err| err.to_string()),
        SampleFormat::I8 => device
            .build_input_stream(
                &stream_config,
                move |data: &[i8], _| {
                    handle_input_data_iter(
                        data.iter()
                            .copied()
                            .map(|sample| sample as f32 / i8::MAX as f32),
                        channels,
                        &shared,
                        &event_tx,
                    )
                },
                error_callback,
                None,
            )
            .map_err(|err| err.to_string()),
        SampleFormat::I16 => device
            .build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    handle_input_data_iter(
                        data.iter()
                            .copied()
                            .map(|sample| sample as f32 / i16::MAX as f32),
                        channels,
                        &shared,
                        &event_tx,
                    )
                },
                error_callback,
                None,
            )
            .map_err(|err| err.to_string()),
        SampleFormat::I32 => device
            .build_input_stream(
                &stream_config,
                move |data: &[i32], _| {
                    handle_input_data_iter(
                        data.iter()
                            .copied()
                            .map(|sample| sample as f32 / i32::MAX as f32),
                        channels,
                        &shared,
                        &event_tx,
                    )
                },
                error_callback,
                None,
            )
            .map_err(|err| err.to_string()),
        SampleFormat::U8 => device
            .build_input_stream(
                &stream_config,
                move |data: &[u8], _| {
                    handle_input_data_iter(
                        data.iter()
                            .copied()
                            .map(|sample| (sample as f32 / u8::MAX as f32) * 2.0 - 1.0),
                        channels,
                        &shared,
                        &event_tx,
                    )
                },
                error_callback,
                None,
            )
            .map_err(|err| err.to_string()),
        SampleFormat::U16 => device
            .build_input_stream(
                &stream_config,
                move |data: &[u16], _| {
                    handle_input_data_iter(
                        data.iter()
                            .copied()
                            .map(|sample| (sample as f32 / u16::MAX as f32) * 2.0 - 1.0),
                        channels,
                        &shared,
                        &event_tx,
                    )
                },
                error_callback,
                None,
            )
            .map_err(|err| err.to_string()),
        SampleFormat::U32 => device
            .build_input_stream(
                &stream_config,
                move |data: &[u32], _| {
                    handle_input_data_iter(
                        data.iter()
                            .copied()
                            .map(|sample| (sample as f32 / u32::MAX as f32) * 2.0 - 1.0),
                        channels,
                        &shared,
                        &event_tx,
                    )
                },
                error_callback,
                None,
            )
            .map_err(|err| err.to_string()),
        other => Err(format!(
            "Sample format '{other}' is not currently supported."
        )),
    }
}

fn handle_input_data_f32(
    data: &[f32],
    channels: usize,
    shared: &Arc<Mutex<RecordingBuffer>>,
    event_tx: &Sender<RecordingEvent>,
) {
    let mono_chunk = interleaved_to_mono(data, channels);
    if let Ok(mut guard) = shared.lock() {
        guard.push_chunk(&mono_chunk, event_tx);
    }
}

fn handle_input_data_iter(
    data: impl Iterator<Item = f32>,
    channels: usize,
    shared: &Arc<Mutex<RecordingBuffer>>,
    event_tx: &Sender<RecordingEvent>,
) {
    let collected: Vec<f32> = data.collect();
    handle_input_data_f32(&collected, channels, shared, event_tx);
}

fn interleaved_to_mono(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }

    let mut mono = Vec::with_capacity(data.len() / channels);
    for frame in data.chunks(channels) {
        let sum: f32 = frame.iter().copied().sum();
        mono.push(sum / channels as f32);
    }
    mono
}

fn root_mean_square(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let power = samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
    power.sqrt()
}

fn transcribe_with_whisper(
    context: Arc<WhisperContext>,
    settings: &AppSettings,
    audio: RecordedAudio,
    language: Option<&str>,
) -> Result<TranscriptionOutput, String> {
    // Discard trailing silence before decoding: Whisper hallucinates subtitle-
    // style filler when it decodes into (near-)silent audio, and that garbage
    // ("Vielen Dank", "Untertitel von …", "Bis zum nächsten Mal im ZDF") would
    // otherwise be appended to every take that ends with a pause. The live
    // preview already truncates the same way — this brings the authoritative
    // final pass in line so the inserted text matches what the user saw.
    let speech = trim_trailing_silence(&audio.samples, audio.sample_rate, audio.last_voiced_end);
    let trimmed_secs =
        audio.samples.len().saturating_sub(speech.len()) as f32 / audio.sample_rate.max(1) as f32;
    if trimmed_secs >= 0.1 {
        log::info!(
            target: "dictation",
            "trimmed {trimmed_secs:.1}s trailing silence before the final pass"
        );
    }

    let resample_started = Instant::now();
    let mono_16khz = resample_to_16khz(speech, audio.sample_rate);
    let resample_secs = resample_started.elapsed().as_secs_f32();
    if mono_16khz.is_empty() {
        return Err("No audio data available for Whisper.".to_owned());
    }

    let n_threads = resolve_thread_count(settings.whisper_thread_count);
    let inference = run_whisper_inference(&context, &mono_16khz, settings, language, n_threads)?;

    // Belt-and-suspenders over the silence trim: a short residual pause can
    // still trigger a hallucinated subtitle/broadcast phrase. Strip those known
    // artifacts from the tail (never legitimate mid-text content — see
    // strip_hallucinated_tail).
    let text = strip_hallucinated_tail(&inference.text);
    if text.is_empty() {
        return Err(format!(
            "Whisper recognized no text. Model: {}, language: {}.",
            settings.local_model.default_filename(),
            language.unwrap_or("auto")
        ));
    }

    Ok(TranscriptionOutput {
        text,
        resample_secs,
        state_secs: inference.state_secs,
        inference_secs: inference.inference_secs,
    })
}

/// Result of one Whisper decode over already-resampled 16 kHz mono samples.
pub(crate) struct WhisperInference {
    pub text: String,
    pub state_secs: f32,
    pub inference_secs: f32,
}

/// Runs Whisper over 16 kHz mono `samples` and returns the transcript plus the
/// state-creation and inference timings. Shared by the live dictation path and
/// the benchmark engine so both exercise identical decoding parameters (#43).
/// `n_threads` is passed in so the benchmark can sweep thread counts without
/// mutating settings.
pub(crate) fn run_whisper_inference(
    context: &WhisperContext,
    samples: &[f32],
    settings: &AppSettings,
    language: Option<&str>,
    n_threads: i32,
) -> Result<WhisperInference, String> {
    let state_started = Instant::now();
    let mut state = context
        .create_state()
        .map_err(|err| format!("Whisper state could not be created: {err}"))?;
    let state_secs = state_started.elapsed().as_secs_f32();

    let params = base_full_params(settings, language, n_threads);

    // Serialize all Whisper inference process-wide. Today the paths (a live
    // dictation, a benchmark run) never overlap by design, but nothing enforced
    // it. A single coordinated worker keeps GPU/CPU pressure predictable and
    // makes the pipeline safe for a future streaming mode where a final pass
    // could otherwise race an in-flight chunk (#43, streaming prep). The lock
    // is held only around inference, not model loading or resampling.
    let inference_started = Instant::now();
    let _inference_guard = whisper_inference_lock();
    state
        .full(params, samples)
        .map_err(|err| format!("Whisper transcription failed: {err}"))?;
    drop(_inference_guard);
    let inference_secs = inference_started.elapsed().as_secs_f32();

    let transcript = collect_transcript(&state);

    Ok(WhisperInference {
        text: transcript,
        state_secs,
        inference_secs,
    })
}

/// Decoding parameters shared by the final dictation pass, the benchmark and
/// the streaming preview (#41), so their behavior can never drift apart.
pub(crate) fn base_full_params<'lang>(
    settings: &AppSettings,
    language: Option<&'lang str>,
    n_threads: i32,
) -> FullParams<'lang, 'static> {
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });
    params.set_n_threads(n_threads);
    params.set_translate(false);
    params.set_language(language);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_timestamps(true);
    params.set_single_segment(settings.whisper_single_segment);
    // Suppress Whisper's non-speech hallucinations. On trailing silence the
    // model loves to emit subtitle-style filler it learned from video training
    // data — most visibly "Vielen Dank", "Untertitel von …", "Thank you for
    // watching". suppress_nst drops those non-speech tokens; suppress_blank
    // avoids spurious leading blanks.
    params.set_suppress_nst(true);
    params.set_suppress_blank(true);
    params
}

/// Joins the decoded segments of a finished pass into one transcript string.
pub(crate) fn collect_transcript(state: &WhisperState) -> String {
    state
        .as_iter()
        .map(|segment| segment.to_string().trim().to_owned())
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The final dictation pass decodes speech only up to the last voiced sample
/// plus this padding; the trailing silence beyond it is dropped. Decoding into
/// silence is exactly where Whisper hallucinates subtitle-style filler. A touch
/// more generous than the live preview's tail padding (streaming
/// `SPEECH_TAIL_PADDING_MS`, 500 ms) because the final pass has no second
/// chance — a trailing word must never be clipped.
const FINAL_PASS_TAIL_PADDING_MS: u64 = 800;

/// Returns `samples` truncated to the last speech (+ [`FINAL_PASS_TAIL_PADDING_MS`])
/// so the final pass never decodes trailing silence. Returns the input
/// unchanged when no speech was tracked (`last_voiced_end == 0`): dropping to a
/// few hundred ms of pure padding could throw away a quiet real take, so the
/// full buffer is kept in that case.
pub(crate) fn trim_trailing_silence(
    samples: &[f32],
    sample_rate: u32,
    last_voiced_end: usize,
) -> &[f32] {
    if last_voiced_end == 0 || sample_rate == 0 {
        return samples;
    }
    let padding = (sample_rate as u64 * FINAL_PASS_TAIL_PADDING_MS / 1000) as usize;
    let speech_end = last_voiced_end.saturating_add(padding).min(samples.len());
    &samples[..speech_end]
}

/// Subtitle / broadcast boilerplate Whisper emits from its video-heavy training
/// data when it decodes (near-)silent audio — never something a user dictates.
/// Matched (normalized, case-insensitively) only against the *trailing*
/// sentences of a transcript, so a legitimate mid-text occurrence is untouched.
///
/// Deliberately conservative: ambiguous phrases a user might genuinely dictate
/// at the end of a message — a bare "Vielen Dank", "Mit freundlichen Grüßen",
/// "Tschüss", a standalone "Untertitel" — are NOT listed. Those are handled
/// upstream by [`trim_trailing_silence`]; blocking them here would silently eat
/// real content. Every entry is a phrase specific enough to broadcast outros.
const HALLUCINATION_MARKERS: &[&str] = &[
    "amara.org",
    "untertitel von",
    "untertitel im auftrag",
    "untertitel der",
    "untertitelung",
    "copyright wdr",
    "copyright zdf",
    "thanks for watching",
    "thank you for watching",
    "für's zuschauen",
    "fürs zuschauen",
    "bis zum nächsten mal",
    "abonniert nicht vergessen",
    "abonnieren nicht vergessen",
];

/// Removes trailing hallucinated subtitle/broadcast phrases from a final
/// transcript. Splits into sentence-ish units and drops trailing units that
/// match a [`HALLUCINATION_MARKERS`] entry, stopping at the first real sentence.
/// Only the tail is examined, so genuine mid-text content is never affected.
pub(crate) fn strip_hallucinated_tail(text: &str) -> String {
    let mut sentences = split_sentences(text);
    while sentences.last().is_some_and(|last| is_hallucinated_sentence(last)) {
        sentences.pop();
    }
    sentences.join(" ").trim().to_owned()
}

/// Splits text into sentence-ish units, keeping each terminator attached, so a
/// surviving sentence retains its own punctuation on rejoin. Whitespace-only
/// fragments are dropped.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        current.push(ch);
        // Only a terminator at a word boundary ends a sentence — a '.' inside a
        // token ("amara.org", "2017.") must not fragment it.
        let boundary = matches!(ch, '.' | '!' | '?' | '…')
            && chars.peek().is_none_or(|next| next.is_whitespace());
        if boundary {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                sentences.push(trimmed.to_owned());
            }
            current.clear();
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        sentences.push(trimmed.to_owned());
    }
    sentences
}

/// Whether one sentence is a known hallucination artifact. Both sides run
/// through [`normalize_for_match`] so markers can be written naturally.
fn is_hallucinated_sentence(sentence: &str) -> bool {
    let normalized = normalize_for_match(sentence);
    if normalized.is_empty() {
        return false;
    }
    HALLUCINATION_MARKERS
        .iter()
        .any(|marker| normalized.contains(&normalize_for_match(marker)))
}

/// Lowercases and reduces a string to alphanumeric words (plus `.`, which keeps
/// tokens like "amara.org" intact) separated by single spaces, so punctuation
/// and spacing differences never defeat a marker match.
fn normalize_for_match(text: &str) -> String {
    let lowered = text.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '.' {
                ch
            } else {
                ' '
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn resample_to_16khz(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    if sample_rate == 16_000 {
        return samples.to_vec();
    }

    let ratio = 16_000.0 / sample_rate as f64;
    let target_len = (samples.len() as f64 * ratio).round() as usize;
    let mut output = Vec::with_capacity(target_len);

    for index in 0..target_len {
        let source_position = index as f64 / ratio;
        let source_index = source_position.floor() as usize;
        let frac = (source_position - source_index as f64) as f32;
        let current = *samples.get(source_index).unwrap_or(&0.0);
        let next = *samples.get(source_index + 1).unwrap_or(&current);
        output.push(current + (next - current) * frac);
    }

    output
}

/// Logs whisper.cpp's compiled backend flags exactly once per process so the
/// log file alone proves whether GPU acceleration is available. There is no
/// boolean "is Metal active" API in whisper-rs — the authoritative source is
/// the `print_system_info()` string (contains `METAL = 1` when the Metal
/// backend is compiled in). We derive a plain-language line from it and also
/// dump the raw string plus the bundled whisper.cpp version. The ggml backend
/// then logs its own `ggml_metal_init: …` lines at context-creation time
/// (routed through `whisper_rs::ggml_logging_hook`, admitted by the file
/// logger) which confirm the GPU is actually initialised and used.
fn log_whisper_backend_info_once() {
    static LOGGED: Once = Once::new();
    LOGGED.call_once(|| {
        let system_info = whisper_rs::print_system_info();
        let metal_enabled = whisper_metal_compiled_in(system_info);
        if metal_enabled {
            log::info!(
                target: "dictation",
                "GPU acceleration: Metal ENABLED (whisper.cpp {})",
                whisper_rs::WHISPER_CPP_VERSION
            );
        } else {
            log::warn!(
                target: "dictation",
                "GPU acceleration: Metal DISABLED — running on CPU (whisper.cpp {})",
                whisper_rs::WHISPER_CPP_VERSION
            );
        }
        log::info!(target: "dictation", "whisper system info: {system_info}");
    });
}

/// Whether whisper.cpp was compiled with the Metal backend, inferred from its
/// `print_system_info()` string. The format has changed across versions, so we
/// accept both the current `Metal : …` section header (whisper.cpp 1.8.x) and
/// the older `METAL = 1` flag. ggml only prints a backend's section when that
/// backend is compiled in, so the presence of the `Metal :` header is the
/// signal (the GPU is then actually used unless no device is available, which
/// the `ggml_metal_*` init lines in the log confirm).
pub(crate) fn whisper_metal_compiled_in(system_info: &str) -> bool {
    system_info.contains("Metal :") || system_info.contains("METAL = 1")
}

/// Resolves the whisper inference thread count from settings. `0` means "auto":
/// use the available parallelism, capped at 6 so a burst of inference threads
/// never starves the UI / audio callbacks. A non-zero setting is an expert
/// override, clamped to a sane 1..=16 range.
fn thread_count() -> i32 {
    resolve_thread_count(0)
}

/// Process-wide lock ensuring only one Whisper inference runs at a time
/// (#43, streaming prep). Returns the guard; poisoning is recovered from since
/// the lock protects no data, only serializes access.
pub(crate) fn whisper_inference_lock() -> std::sync::MutexGuard<'static, ()> {
    WHISPER_INFERENCE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Non-blocking variant for the streaming worker: a preview pass must never
/// queue behind a long-running final pass (e.g. of a just-cancelled previous
/// dictation) — it would publish a stale snapshot much later. Skipping keeps
/// the worker ticking with fresh audio instead (#41).
pub(crate) fn try_whisper_inference_lock() -> Option<std::sync::MutexGuard<'static, ()>> {
    match WHISPER_INFERENCE_LOCK.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

static WHISPER_INFERENCE_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn resolve_thread_count(configured: u32) -> i32 {
    if configured == 0 {
        std::thread::available_parallelism()
            .map(|value| value.get().min(6) as i32)
            .unwrap_or(4)
    } else {
        configured.clamp(1, 16) as i32
    }
}

/// The recording bubble is always shown; the live preview runs whenever the
/// user picked the live-text display mode (`show_recording_indicator` is a
/// legacy field — the bubble can no longer be disabled).
fn live_transcription_active(settings: &AppSettings) -> bool {
    settings.live_transcription_enabled
}

/// Runs one short silent inference right after a model is (pre)loaded. ggml's
/// Metal backend compiles its pipeline cache lazily on the first inference of
/// the process — a multi-second, one-time cliff on large models that would
/// otherwise hit the first live-preview pass or the first final pass (#41).
/// Best-effort: failures only cost the warm start, never a dictation.
fn warm_inference_graph(context: &WhisperContext, settings: &AppSettings) {
    let started = Instant::now();
    let mut state = match context.create_state() {
        Ok(state) => state,
        Err(err) => {
            log::debug!(target: "dictation", "graph warmup skipped: {err}");
            return;
        }
    };
    let language = normalized_language(&settings.transcription_language);
    let params = base_full_params(
        settings,
        language.as_deref(),
        resolve_thread_count(settings.whisper_thread_count),
    );
    let silence = vec![0.0_f32; 16_000];
    let _inference_guard = whisper_inference_lock();
    match state.full(params, &silence) {
        Ok(()) => log::info!(
            target: "dictation",
            "inference graph warmed in {:.1}s",
            started.elapsed().as_secs_f32()
        ),
        Err(err) => log::debug!(target: "dictation", "graph warmup inference failed: {err}"),
    }
}

fn normalized_language(language: &str) -> Option<String> {
    let trimmed = language.trim().to_lowercase();
    if trimmed.is_empty() || trimmed == "auto" {
        None
    } else {
        Some(trimmed)
    }
}

fn discover_input_devices() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let mut devices = host
        .input_devices()
        .map_err(|err| err.to_string())?
        .filter_map(|device| {
            device
                .description()
                .ok()
                .map(|description| description.name().to_owned())
        })
        .collect::<Vec<_>>();

    devices.sort();
    devices.dedup();

    if let Some(default_name) = default_input_device_name() {
        devices.retain(|name| name != &default_name);
        devices.insert(0, default_name);
    }

    Ok(devices)
}

fn select_input_device_for_recording(resolved_name: &str) -> Result<(Device, String), String> {
    let host = cpal::default_host();

    if resolved_name == system_default_label() || resolved_name.is_empty() {
        let device = host
            .default_input_device()
            .ok_or_else(|| "No default input device available.".to_owned())?;
        let name = device
            .description()
            .ok()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|| system_default_label().to_owned());
        return Ok((device, name));
    }

    if let Some(default_device) = host.default_input_device()
        && default_device
            .description()
            .map(|description| description.name() == resolved_name)
            .unwrap_or(false)
    {
        return Ok((default_device, resolved_name.to_owned()));
    }

    let matching = host
        .input_devices()
        .map_err(|err| err.to_string())?
        .find(|device| {
            device
                .description()
                .map(|description| description.name() == resolved_name)
                .unwrap_or(false)
        });

    matching
        .map(|device| (device, resolved_name.to_owned()))
        .ok_or_else(|| format!("Input device '{}' was not found.", resolved_name))
}

fn resolve_input_device_name(settings: &AppSettings, available: &[String]) -> String {
    for preferred in settings.preferred_input_devices_sorted() {
        if preferred.name == system_default_label() {
            return system_default_label().to_owned();
        }
        if available.iter().any(|name| name == &preferred.name) {
            return preferred.name.clone();
        }
    }

    let primary = settings.input_device_name.trim();
    if !primary.is_empty()
        && (primary == system_default_label() || available.iter().any(|name| name == primary))
    {
        return primary.to_owned();
    }

    system_default_label().to_owned()
}

fn default_input_device_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|device| {
            device
                .description()
                .ok()
                .map(|description| description.name().to_owned())
        })
}

fn system_default_label() -> &'static str {
    SYSTEM_DEFAULT_DEVICE_LABEL
}

#[derive(Clone, Copy)]
pub enum RecordingCue {
    Start,
    Stop,
    Cancel,
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn play_recording_cue(cue: RecordingCue) {
    thread::spawn(move || {
        let _ = play_recording_cue_blocking(cue);
    });
}

pub fn play_cancel_cue() {
    play_recording_cue(RecordingCue::Cancel);
}

fn play_recording_cue_blocking(cue: RecordingCue) -> Result<(), String> {
    let Some(device) = cpal::default_host().default_output_device() else {
        return Ok(());
    };

    let config = device
        .default_output_config()
        .map_err(|err| format!("Output configuration could not be loaded: {err}"))?;
    let stream_config = config.config();
    let sample_rate = stream_config.sample_rate;
    let channels = stream_config.channels as usize;
    let samples = render_recording_cue(cue, sample_rate);
    if samples.is_empty() {
        return Ok(());
    }

    let playback_duration = cue_playback_duration(cue);
    let stream = match config.sample_format() {
        SampleFormat::I8 => {
            build_cue_output_stream::<i8>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::I16 => {
            build_cue_output_stream::<i16>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::I24 => {
            build_cue_output_stream::<I24>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::I32 => {
            build_cue_output_stream::<i32>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::I64 => {
            build_cue_output_stream::<i64>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::U8 => {
            build_cue_output_stream::<u8>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::U16 => {
            build_cue_output_stream::<u16>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::U24 => {
            build_cue_output_stream::<U24>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::U32 => {
            build_cue_output_stream::<u32>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::U64 => {
            build_cue_output_stream::<u64>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::F32 => {
            build_cue_output_stream::<f32>(&device, &stream_config, channels, samples)?
        }
        SampleFormat::F64 => {
            build_cue_output_stream::<f64>(&device, &stream_config, channels, samples)?
        }
        other => {
            return Err(format!(
                "Sample format '{other}' for output signal is not currently supported."
            ));
        }
    };

    stream
        .play()
        .map_err(|err| format!("Output signal could not be started: {err}"))?;
    thread::sleep(playback_duration);
    Ok(())
}

fn build_cue_output_stream<T>(
    device: &Device,
    config: &cpal::StreamConfig,
    channels: usize,
    samples: Vec<f32>,
) -> Result<Stream, String>
where
    T: Sample + SizedSample + FromSample<f32>,
{
    let mut cursor = 0usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| write_cue_output_data(data, channels, &samples, &mut cursor),
            |_err| {},
            None,
        )
        .map_err(|err| err.to_string())
}

fn write_cue_output_data<T>(output: &mut [T], channels: usize, samples: &[f32], cursor: &mut usize)
where
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let value = if *cursor < samples.len() {
            let sample = samples[*cursor];
            *cursor += 1;
            sample
        } else {
            0.0
        };
        let output_sample = T::from_sample(value);
        for channel in frame {
            *channel = output_sample;
        }
    }
}

fn render_recording_cue(cue: RecordingCue, sample_rate: u32) -> Vec<f32> {
    let notes = cue_notes(cue);
    let gap_samples = ms_to_output_samples(CUE_NOTE_GAP_MS, sample_rate);
    let total_samples = notes
        .iter()
        .map(|(_, duration_ms)| ms_to_output_samples(*duration_ms, sample_rate))
        .sum::<usize>()
        + gap_samples * notes.len().saturating_sub(1);
    let mut rendered = Vec::with_capacity(total_samples);

    for (index, (frequency_hz, duration_ms)) in notes.iter().copied().enumerate() {
        append_cue_note(&mut rendered, sample_rate, frequency_hz, duration_ms);
        if index + 1 < notes.len() {
            rendered.extend(std::iter::repeat_n(0.0, gap_samples));
        }
    }

    rendered
}

fn cue_notes(cue: RecordingCue) -> &'static [(f32, u32)] {
    match cue {
        RecordingCue::Start => &RECORDING_START_NOTES,
        RecordingCue::Stop => &RECORDING_STOP_NOTES,
        RecordingCue::Cancel => &RECORDING_CANCEL_NOTES,
    }
}

fn append_cue_note(output: &mut Vec<f32>, sample_rate: u32, frequency_hz: f32, duration_ms: u32) {
    let sample_count = ms_to_output_samples(duration_ms, sample_rate);
    let attack_samples = ms_to_output_samples(5, sample_rate)
        .min(sample_count)
        .max(1);
    let release_samples = ms_to_output_samples(28, sample_rate)
        .min(sample_count)
        .max(1);

    for sample_index in 0..sample_count {
        let seconds = sample_index as f32 / sample_rate as f32;
        let phase = 2.0 * PI * frequency_hz * seconds;
        let tone = phase.sin() * 0.94 + (phase * 2.0).sin() * 0.06;
        let envelope = cue_envelope(sample_index, sample_count, attack_samples, release_samples);
        output.push(tone * envelope * CUE_VOLUME);
    }
}

fn cue_envelope(
    sample_index: usize,
    sample_count: usize,
    attack_samples: usize,
    release_samples: usize,
) -> f32 {
    let attack = if sample_index >= attack_samples {
        1.0
    } else {
        let progress = sample_index as f32 / attack_samples as f32;
        (progress * PI * 0.5).sin()
    };

    let remaining_samples = sample_count.saturating_sub(sample_index + 1);
    let release = if remaining_samples >= release_samples {
        1.0
    } else {
        let progress = remaining_samples as f32 / release_samples as f32;
        (progress * PI * 0.5).sin()
    };

    attack.min(release)
}

fn ms_to_output_samples(duration_ms: u32, sample_rate: u32) -> usize {
    ((sample_rate as u64 * duration_ms as u64) / 1_000).max(1) as usize
}

fn cue_playback_duration(cue: RecordingCue) -> Duration {
    let total_ms = cue_notes(cue)
        .iter()
        .map(|(_, duration_ms)| *duration_ms)
        .sum::<u32>()
        + CUE_NOTE_GAP_MS * cue_notes(cue).len().saturating_sub(1) as u32
        + 80;
    Duration::from_millis(total_ms as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_conversion_averages_channels() {
        let stereo = [1.0, -1.0, 0.5, 0.5];
        let mono = interleaved_to_mono(&stereo, 2);
        assert_eq!(mono, vec![0.0, 0.5]);
    }

    #[test]
    fn resample_identity_keeps_length() {
        let audio = vec![0.0, 0.5, -0.5, 1.0];
        let resampled = resample_to_16khz(&audio, 16_000);
        assert_eq!(audio, resampled);
    }

    #[test]
    fn auto_language_maps_to_none() {
        assert_eq!(normalized_language("auto"), None);
        assert_eq!(normalized_language("de"), Some("de".to_owned()));
    }

    #[test]
    fn push_chunk_counts_only_voiced_samples() {
        let (event_tx, _event_rx) = mpsc::channel();
        let mut buffer = RecordingBuffer::new(16_000, false, 0.014, 900);

        let loud = vec![0.2_f32; 800];
        let quiet = vec![0.001_f32; 800];
        buffer.push_chunk(&loud, &event_tx);
        buffer.push_chunk(&quiet, &event_tx);
        buffer.push_chunk(&loud, &event_tx);

        // Only the two loud chunks count as voiced; the buffer holds all three.
        assert_eq!(buffer.voiced_samples(), 1_600);
        assert_eq!(buffer.samples.len(), 2_400);
    }

    #[test]
    fn quiet_speech_below_vad_threshold_still_counts_as_voiced() {
        let (event_tx, _event_rx) = mpsc::channel();
        // Default VAD threshold 0.014 — the auto-stop keeps using it, but the
        // streaming gate must accept quiet-but-clear speech on low-gain mics.
        let mut buffer = RecordingBuffer::new(16_000, false, 0.014, 900);

        let silence = vec![0.0005_f32; 800];
        let quiet_speech = vec![0.006_f32; 800];
        buffer.push_chunk(&silence, &event_tx);
        buffer.push_chunk(&quiet_speech, &event_tx);
        buffer.push_chunk(&silence, &event_tx);
        buffer.push_chunk(&quiet_speech, &event_tx);

        assert_eq!(buffer.voiced_samples(), 1_600);
        // The strict VAD latch must NOT have fired for any of these chunks.
        assert!(!buffer.voice_detected);
    }

    #[test]
    fn copy_new_samples_appends_only_the_tail() {
        let (event_tx, _event_rx) = mpsc::channel();
        let mut buffer = RecordingBuffer::new(16_000, false, 0.014, 900);
        buffer.push_chunk(&[0.1, 0.2, 0.3], &event_tx);

        let mut local = Vec::new();
        buffer.copy_new_samples(0, &mut local);
        assert_eq!(local, vec![0.1, 0.2, 0.3]);

        buffer.push_chunk(&[0.4, 0.5], &event_tx);
        buffer.copy_new_samples(local.len(), &mut local);
        assert_eq!(local, vec![0.1, 0.2, 0.3, 0.4, 0.5]);

        // Beyond-length reads (buffer emptied by finish()) are a no-op.
        buffer.finish(Duration::from_secs(1));
        buffer.copy_new_samples(local.len(), &mut local);
        assert_eq!(local.len(), 5);
    }

    #[test]
    fn recording_cues_are_short_and_distinct() {
        let sample_rate = 48_000;
        let start = render_recording_cue(RecordingCue::Start, sample_rate);
        let stop = render_recording_cue(RecordingCue::Stop, sample_rate);
        let cancel = render_recording_cue(RecordingCue::Cancel, sample_rate);
        let max_samples = ms_to_output_samples(320, sample_rate);

        assert!(!start.is_empty());
        assert!(!stop.is_empty());
        assert!(!cancel.is_empty());
        assert!(start.len() <= max_samples);
        assert!(stop.len() <= max_samples);
        assert!(cancel.len() <= max_samples);
        assert!(start.iter().any(|sample| sample.abs() > 0.001));
        assert!(stop.iter().any(|sample| sample.abs() > 0.001));
        assert!(cancel.iter().any(|sample| sample.abs() > 0.001));
        assert_ne!(start, stop);
        assert_ne!(start, cancel);
        assert_ne!(stop, cancel);
    }

    #[test]
    fn trim_trailing_silence_cuts_to_last_speech_plus_padding() {
        // 16 kHz, speech ends at sample 1_000, then a long silent tail.
        let samples = vec![0.0_f32; 40_000];
        let trimmed = trim_trailing_silence(&samples, 16_000, 1_000);
        // 800 ms padding at 16 kHz = 12_800 samples.
        assert_eq!(trimmed.len(), 1_000 + 12_800);
    }

    #[test]
    fn trim_trailing_silence_keeps_full_buffer_without_speech() {
        // last_voiced_end == 0 means no speech was ever tracked: never truncate,
        // or a quiet real take would be reduced to pure padding.
        let samples = vec![0.1_f32; 5_000];
        assert_eq!(trim_trailing_silence(&samples, 16_000, 0).len(), 5_000);
    }

    #[test]
    fn trim_trailing_silence_never_extends_past_the_buffer() {
        let samples = vec![0.0_f32; 2_000];
        // Padding would reach past the end; result is clamped to the buffer.
        assert_eq!(trim_trailing_silence(&samples, 16_000, 1_900).len(), 2_000);
    }

    #[test]
    fn strip_removes_trailing_broadcast_outros() {
        assert_eq!(
            strip_hallucinated_tail("Das ist mein Text. Bis zum nächsten Mal im ZDF."),
            "Das ist mein Text."
        );
        assert_eq!(
            strip_hallucinated_tail("Bitte um Rückmeldung. Vielen Dank fürs Zuschauen."),
            "Bitte um Rückmeldung."
        );
        assert_eq!(
            strip_hallucinated_tail("Ende. Untertitel von der Amara.org-Community"),
            "Ende."
        );
        assert_eq!(
            strip_hallucinated_tail("Hello there. Thank you for watching!"),
            "Hello there."
        );
    }

    #[test]
    fn strip_removes_multiple_stacked_outros() {
        // Two genuine artifacts stacked at the tail are both removed, stopping
        // at the first real sentence.
        assert_eq!(
            strip_hallucinated_tail(
                "Der eigentliche Inhalt. Untertitelung des ZDF. Bis zum nächsten Mal."
            ),
            "Der eigentliche Inhalt."
        );
        assert_eq!(
            strip_hallucinated_tail("Inhalt. Vielen Dank fürs Zuschauen. Bis zum nächsten Mal."),
            "Inhalt."
        );
    }

    #[test]
    fn strip_keeps_legitimate_farewells() {
        // A bare farewell is real dictation (e.g. an email) — must be preserved.
        // Trailing-silence trimming, not the blocklist, prevents the hallucinated
        // variant, so these ambiguous phrases stay untouched here.
        for text in [
            "Ich melde mich bald. Vielen Dank.",
            "Anbei die Unterlagen. Mit freundlichen Grüßen.",
            "Wir sehen uns morgen. Tschüss.",
            "Bitte füge die Untertitel zum Video hinzu.",
            "Ich habe den Newsletter abonniert.",
        ] {
            assert_eq!(strip_hallucinated_tail(text), text);
        }
    }

    #[test]
    fn strip_leaves_clean_text_unchanged() {
        let text = "Ein ganz normaler Satz ohne Floskeln.";
        assert_eq!(strip_hallucinated_tail(text), text);
    }
}
