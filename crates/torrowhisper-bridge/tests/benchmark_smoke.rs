//! End-to-end smoke test for the whisper benchmark (#43).
//!
//! Exercises the real FFI entry point against whatever models are installed on
//! this machine, so it is `#[ignore]` by default (CI has no models). Run it
//! locally with:
//!
//! ```sh
//! cargo test -p torrowhisper-bridge --test benchmark_smoke -- --ignored --nocapture
//! ```
//!
//! It proves the full path works: reference-clip decode → model load → Metal
//! inference → RTF / memory / quality reporting.

use std::ffi::{CStr, CString};

#[ignore = "requires locally installed whisper models; run manually"]
#[test]
fn benchmark_runs_over_installed_models() {
    let request = CString::new(r#"{"thread_counts":[1,4,8]}"#).unwrap();
    let raw = torrowhisper_bridge::ow_run_whisper_benchmark(request.as_ptr());
    assert!(!raw.is_null());
    // SAFETY: `raw` came from the bridge allocator and is freed once below.
    let json = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
    unsafe { torrowhisper_bridge::ow_string_free(raw) };

    println!("benchmark report:\n{json}");
    assert!(
        json.contains("\"ok\":true"),
        "benchmark FFI must succeed: {json}"
    );
    assert!(json.contains("\"rows\""), "report must carry rows");
    // At least one real measurement (inference_secs > 0) should be present when
    // any model is installed.
    assert!(
        json.contains("\"inference_secs\""),
        "report rows must contain inference timing"
    );
}
