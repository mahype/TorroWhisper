//! Worker process for local LLM post-processing.
//!
//! Speaks a line-based JSON protocol on stdin/stdout: one request object per
//! line in, one response object per line out. Diagnostics (including
//! llama.cpp's own output) go to stderr, which the bridge forwards into the
//! app log. The bridge cancels a generation or unloads the model by killing
//! this process; there is no in-band cancel command.
//!
//! This binary exists so llama-cpp-2 is never linked into the app process —
//! see Cargo.toml for the ggml symbol-collision background.

use std::{
    io::{self, BufRead, Write},
    num::NonZeroU32,
    path::PathBuf,
    time::Instant,
};

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use serde::{Deserialize, Serialize};

const MAX_OUTPUT_TOKENS: i32 = 512;
const STOP_SEQUENCE: &str = "<turn|>";
const PROMPT_BATCH_CAPACITY: usize = 512;

#[derive(Deserialize)]
struct Request {
    model_path: PathBuf,
    n_ctx: u32,
    system_prompt: String,
    text: String,
    /// `post_processing` (default) revises the text; `chat` answers it
    /// conversationally. Defaulted so older callers stay on post-processing.
    #[serde(default)]
    task: HelperTask,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum HelperTask {
    #[default]
    PostProcessing,
    Chat,
}

#[derive(Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn success(text: String) -> Self {
        Self {
            ok: true,
            text: Some(text),
            error: None,
        }
    }

    fn failure(error: String) -> Self {
        Self {
            ok: false,
            text: None,
            error: Some(error),
        }
    }
}

struct LoadedModel {
    path: PathBuf,
    model: LlamaModel,
}

fn main() {
    eprintln!(
        "llm helper started (pid {}, version {})",
        std::process::id(),
        env!("CARGO_PKG_VERSION")
    );

    let backend = LlamaBackend::init()
        .map_err(|err| format!("llama.cpp backend could not be initialized: {err}"));

    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut loaded: Option<LoadedModel> = None;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => match &backend {
                Ok(backend) => handle_request(backend, &mut loaded, request),
                Err(err) => Response::failure(err.clone()),
            },
            Err(err) => Response::failure(format!("Invalid helper request: {err}")),
        };

        let Ok(encoded) = serde_json::to_string(&response) else {
            break;
        };
        if writeln!(stdout, "{encoded}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

fn handle_request(
    backend: &LlamaBackend,
    loaded: &mut Option<LoadedModel>,
    request: Request,
) -> Response {
    if let Err(err) = ensure_loaded(backend, loaded, &request.model_path) {
        return Response::failure(err);
    }
    let model = &loaded.as_ref().expect("ensure_loaded just succeeded").model;

    match generate(
        backend,
        model,
        request.n_ctx,
        &request.system_prompt,
        &request.text,
        request.task,
    ) {
        Ok(text) => Response::success(text),
        Err(err) => Response::failure(err),
    }
}

fn ensure_loaded(
    backend: &LlamaBackend,
    loaded: &mut Option<LoadedModel>,
    target_path: &PathBuf,
) -> Result<(), String> {
    if loaded
        .as_ref()
        .is_some_and(|current| &current.path == target_path)
    {
        return Ok(());
    }

    // Drop the previous model first so both never occupy memory at once.
    *loaded = None;

    if !target_path.exists() {
        return Err(format!(
            "Language model file was not found at {}.",
            target_path.display()
        ));
    }

    let started = Instant::now();
    let params = LlamaModelParams::default().with_n_gpu_layers(1_000);
    let model = LlamaModel::load_from_file(backend, target_path, &params)
        .map_err(|err| format!("Language model could not be loaded: {err}"))?;
    eprintln!(
        "model loaded in {:.1}s ({})",
        started.elapsed().as_secs_f32(),
        target_path.display()
    );

    *loaded = Some(LoadedModel {
        path: target_path.clone(),
        model,
    });
    Ok(())
}

fn generate(
    backend: &LlamaBackend,
    model: &LlamaModel,
    n_ctx_value: u32,
    system_prompt: &str,
    user_text: &str,
    task: HelperTask,
) -> Result<String, String> {
    let n_ctx = NonZeroU32::new(n_ctx_value)
        .ok_or_else(|| "context_size must be greater than zero".to_owned())?;
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));

    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|err| format!("LLM context could not be created: {err}"))?;

    let prompt = match task {
        HelperTask::Chat => build_gemma_conversation_prompt(system_prompt, user_text),
        HelperTask::PostProcessing => build_gemma_chat_prompt(system_prompt, user_text),
    };
    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|err| format!("LLM tokenization failed: {err}"))?;

    if tokens.is_empty() {
        return Err("LLM prompt produced no tokens.".to_owned());
    }

    let n_input = tokens.len() as i32;
    if n_input + MAX_OUTPUT_TOKENS >= n_ctx_value as i32 {
        return Err(format!(
            "Input is too long for the language model context window ({} tokens, max {}).",
            n_input,
            n_ctx_value as i32 - MAX_OUTPUT_TOKENS
        ));
    }

    let mut batch = LlamaBatch::new(PROMPT_BATCH_CAPACITY.max(tokens.len()), 1);
    for (i, token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(*token, i as i32, &[0], is_last)
            .map_err(|err| format!("LLM batch could not be populated: {err}"))?;
    }

    ctx.decode(&mut batch)
        .map_err(|err| format!("LLM decode of the prompt failed: {err}"))?;

    let mut sampler = LlamaSampler::chain_simple([LlamaSampler::greedy()]);

    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut output = String::new();
    let mut n_cur = n_input;
    let n_max = n_input + MAX_OUTPUT_TOKENS;

    while n_cur < n_max {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        sampler.accept(token);

        if model.is_eog_token(token) {
            break;
        }

        let piece = model
            .token_to_piece(token, &mut decoder, false, None)
            .map_err(|err| format!("LLM detokenization failed: {err}"))?;

        output.push_str(&piece);

        if let Some(idx) = output.find(STOP_SEQUENCE) {
            output.truncate(idx);
            break;
        }

        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|err| format!("LLM batch update failed: {err}"))?;
        n_cur += 1;

        ctx.decode(&mut batch)
            .map_err(|err| format!("LLM decode failed: {err}"))?;
    }

    let trimmed = output.trim().to_owned();
    if trimmed.is_empty() {
        return Err("The language model returned no text.".to_owned());
    }

    Ok(trimmed)
}

