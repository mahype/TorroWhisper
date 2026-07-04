//! Chat plugin engine (#17).
//!
//! The chat reuses the main [`crate::dictation::DictationController`] for audio
//! capture + Whisper (already mic/device-initialized) — when a chat turn is
//! listening, the bridge routes the finished transcript here instead of
//! inserting it. This controller owns the conversation(s) + LLM generation.
//!
//! Conversations are persisted as multiple [`ChatSession`]s (the sidebar
//! history); one is active at a time. v1 generation is blocking (the whole
//! answer is produced, then spoken). Multi-turn context is flattened into the
//! system prompt for now; true per-provider message arrays come later.

use std::{
    cmp::Reverse,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

/// Monotonic suffix so two sessions created in the same nanosecond never share
/// an id.
static SESSION_SEQ: AtomicU64 = AtomicU64::new(0);

use donnywhisper_core::{
    AppSettings, ChatMessageDto, ChatPhase, ChatRole, ChatSession, ChatSessionDto, ChatStateDto,
    LlmModelRef, LlmPreset,
};

use crate::plugin_api::{BridgeHost, LogLevel, PluginHost};
use crate::{plugin_log, sessions_store};

const DEFAULT_SYSTEM_PROMPT: &str = "You are a friendly voice assistant. Answer briefly and conversationally, as if speaking aloud.";

/// Always appended to the (default or user-configured) system prompt: the answer
/// is read aloud by TTS, so it must be natural spoken prose — not bullet points
/// or abbreviations, which are hard to listen to. A best-effort instruction;
/// `speech_text::normalize_for_speech` is the model-independent safety net.
const SPEAK_STYLE: &str = "Your reply is read aloud by text-to-speech, so write it the way a person actually speaks: natural, flowing full sentences. Do not use bullet points, numbered lists, headings, markdown, tables, code blocks, emoji or symbols. Write abbreviations out in full (for example say \"zum Beispiel\" instead of \"z. B.\", \"und so weiter\" instead of \"usw.\"). Do not read out URLs or code. Keep sentences short and easy to follow by ear. Reply in the user's language.";

/// Max characters of the first user message kept as a session title.
const TITLE_MAX_CHARS: usize = 40;

/// Events streamed from the generation worker thread to [`ChatController::poll`].
enum GenEvent {
    /// An incremental text delta of the answer.
    Chunk(String),
    /// Generation finished: the full answer, or an error.
    Done(Result<String, String>),
}

pub struct ChatController {
    sessions: Vec<ChatSession>,
    active_id: String,
    generation_rx: Option<Receiver<GenEvent>>,
    generating: bool,
    /// True once the in-progress assistant message for the current generation has
    /// been appended (on the first streamed chunk).
    generating_msg_open: bool,
    /// Session the in-flight answer belongs to, so it lands in the right place
    /// even if the user switches sessions while it generates.
    generating_session_id: String,
    cancelled: Arc<AtomicBool>,
    revision: u64,
    error: Option<String>,
    /// Disabled in tests so unit tests never touch the on-disk sessions file.
    persist_enabled: bool,
}

impl ChatController {
    pub fn new() -> Self {
        let mut sessions = sessions_store::load().unwrap_or_default();
        sessions.sort_by_key(|s| Reverse(s.updated_at));
        Self::with_sessions(sessions, true)
    }

    fn with_sessions(mut sessions: Vec<ChatSession>, persist_enabled: bool) -> Self {
        let active_id = match sessions.first() {
            Some(first) => first.id.clone(),
            None => {
                let session = new_empty_session();
                let id = session.id.clone();
                sessions.push(session);
                id
            }
        };
        Self {
            sessions,
            active_id,
            generation_rx: None,
            generating: false,
            generating_msg_open: false,
            generating_session_id: String::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
            revision: 0,
            error: None,
            persist_enabled,
        }
    }

    /// Sets the active conversation's model/agent and persists it, so each
    /// conversation remembers its own pick across window reopens and restarts
    /// (#agent). `updated_at` is intentionally left untouched — picking a model
    /// must not reshuffle the sidebar's newest-first order.
    pub fn set_model(&mut self, model_ref: Option<LlmModelRef>) {
        if let Some(index) = self.active_index() {
            self.sessions[index].model_ref = model_ref;
            self.persist();
        }
    }

    /// True while an answer is being generated. The shortcut uses this to avoid
    /// starting a new recording (which would drop the in-flight answer).
    pub fn is_generating(&self) -> bool {
        self.generating
    }

    fn active_index(&self) -> Option<usize> {
        self.sessions.iter().position(|s| s.id == self.active_id)
    }

    fn persist(&self) {
        if !self.persist_enabled {
            return;
        }
        if let Err(err) = sessions_store::save(&self.sessions) {
            plugin_log::warn("chat", &format!("could not save sessions: {err}"));
        }
    }

    fn cancel_generation(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.generation_rx = None;
        self.generating = false;
        self.generating_msg_open = false;
        self.cancelled = Arc::new(AtomicBool::new(false));
    }

    /// Clears the active conversation's messages (keeps the session entry).
    pub fn reset(&mut self) {
        self.cancel_generation();
        self.error = None;
        if let Some(index) = self.active_index() {
            self.sessions[index].messages.clear();
            self.sessions[index].title.clear();
            self.sessions[index].updated_at = now_unix();
        }
        self.persist();
        self.revision += 1;
    }

    /// Starts a fresh conversation. If the active one is already empty, stays on
    /// it; otherwise the current session is kept in the list and a new empty one
    /// becomes active.
    pub fn new_session(&mut self) {
        let active_empty = self
            .active_index()
            .map(|index| self.sessions[index].messages.is_empty())
            .unwrap_or(true);
        if !active_empty {
            self.cancel_generation();
            // Carry the current pick into the new conversation so the user does
            // not have to re-select their model/agent every time.
            let inherited = self
                .active_index()
                .and_then(|index| self.sessions[index].model_ref.clone());
            let mut session = new_empty_session();
            session.model_ref = inherited;
            self.active_id = session.id.clone();
            self.sessions.insert(0, session);
            self.persist();
        }
        self.error = None;
        self.revision += 1;
    }

    /// Switches the active session to an existing one.
    pub fn switch_session(&mut self, id: &str) {
        if self.sessions.iter().any(|s| s.id == id) {
            self.active_id = id.to_owned();
            self.error = None;
            self.revision += 1;
        }
    }

    /// Deletes a session. If it was active, falls back to the newest remaining
    /// session (or a fresh empty one).
    pub fn delete_session(&mut self, id: &str) {
        // If the deleted session is the one currently generating, cancel it —
        // its answer can no longer land anywhere, and a stuck `generating` flag
        // would lock out the chat shortcut.
        if self.generating && self.generating_session_id == id {
            self.cancel_generation();
        }
        self.sessions.retain(|s| s.id != id);
        if self.active_id == id {
            self.cancel_generation();
            self.sessions.sort_by_key(|s| Reverse(s.updated_at));
            match self.sessions.first() {
                Some(first) => self.active_id = first.id.clone(),
                None => {
                    let session = new_empty_session();
                    self.active_id = session.id.clone();
                    self.sessions.push(session);
                }
            }
            self.error = None;
        }
        self.persist();
        self.revision += 1;
    }

    /// Called by the bridge when a chat-turn transcript is ready. Appends the
    /// user message to the active session and kicks off generation.
    pub fn on_transcript(&mut self, transcript: String, settings: &AppSettings) {
        let text = transcript.trim().to_owned();
        if text.is_empty() {
            // Nothing said — just re-arm without a turn.
            return;
        }
        plugin_log::info(
            "chat",
            &format!("user turn received ({} chars)", text.chars().count()),
        );
        let Some(index) = self.active_index() else {
            plugin_log::warn(
                "chat",
                "transcript arrived with no active session — dropped",
            );
            return;
        };
        self.error = None;
        if self.sessions[index].title.is_empty() {
            self.sessions[index].title = derive_title(&text);
        }
        self.sessions[index].messages.push(ChatMessageDto {
            role: ChatRole::User,
            content: text,
        });
        self.sessions[index].updated_at = now_unix();
        self.persist();
        self.revision += 1;
        self.start_generation(settings);
    }

    /// Drained from `BridgeRuntime::poll()`. Consumes streamed chunks (growing
    /// the in-progress answer) and the final completion. Drains all queued events
    /// per call so the UI sees the latest text promptly.
    pub fn poll(&mut self) {
        loop {
            let Some(rx) = &self.generation_rx else {
                return;
            };
            match rx.try_recv() {
                Ok(GenEvent::Chunk(delta)) => {
                    self.append_chunk(&delta);
                    self.revision += 1;
                }
                Ok(GenEvent::Done(Ok(answer))) => {
                    self.complete_generation(answer);
                }
                Ok(GenEvent::Done(Err(err))) => {
                    self.fail_generation(err);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.generation_rx = None;
                    self.generating = false;
                    self.generating_msg_open = false;
                    self.error = Some("Chat generation stopped unexpectedly.".to_owned());
                    self.revision += 1;
                    return;
                }
            }
        }
    }

    /// Index of the session the in-flight generation belongs to.
    fn generating_index(&self) -> Option<usize> {
        self.sessions
            .iter()
            .position(|s| s.id == self.generating_session_id)
    }

    /// Appends a streamed delta, creating the assistant message on the first one.
    fn append_chunk(&mut self, delta: &str) {
        let Some(index) = self.generating_index() else {
            return;
        };
        if !self.generating_msg_open {
            self.sessions[index].messages.push(ChatMessageDto {
                role: ChatRole::Assistant,
                content: String::new(),
            });
            self.generating_msg_open = true;
        }
        if let Some(last) = self.sessions[index].messages.last_mut() {
            last.content.push_str(delta);
        }
        self.sessions[index].updated_at = now_unix();
        // Persist once on completion, not per chunk.
    }

    fn complete_generation(&mut self, answer: String) {
        let trimmed = answer.trim().to_owned();
        self.generation_rx = None;
        self.generating = false;
        if let Some(index) = self.generating_index() {
            if self.generating_msg_open {
                if trimmed.is_empty() {
                    // Drop the empty streamed bubble.
                    if matches!(self.sessions[index].messages.last(), Some(m) if m.role == ChatRole::Assistant)
                    {
                        self.sessions[index].messages.pop();
                    }
                } else if let Some(last) = self.sessions[index].messages.last_mut() {
                    last.content = trimmed.clone();
                }
            } else if !trimmed.is_empty() {
                // No chunks arrived (defensive) — append the whole answer.
                self.sessions[index].messages.push(ChatMessageDto {
                    role: ChatRole::Assistant,
                    content: trimmed.clone(),
                });
            }
            self.sessions[index].updated_at = now_unix();
            self.persist();
        }
        if trimmed.is_empty() {
            plugin_log::warn("chat", "model returned an empty answer");
            self.error = Some("The model returned an empty answer.".to_owned());
        } else {
            plugin_log::info(
                "chat",
                &format!("answer received ({} chars)", trimmed.chars().count()),
            );
        }
        self.generating_msg_open = false;
        self.revision += 1;
    }

    fn fail_generation(&mut self, err: String) {
        self.generation_rx = None;
        self.generating = false;
        // Drop an empty in-progress bubble; keep any partial text already streamed.
        if self.generating_msg_open {
            if let Some(index) = self.generating_index() {
                if matches!(self.sessions[index].messages.last(), Some(m) if m.role == ChatRole::Assistant && m.content.trim().is_empty())
                {
                    self.sessions[index].messages.pop();
                }
            }
        }
        self.generating_msg_open = false;
        self.error = Some(err);
        self.revision += 1;
    }

    /// Builds the snapshot for the UI. `listening`/`transcribing` come from the
    /// shared dictation controller (chat reuses it).
    pub fn state(&self, listening: bool, transcribing: bool) -> ChatStateDto {
        // Only show "generating" when the *active* session is the one being
        // generated — switching to another session must not look busy.
        let phase = if listening {
            ChatPhase::Listening
        } else if transcribing {
            ChatPhase::Transcribing
        } else if self.generating && self.generating_session_id == self.active_id {
            ChatPhase::Generating
        } else {
            ChatPhase::Idle
        };
        let messages = self
            .active_index()
            .map(|index| self.sessions[index].messages.clone())
            .unwrap_or_default();
        let mut sessions: Vec<ChatSessionDto> = self
            .sessions
            .iter()
            .map(|s| ChatSessionDto {
                id: s.id.clone(),
                title: s.title.clone(),
                updated_at: s.updated_at,
                message_count: s.messages.len(),
            })
            .collect();
        sessions.sort_by_key(|s| Reverse(s.updated_at));
        let active_model_ref = self
            .active_index()
            .and_then(|index| self.sessions[index].model_ref.clone());
        ChatStateDto {
            phase,
            messages,
            revision: self.revision,
            error: self.error.clone(),
            sessions,
            active_session_id: self.active_id.clone(),
            active_model_ref,
        }
    }

    fn start_generation(&mut self, settings: &AppSettings) {
        let messages = self
            .active_index()
            .map(|index| self.sessions[index].messages.clone())
            .unwrap_or_default();

        // Per-conversation pick → persisted default → local preset.
        let model_ref = self
            .active_index()
            .and_then(|index| self.sessions[index].model_ref.clone())
            .or_else(|| settings.chat.default_model_ref.clone())
            .unwrap_or(LlmModelRef::LocalPreset {
                preset: LlmPreset::default(),
            });
        let configured_prompt = settings.chat.system_prompt.trim();
        let base_prompt = if configured_prompt.is_empty() {
            DEFAULT_SYSTEM_PROMPT
        } else {
            configured_prompt
        };
        // Always enforce speakable prose, even with a custom prompt.
        let base_prompt = format!("{base_prompt}\n\n{SPEAK_STYLE}");
        let system = build_system_with_history(&base_prompt, &messages);
        let latest_user = messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();

        // The chat plugin reaches LLMs only through the shared plugin host —
        // the same versioned surface third-party plugins will use.
        let host = BridgeHost::new("chat", settings.clone());
        host.log(
            LogLevel::Info,
            &format!("generating with {}", describe_model(&model_ref)),
        );

        let cancelled = self.cancelled.clone();
        // Stable per-conversation scope so memory-capable agents (Hermes) keep
        // context across turns and sessions.
        let session_key = self.active_id.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let chunk_tx = tx.clone();
            let mut on_chunk = move |delta: &str| {
                let _ = chunk_tx.send(GenEvent::Chunk(delta.to_owned()));
            };
            let result = host.chat_stream(
                &model_ref,
                &system,
                &latest_user,
                Some(session_key.as_str()),
                &cancelled,
                &mut on_chunk,
            );
            if let Err(err) = &result {
                host.log(LogLevel::Error, &format!("generation failed: {err}"));
            }
            let _ = tx.send(GenEvent::Done(result));
        });
        self.generating_session_id = self.active_id.clone();
        self.generating_msg_open = false;
        self.generation_rx = Some(rx);
        self.generating = true;
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn new_empty_session() -> ChatSession {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    ChatSession {
        id: format!("session-{nanos}-{seq}"),
        title: String::new(),
        messages: Vec::new(),
        updated_at: now_unix(),
        model_ref: None,
    }
}

/// First line of the first user message, trimmed to a short sidebar title.
fn derive_title(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() <= TITLE_MAX_CHARS {
        return line.to_owned();
    }
    let truncated: String = line.chars().take(TITLE_MAX_CHARS).collect();
    format!("{}…", truncated.trim_end())
}

/// A short, log-safe label for a model reference (no secrets).
fn describe_model(model_ref: &LlmModelRef) -> String {
    match model_ref {
        LlmModelRef::LocalPreset { preset } => format!("local preset {preset:?}"),
        LlmModelRef::LocalCustom { id } => format!("local custom {id}"),
        LlmModelRef::Ollama { model_name } => format!("ollama {model_name}"),
        LlmModelRef::LmStudio { model_name } => format!("lm_studio {model_name}"),
        LlmModelRef::OpenAiCompatible {
            provider,
            model_name,
        } => format!("{provider:?} {model_name}"),
        LlmModelRef::Anthropic { model_name } => format!("anthropic {model_name}"),
        LlmModelRef::Gemini { model_name } => format!("gemini {model_name}"),
        LlmModelRef::Hermes { id } => format!("hermes agent {id}"),
    }
}

/// Folds the conversation so far into the system prompt as context. (v1 — true
/// multi-turn message arrays per provider come later.) The latest user turn is
/// sent separately as the user message, so it's excluded here.
fn build_system_with_history(system_prompt: &str, messages: &[ChatMessageDto]) -> String {
    let history: Vec<&ChatMessageDto> = messages.iter().collect();
    // Drop the trailing user turn (it is the current question).
    let context = history
        .split_last()
        .map(|(_, rest)| rest)
        .unwrap_or(&[][..]);
    if context.is_empty() {
        return system_prompt.to_owned();
    }
    let mut out = String::from(system_prompt);
    out.push_str("\n\nConversation so far:\n");
    for message in context {
        let speaker = match message.role {
            ChatRole::User => "User",
            ChatRole::Assistant => "Assistant",
            ChatRole::System => continue,
        };
        out.push_str(speaker);
        out.push_str(": ");
        out.push_str(&message.content);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ephemeral() -> ChatController {
        ChatController::with_sessions(vec![new_empty_session()], false)
    }

    #[test]
    fn empty_transcript_does_not_add_a_turn() {
        let mut chat = ephemeral();
        chat.on_transcript("   ".to_owned(), &AppSettings::default());
        assert!(chat.state(false, false).messages.is_empty());
    }

    #[test]
    fn first_user_message_seeds_the_session_title() {
        let mut chat = ephemeral();
        chat.on_transcript(
            "Wie ist das Wetter heute?".to_owned(),
            &AppSettings::default(),
        );
        let state = chat.state(false, false);
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].title, "Wie ist das Wetter heute?");
        assert_eq!(state.sessions[0].message_count, 1);
    }

    #[test]
    fn new_session_archives_a_non_empty_one() {
        let mut chat = ephemeral();
        chat.on_transcript("erste Frage".to_owned(), &AppSettings::default());
        chat.new_session();
        let state = chat.state(false, false);
        assert_eq!(state.sessions.len(), 2);
        assert!(state.messages.is_empty()); // active is the fresh empty one
        // A second new_session on an empty active does not pile up blanks.
        chat.new_session();
        assert_eq!(chat.state(false, false).sessions.len(), 2);
    }

    #[test]
    fn switch_session_changes_the_visible_transcript() {
        let mut chat = ephemeral();
        chat.on_transcript("erste".to_owned(), &AppSettings::default());
        let first_id = chat.state(false, false).active_session_id.clone();
        chat.new_session();
        chat.on_transcript("zweite".to_owned(), &AppSettings::default());
        chat.switch_session(&first_id);
        let state = chat.state(false, false);
        assert_eq!(state.active_session_id, first_id);
        assert_eq!(state.messages[0].content, "erste");
    }

    #[test]
    fn history_is_folded_into_system_prompt() {
        let messages = vec![
            ChatMessageDto {
                role: ChatRole::User,
                content: "hello".to_owned(),
            },
            ChatMessageDto {
                role: ChatRole::Assistant,
                content: "hi there".to_owned(),
            },
            ChatMessageDto {
                role: ChatRole::User,
                content: "how are you".to_owned(),
            },
        ];
        let system = build_system_with_history("SYS", &messages);
        assert!(system.contains("Conversation so far:"));
        assert!(system.contains("User: hello"));
        assert!(system.contains("Assistant: hi there"));
        // The current question is not part of the folded context.
        assert!(!system.contains("User: how are you"));
    }
}
