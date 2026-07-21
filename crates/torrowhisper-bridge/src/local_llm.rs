//! Client for the local LLM helper process.
//!
//! llama-cpp-2 is deliberately NOT linked into this crate: whisper-rs and
//! llama-cpp-2 each bundle their own ggml revision, and statically linking
//! both into the app binary mixes the two copies into one runtime (the linker
//! keeps only one set of the duplicated `ggml_*` symbols). The ggml_op enums
//! of the two revisions are shifted against each other, so the mixed runtime
//! dispatches wrong compute kernels — memory corruption and intermittent
//! crashes of the whole app. The model therefore runs in a separate
//! `torrowhisper-llm-helper` process and this module only speaks its
//! line-based JSON protocol over stdin/stdout.
//!
//! Cancellation and auto-unload are both implemented by killing the helper;
//! the next generation request simply spawns a fresh one.

use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, channel},
    },
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use torrowhisper_core::LlmPreset;

use crate::llm_model_manager::default_llm_model_path;

const HELPER_BINARY_NAME: &str = "torrowhisper-llm-helper";
const CUSTOM_CONTEXT_SIZE: u32 = 2_048;
/// Hard ceiling for one generation including a cold model load. The user can
/// always cancel earlier; this only protects against a wedged helper.
const GENERATION_TIMEOUT: Duration = Duration::from_secs(300);
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalLlmKey {
    Preset(LlmPreset),
    Custom(String),
}

#[derive(Deserialize)]
struct HelperResponse {
    ok: bool,
    text: Option<String>,
    error: Option<String>,
}

struct HelperProcess {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<String>,
}

impl HelperProcess {
    fn spawn() -> Result<Self, String> {
        let path = helper_executable_path()?;
        let mut child = Command::new(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                format!(
                    "LLM helper could not be started ({}): {err}",
                    path.display()
                )
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "LLM helper stdout could not be captured.".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "LLM helper stderr could not be captured.".to_owned())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "LLM helper stdin could not be captured.".to_owned())?;

        let (tx, responses) = channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                log::info!(target: "llm-helper", "{line}");
            }
        });

        log::info!(
            target: "dictation",
            "llm helper spawned (pid {}, {})",
            child.id(),
            path.display()
        );

        Ok(Self {
            child,
            stdin,
            responses,
        })
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

pub struct LocalLlmRuntime {
    helper: Option<HelperProcess>,
    loaded: Option<LocalLlmKey>,
    last_used: Instant,
}