fn build_gemma_chat_prompt(mode_instruction: &str, transcript: &str) -> String {
    let instruction = mode_instruction.trim();
    let text = transcript.trim();

    let body = if instruction.is_empty() {
        format!(
            "Du bereinigst einen diktierten Text. Korrigiere Satzzeichen, Grossschreibung und offensichtliche Erkennungsfehler, ohne den Inhalt zu veraendern.\n\nText zum Bereinigen:\n{text}\n\nGib ausschliesslich den bereinigten Text zurueck, ohne Erklaerungen, Kommentare oder Anfuehrungszeichen."
        )
    } else {
        format!(
            "Du ueberarbeitest einen diktierten Text nach folgender Anweisung. Wende die Anweisung auf den Text an, ohne die Anweisung selbst zurueckzugeben.\n\nAnweisung:\n{instruction}\n\nText zum Ueberarbeiten:\n{text}\n\nGib ausschliesslich den ueberarbeiteten Text zurueck, ohne Erklaerungen, Kommentare oder Anfuehrungszeichen."
        )
    };

    format!("<bos><|turn>user\n{body}<turn|>\n<|turn>model\n")
}

/// Conversational prompt for the chat plugin. Gemma has no system role, so the
/// system prompt (which already carries any folded history) is prepended to the
/// user's turn, then the model is asked to respond. Uses Gemma's canonical
/// `<start_of_turn>` template so the model answers instead of revising the text.
fn build_gemma_conversation_prompt(system_prompt: &str, user_text: &str) -> String {
    let system = system_prompt.trim();
    let user = user_text.trim();
    let turn = if system.is_empty() {
        user.to_owned()
    } else {
        format!("{system}\n\n{user}")
    };
    // AddBos::Always prepends <bos>, so it is not included literally here.
    format!("<start_of_turn>user\n{turn}<end_of_turn>\n<start_of_turn>model\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemma_prompt_labels_instruction_and_text() {
        let prompt = build_gemma_chat_prompt("Schreibe foermlicher.", "hallo welt");
        assert!(prompt.starts_with("<bos><|turn>user\n"));
        assert!(prompt.contains("Anweisung:\nSchreibe foermlicher."));
        assert!(prompt.contains("Text zum Ueberarbeiten:\nhallo welt"));
        assert!(prompt.contains("ohne Erklaerungen"));
        assert!(prompt.ends_with("<|turn>model\n"));
    }

    #[test]
    fn gemma_prompt_falls_back_to_cleanup_when_instruction_empty() {
        let prompt = build_gemma_chat_prompt("   ", "hallo welt");
        assert!(prompt.contains("bereinigst"));
        assert!(prompt.contains("Text zum Bereinigen:\nhallo welt"));
    }

    #[test]
    fn conversation_prompt_is_a_real_turn_not_a_revision() {
        let prompt = build_gemma_conversation_prompt("You are a helpful assistant.", "Wie geht's?");
        assert!(prompt.starts_with("<start_of_turn>user\n"));
        assert!(prompt.contains("You are a helpful assistant.\n\nWie geht's?"));
        assert!(prompt.ends_with("<start_of_turn>model\n"));
        // Must NOT carry the post-processing "revise the text" framing.
        assert!(!prompt.contains("bereinig"));
        assert!(!prompt.contains("ueberarbeit"));
    }

    #[test]
    fn request_defaults_to_post_processing_task() {
        let line = r#"{"model_path":"/tmp/m.gguf","n_ctx":2048,"system_prompt":"p","text":"t"}"#;
        let request: Request = serde_json::from_str(line).unwrap();
        assert_eq!(request.task, HelperTask::PostProcessing);
    }

    #[test]
    fn request_parses_from_protocol_line() {
        let line = r#"{"model_path":"/tmp/m.gguf","n_ctx":2048,"system_prompt":"p","text":"t"}"#;
        let request: Request = serde_json::from_str(line).unwrap();
        assert_eq!(request.model_path, PathBuf::from("/tmp/m.gguf"));
        assert_eq!(request.n_ctx, 2048);
    }

    #[test]
    fn responses_serialize_to_single_lines() {
        let ok = serde_json::to_string(&Response::success("a\nb".to_owned())).unwrap();
        assert!(!ok.contains('\n'));
        let err = serde_json::to_string(&Response::failure("kaputt".to_owned())).unwrap();
        assert!(!err.contains('\n'));
        assert!(err.contains("kaputt"));
    }
}
