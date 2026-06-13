//! Chat plugin engine (#17).
//!
//! The chat reuses the main [`crate::dictation::DictationController`] for audio
//! capture + Whisper (already mic/device-initialized) — when a chat turn is
//! listening, the bridge routes the finished transcript here instead of
//! inserting it. This controller owns only the conversation + LLM generation.
//!
//! v1 is blocking (the whole answer is produced, then spoken). Streaming
//! sentence-by-sentence TTS is a follow-up. Multi-turn context is flattened
//! into the system prompt for now; true per-provider message arrays come later.

use std::{
    sync::{
        Arc,
        atomic::AtomicBool,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

use open_whisper_core::{
    AppSettings, ChatMessageDto, ChatPhase, ChatRole, ChatStateDto, LlmModelRef, LlmPreset,
};

use crate::llm;

const DEFAULT_SYSTEM_PROMPT: &str = "You are a friendly voice assistant. Answer briefly and conversationally, as if speaking aloud. Avoid markdown, lists and code blocks unless explicitly asked.";

pub struct ChatController {
    messages: Vec<ChatMessageDto>,
    model_ref: Option<LlmModelRef>,
    system_prompt: String,
    generation_rx: Option<Receiver<Result<String, String>>>,
    generating: bool,
    cancelled: Arc<AtomicBool>,
    revision: u64,
    error: Option<String>,
}

impl ChatController {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            model_ref: None,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_owned(),
            generation_rx: None,
            generating: false,
            cancelled: Arc::new(AtomicBool::new(false)),
            revision: 0,
            error: None,
        }
    }

    pub fn set_model(&mut self, model_ref: Option<LlmModelRef>) {
        self.model_ref = model_ref;
    }

    /// Clears the conversation and cancels any in-flight generation.
    pub fn reset(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.messages.clear();
        self.generation_rx = None;
        self.generating = false;
        self.error = None;
        self.cancelled = Arc::new(AtomicBool::new(false));
        self.revision += 1;
    }

    /// Called by the bridge when a chat-turn transcript is ready. Appends the
    /// user message and kicks off generation.
    pub fn on_transcript(&mut self, transcript: String, settings: &AppSettings) {
        let text = transcript.trim().to_owned();
        if text.is_empty() {
            // Nothing said — just re-arm without a turn.
            return;
        }
        self.error = None;
        self.messages.push(ChatMessageDto {
            role: ChatRole::User,
            content: text,
        });
        self.revision += 1;
        self.start_generation(settings);
    }

    /// Drained from `BridgeRuntime::poll()`. Collects a finished generation.
    pub fn poll(&mut self) {
        let Some(rx) = &self.generation_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(answer)) => {
                let trimmed = answer.trim().to_owned();
                self.generation_rx = None;
                self.generating = false;
                if trimmed.is_empty() {
                    self.error = Some("The model returned an empty answer.".to_owned());
                } else {
                    self.messages.push(ChatMessageDto {
                        role: ChatRole::Assistant,
                        content: trimmed,
                    });
                }
                self.revision += 1;
            }
            Ok(Err(err)) => {
                self.generation_rx = None;
                self.generating = false;
                self.error = Some(err);
                self.revision += 1;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.generation_rx = None;
                self.generating = false;
                self.error = Some("Chat generation stopped unexpectedly.".to_owned());
                self.revision += 1;
            }
        }
    }

    /// Builds the snapshot for the UI. `listening`/`transcribing` come from the
    /// shared dictation controller (chat reuses it).
    pub fn state(&self, listening: bool, transcribing: bool) -> ChatStateDto {
        let phase = if listening {
            ChatPhase::Listening
        } else if transcribing {
            ChatPhase::Transcribing
        } else if self.generating {
            ChatPhase::Generating
        } else {
            ChatPhase::Idle
        };
        ChatStateDto {
            phase,
            messages: self.messages.clone(),
            revision: self.revision,
            error: self.error.clone(),
        }
    }

    fn start_generation(&mut self, settings: &AppSettings) {
        let model_ref = self.model_ref.clone().unwrap_or(LlmModelRef::LocalPreset {
            preset: LlmPreset::default(),
        });
        let system = build_system_with_history(&self.system_prompt, &self.messages);
        let latest_user = self
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();

        let settings = settings.clone();
        let cancelled = self.cancelled.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = (|| {
                let provider = llm::provider_for(&model_ref, &settings)?;
                provider.generate(&system, &latest_user, &cancelled)
            })();
            let _ = tx.send(result);
        });
        self.generation_rx = Some(rx);
        self.generating = true;
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

    #[test]
    fn empty_transcript_does_not_add_a_turn() {
        let mut chat = ChatController::new();
        chat.on_transcript("   ".to_owned(), &AppSettings::default());
        assert!(chat.state(false, false).messages.is_empty());
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
