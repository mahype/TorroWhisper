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
use cpal::{
    Device, FromSample, I24, Sample, SampleFormat, SizedSample, Stream, SupportedStreamConfig, U24,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use torrowhisper_core::{AppSettings, SYSTEM_DEFAULT_DEVICE_LABEL, TriggerMode};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

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
                (path_for_thread, Arc::new(context))
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
    fn drain_warmup(&mut self) {
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
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                self.warmup_rx = None;
            }
            Err(TryRecvError::Empty) => {}
        }
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
        if let Err(err) = validated_model_path(settings) {
            self.mark_blocked_now();
            return Err(format!("Recording blocked: {err}"));
        }

        let resolved_name = resolve_input_device_name(settings, &self.available_input_devices);
        let (recording, used_name) = ActiveRecording::start(settings, &resolved_name)?;
        self.recording = Some(recording);
        self.active_input_device_name = used_name.clone();
        self.clear_blocked();
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

        thread::spawn(move || {
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

        self.drain_warmup();

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
                    outcomes.push(DictationOutcome::Error(err));
                }
                Err(TryRecvError::Disconnected) => {
                    // The worker thread died without sending a result — most
                    // likely a panic inside whisper. The panic hook has the
                    // details in the log.
                    self.transcription_rx = None;
                    self.pending_timing_seed = None;
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

enum RecordingEvent {
    SilenceDetected,
    StreamError(String),
}

const LEVEL_HISTORY_CAPACITY: usize = 120;

struct RecordingBuffer {
    samples: Vec<f32>,
    sample_rate: u32,
    vad_enabled: bool,
    vad_threshold: f32,
    silence_limit_samples: usize,
    silence_run_samples: usize,
    voice_detected: bool,
    silence_notification_sent: bool,
    level_history: VecDeque<f32>,
}

impl RecordingBuffer {
    fn new(sample_rate: u32, vad_enabled: bool, vad_threshold: f32, vad_silence_ms: u32) -> Self {
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
            level_history: VecDeque::with_capacity(LEVEL_HISTORY_CAPACITY),
        }
    }

    fn push_chunk(&mut self, chunk: &[f32], event_tx: &Sender<RecordingEvent>) {
        if chunk.is_empty() {
            return;
        }

        self.samples.extend_from_slice(chunk);

        let rms = root_mean_square(chunk);
        if self.level_history.len() == LEVEL_HISTORY_CAPACITY {
            self.level_history.pop_front();
        }
        self.level_history.push_back(rms);

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

    fn finish(&mut self, duration: Duration) -> RecordedAudio {
        RecordedAudio {
            samples: std::mem::take(&mut self.samples),
            sample_rate: self.sample_rate,
            duration,
        }
    }
}

struct RecordedAudio {
    samples: Vec<f32>,
    sample_rate: u32,
    duration: Duration,
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
    let resample_started = Instant::now();
    let mono_16khz = resample_to_16khz(&audio.samples, audio.sample_rate);
    let resample_secs = resample_started.elapsed().as_secs_f32();
    if mono_16khz.is_empty() {
        return Err("No audio data available for Whisper.".to_owned());
    }

    let n_threads = resolve_thread_count(settings.whisper_thread_count);
    let inference = run_whisper_inference(&context, &mono_16khz, settings, language, n_threads)?;

    if inference.text.is_empty() {
        return Err(format!(
            "Whisper recognized no text. Model: {}, language: {}.",
            settings.local_model.default_filename(),
            language.unwrap_or("auto")
        ));
    }

    Ok(TranscriptionOutput {
        text: inference.text,
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

    let transcript = state
        .as_iter()
        .map(|segment| segment.to_string().trim().to_owned())
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    Ok(WhisperInference {
        text: transcript,
        state_secs,
        inference_secs,
    })
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
fn whisper_inference_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn resolve_thread_count(configured: u32) -> i32 {
    if configured == 0 {
        std::thread::available_parallelism()
            .map(|value| value.get().min(6) as i32)
            .unwrap_or(4)
    } else {
        configured.clamp(1, 16) as i32
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
}
