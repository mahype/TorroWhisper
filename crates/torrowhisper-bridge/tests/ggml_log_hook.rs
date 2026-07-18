//! Verifies that whisper.cpp / GGML log output reaches the shared log file.
//!
//! The hook (`whisper_rs::install_logging_hooks()` in `logging::init`, plus
//! the `log_backend` feature on whisper-rs) is the only thing that keeps
//! whisper.cpp's error output visible in a bundled app — without it the
//! lines go to stderr and are lost. This test lives in its own integration
//! binary so it fully owns the process-wide `OW_LOG_DIR` variable and the
//! Once-guarded logger initialisation.

use std::fs;

#[test]
fn whisper_errors_reach_the_log_file() {
    let dir = std::env::temp_dir().join(format!("ow-ggml-hook-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    // SAFETY: this integration test binary runs only this test, so no other
    // thread touches the environment or the logger.
    unsafe { std::env::set_var("OW_LOG_DIR", &dir) };

    // Reaches logging::init() (and thus the GGML hook) through the public
    // FFI surface, exactly like the app does.
    let raw = torrowhisper_bridge::ow_get_log_path();
    assert!(!raw.is_null());
    // SAFETY: `raw` came from the bridge allocator above and is freed once.
    unsafe { torrowhisper_bridge::ow_string_free(raw) };

    // Loading a model from a path that does not exist makes whisper.cpp emit
    // an error line through its log callback.
    let missing = dir.join("no-such-model.bin");
    let result = whisper_rs::WhisperContext::new_with_params(
        missing.to_str().expect("temp path is valid UTF-8"),
        whisper_rs::WhisperContextParameters::default(),
    );
    assert!(result.is_err(), "loading a missing model must fail");

    let log = fs::read_to_string(dir.join("torrowhisper.log")).expect("log file must exist");
    assert!(
        log.contains("whisper_logging_hook") || log.contains("ggml_logging_hook"),
        "whisper.cpp error output must be routed into the log file; log was:\n{log}"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Acceptance test for #43, task 1: the macOS build must compile in the Metal
/// backend so inference can run on the GPU. `print_system_info()` reports
/// `METAL = 1` exactly when the `metal` feature is active. On non-macOS targets
/// the feature is intentionally off, so this only runs on macOS.
#[cfg(target_os = "macos")]
#[test]
fn metal_backend_is_compiled_in_on_macos() {
    // whisper.cpp 1.8.x reports Metal as a `Metal : …` section; older versions
    // used `METAL = 1`. Either proves the backend is compiled in.
    let info = whisper_rs::print_system_info();
    assert!(
        info.contains("Metal :") || info.contains("METAL = 1"),
        "macOS build must compile in Metal (whisper system info: {info})"
    );
}