impl LocalLlmRuntime {
    pub fn new() -> Self {
        Self {
            helper: None,
            loaded: None,
            last_used: Instant::now(),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    pub fn loaded_preset(&self) -> Option<LlmPreset> {
        match self.loaded.as_ref()? {
            LocalLlmKey::Preset(preset) => Some(*preset),
            LocalLlmKey::Custom(_) => None,
        }
    }

    pub fn loaded_custom_id(&self) -> Option<String> {
        match self.loaded.as_ref()? {
            LocalLlmKey::Preset(_) => None,
            LocalLlmKey::Custom(id) => Some(id.clone()),
        }
    }

    pub fn maybe_unload(&mut self, auto_unload_secs: u32) {
        if auto_unload_secs == 0 {
            return;
        }
        if self.helper.is_none() && self.loaded.is_none() {
            return;
        }
        if self.last_used.elapsed() >= Duration::from_secs(auto_unload_secs as u64) {
            self.unload();
        }
    }

    pub fn unload(&mut self) {
        if let Some(mut helper) = self.helper.take() {
            helper.kill();
        }
        self.loaded = None;
    }

    pub fn generate(
        &mut self,
        preset: LlmPreset,
        system_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.generate_preset(
            preset,
            system_prompt,
            user_text,
            LocalLlmTask::PostProcessing,
            cancelled,
        )
    }

    /// Conversational generation (chat plugin) — the helper answers the user
    /// instead of revising the text.
    pub fn chat(
        &mut self,
        preset: LlmPreset,
        system_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.generate_preset(
            preset,
            system_prompt,
            user_text,
            LocalLlmTask::Chat,
            cancelled,
        )
    }

    fn generate_preset(
        &mut self,
        preset: LlmPreset,
        system_prompt: &str,
        user_text: &str,
        task: LocalLlmTask,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        let target_path = default_llm_model_path(preset)?;
        if !target_path.exists() {
            return Err(format!(
                "Local language model ({}) has not been downloaded yet.",
                preset.display_label()
            ));
        }
        self.generate_any(
            LocalLlmKey::Preset(preset),
            &target_path,
            preset.context_size(),
            system_prompt,
            user_text,
            task,
            cancelled,
        )
    }

    pub fn generate_custom(
        &mut self,
        id: &str,
        display_name: &str,
        path: &Path,
        system_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.generate_custom_any(
            id,
            display_name,
            path,
            system_prompt,
            user_text,
            LocalLlmTask::PostProcessing,
            cancelled,
        )
    }

    pub fn chat_custom(
        &mut self,
        id: &str,
        display_name: &str,
        path: &Path,
        system_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.generate_custom_any(
            id,
            display_name,
            path,
            system_prompt,
            user_text,
            LocalLlmTask::Chat,
            cancelled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_custom_any(
        &mut self,
        id: &str,
        display_name: &str,
        path: &Path,
        system_prompt: &str,
        user_text: &str,
        task: LocalLlmTask,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        if !path.exists() {
            return Err(format!(
                "Custom language model '{}' was not found at {}.",
                display_name,
                path.display()
            ));
        }
        self.generate_any(
            LocalLlmKey::Custom(id.to_owned()),
            path,
            CUSTOM_CONTEXT_SIZE,
            system_prompt,
            user_text,
            task,
            cancelled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_any(
        &mut self,
        key: LocalLlmKey,
        target_path: &Path,
        n_ctx: u32,
        system_prompt: &str,
        user_text: &str,
        task: LocalLlmTask,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.last_used = Instant::now();
        self.ensure_helper()?;

        let request = serde_json::json!({
            "model_path": target_path,
            "n_ctx": n_ctx,
            "system_prompt": system_prompt,
            "text": user_text,
            "task": task.as_request_str(),
        });
        let helper = self.helper.as_mut().expect("ensure_helper just succeeded");
        let mut encoded = request.to_string();
        encoded.push('\n');
        if helper.stdin.write_all(encoded.as_bytes()).is_err() || helper.stdin.flush().is_err() {
            self.unload();
            return Err("LLM helper is not reachable (stdin closed). Please try again.".to_owned());
        }

        let started = Instant::now();
        loop {
            if cancelled.load(Ordering::Relaxed) {
                self.unload();
                return Err("Post-processing cancelled.".to_owned());
            }
            if started.elapsed() > GENERATION_TIMEOUT {
                self.unload();
                return Err(format!(
                    "Local post-processing timed out after {}s.",
                    GENERATION_TIMEOUT.as_secs()
                ));
            }

            let helper = self.helper.as_mut().expect("helper set for this request");
            match helper.responses.recv_timeout(CANCEL_POLL_INTERVAL) {
                Ok(line) => {
                    let response: HelperResponse = serde_json::from_str(&line)
                        .map_err(|err| format!("LLM helper sent an unreadable response: {err}"))?;
                    self.last_used = Instant::now();
                    return if response.ok {
                        // The helper keeps exactly this model loaded until the
                        // next request or until we kill it.
                        self.loaded = Some(key);
                        response
                            .text
                            .ok_or_else(|| "LLM helper response contained no text.".to_owned())
                    } else {
                        Err(response
                            .error
                            .unwrap_or_else(|| "LLM helper reported an unknown error.".to_owned()))
                    };
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    let status = self
                        .helper
                        .as_mut()
                        .and_then(|helper| helper.child.try_wait().ok().flatten())
                        .map(|status| status.to_string())
                        .unwrap_or_else(|| "unknown exit status".to_owned());
                    self.unload();
                    log::error!(
                        target: "dictation",
                        "llm helper exited mid-generation ({status})"
                    );
                    return Err(format!(
                        "Local post-processing crashed ({status}). The transcript was kept unprocessed."
                    ));
                }
            }
        }
    }

    fn ensure_helper(&mut self) -> Result<(), String> {
        let alive = self.helper.as_mut().is_some_and(HelperProcess::is_alive);
        if !alive {
            self.unload();
            self.helper = Some(HelperProcess::spawn()?);
        }
        Ok(())
    }
}

impl Default for LocalLlmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

fn helper_executable_path() -> Result<PathBuf, String> {
    // Dev override (set by scripts/dev.sh): the helper lives in target/, not
    // next to the SwiftPM-built executable.
    if let Ok(custom) = env::var("OW_LLM_HELPER")
        && !custom.trim().is_empty()
    {
        let path = PathBuf::from(custom);
        return if path.exists() {
            Ok(path)
        } else {
            Err(format!(
                "OW_LLM_HELPER points to {}, but no file exists there.",
                path.display()
            ))
        };
    }

    let exe = env::current_exe()
        .map_err(|err| format!("Own executable path could not be determined: {err}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "Own executable has no parent directory.".to_owned())?;
    let candidate = dir.join(HELPER_BINARY_NAME);
    if candidate.exists() {
        return Ok(candidate);
    }

    Err(format!(
        "LLM helper binary is missing ({}). Local post-processing is unavailable; reinstalling the app should fix this.",
        candidate.display()
    ))
}

/// Whether the helper should revise the text (post-processing) or answer it
/// conversationally (chat). Serialized into the helper request `task` field.
#[derive(Clone, Copy)]
enum LocalLlmTask {
    PostProcessing,
    Chat,
}

impl LocalLlmTask {
    fn as_request_str(self) -> &'static str {
        match self {
            LocalLlmTask::PostProcessing => "post_processing",
            LocalLlmTask::Chat => "chat",
        }
    }
}

static SHARED_RUNTIME: OnceLock<Mutex<LocalLlmRuntime>> = OnceLock::new();

pub fn shared_runtime() -> &'static Mutex<LocalLlmRuntime> {
    SHARED_RUNTIME.get_or_init(|| Mutex::new(LocalLlmRuntime::new()))
}

pub fn generate_with_shared_runtime(
    preset: LlmPreset,
    system_prompt: &str,
    user_text: &str,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    let mut runtime = shared_runtime()
        .lock()
        .map_err(|_| "Local language model runtime mutex was poisoned.".to_owned())?;
    runtime.generate(preset, system_prompt, user_text, cancelled)
}

pub fn generate_with_custom_path(
    id: &str,
    display_name: &str,
    path: &Path,
    system_prompt: &str,
    user_text: &str,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    let mut runtime = shared_runtime()
        .lock()
        .map_err(|_| "Local language model runtime mutex was poisoned.".to_owned())?;
    runtime.generate_custom(id, display_name, path, system_prompt, user_text, cancelled)
}

pub fn chat_with_shared_runtime(
    preset: LlmPreset,
    system_prompt: &str,
    user_text: &str,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    let mut runtime = shared_runtime()
        .lock()
        .map_err(|_| "Local language model runtime mutex was poisoned.".to_owned())?;
    runtime.chat(preset, system_prompt, user_text, cancelled)
}

pub fn chat_with_custom_path(
    id: &str,
    display_name: &str,
    path: &Path,
    system_prompt: &str,
    user_text: &str,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    let mut runtime = shared_runtime()
        .lock()
        .map_err(|_| "Local language model runtime mutex was poisoned.".to_owned())?;
    runtime.chat_custom(id, display_name, path, system_prompt, user_text, cancelled)
}

pub fn maybe_unload_shared_runtime(auto_unload_secs: u32) {
    if let Some(mutex) = SHARED_RUNTIME.get()
        && let Ok(mut runtime) = mutex.lock()
    {
        runtime.maybe_unload(auto_unload_secs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_starts_unloaded() {
        let runtime = LocalLlmRuntime::new();
        assert!(!runtime.is_loaded());
        assert!(runtime.loaded_preset().is_none());
        assert!(runtime.loaded_custom_id().is_none());
    }

    #[test]
    fn maybe_unload_noop_on_zero_secs() {
        let mut runtime = LocalLlmRuntime::new();
        runtime.last_used = Instant::now() - Duration::from_secs(3_600);
        runtime.maybe_unload(0);
        assert!(!runtime.is_loaded());
    }

    #[test]
    fn unload_clears_loaded_state() {
        let mut runtime = LocalLlmRuntime::new();
        runtime.loaded = Some(LocalLlmKey::Preset(LlmPreset::Small));
        runtime.unload();
        assert!(!runtime.is_loaded());
    }

    /// Covers both the env override validation and the full
    /// spawn → request → response round trip against a fake helper script.
    /// One test on purpose: parallel tests must not race on OW_LLM_HELPER.
    #[test]
    fn helper_override_and_round_trip() {
        // SAFETY: only this test touches OW_LLM_HELPER.
        unsafe { env::set_var("OW_LLM_HELPER", "/nonexistent/helper-binary") };
        assert!(helper_executable_path().is_err());

        let dir = env::temp_dir().join(format!("ow-llm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake_helper = dir.join("fake-helper.sh");
        std::fs::write(
            &fake_helper,
            "#!/bin/sh\nwhile read -r _line; do printf '%s\\n' '{\"ok\":true,\"text\":\"fake antwort\"}'; done\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&fake_helper).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&fake_helper, perms).unwrap();

        unsafe { env::set_var("OW_LLM_HELPER", &fake_helper) };
        let mut runtime = LocalLlmRuntime::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        // The "model file" only has to exist; the fake helper ignores it.
        let result = runtime.generate_custom(
            "test-id",
            "Testmodell",
            &fake_helper,
            "prompt",
            "text",
            &cancelled,
        );
        unsafe { env::remove_var("OW_LLM_HELPER") };
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(result.unwrap(), "fake antwort");
        assert_eq!(runtime.loaded_custom_id().as_deref(), Some("test-id"));
        runtime.unload();
        assert!(!runtime.is_loaded());
    }
}
