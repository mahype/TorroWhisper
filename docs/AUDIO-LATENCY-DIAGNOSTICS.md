# Audio latency diagnostics (first step)

These diagnostics distinguish causes; they are **not a measured fix** for the
reported C920 input underrun, output-cue deadlines, or Parakeet call-time spikes
up to 53.55 seconds. A live reproduction is still required. No audio-device
buffer sizes, user settings, backend selection, or CoreAudio restart policy are
changed. The waveform bridge call stays on MainActor because `BridgeRuntime`
is thread-local; moving it to `Task.detached` would access another runtime.

## Reading the existing app log

New Rust lines use `[latency]`; Swift waveform lines use `[app] latency`.
`recording=N` is a process-local counter shared across capture, the level DTO,
waveform, cues, and final backend work. It resets on process restart; combine
it with the existing app-session boundary. `recording=0` means uncorrelated
(e.g. a cancellation cue after recording has already ended).

Capture summaries are cumulative, at most once per five seconds from the
control/status poll, plus capture teardown and a final pre-backend summary.
Waveform summaries are cumulative, at most once per five seconds while ticking,
plus stop/session boundaries. Realtime callbacks only update fixed-size relaxed
atomic counters; they do not format diagnostics, log, or write files. Concurrent
capture summaries are approximate, not transactionally consistent snapshots.
No new diagnostics contain audio samples, level histories, transcript text,
error payloads, device names, paths, or runtime/settings dumps.

| Field/stage | Meaning |
| --- | --- |
| `main_cadence`, `main_lateness` (ms) | Gap between executed MainActor ticks, and positive excess over 1/30 s; not an audio-device clock. |
| `ffi_decode` (ms) | Synchronous Swift FFI call plus JSON decoding, including failures, excluding publication/logging. |
| `bridge` (µs) | Rust level endpoint including response serialization; excludes Swift decoding. |
| `level_snapshot`, `level_try_wait`, `level_hold` (µs) | Level snapshot work, nonblocking mutex attempt, and successful mutex hold. `level_misses` counts contention/poison; cached levels are returned unchanged. |
| `callback`, `gap`, `lateness` (µs) | Capture handler execution (including conversion), inter-entry gap, and positive excess over the **previous** callback's frame-duration budget. These are host entry times, not CPAL device timestamps. Device swaps also appear as gaps. |
| `capture_wait`, `capture_hold` (µs) | Capture's buffer-lock acquisition and hold, separated from conversion and callback scheduling. |
| `streaming_wait`, `streaming_hold`, `finish_wait`, `finish_hold` (µs) | Other buffer owners: incremental live-preview copy and final audio take. |
| `capture_errors`, `capture_xruns`, `capture_lock_errors` | Input error callbacks, CPAL `Xrun` subset (underrun/overrun), and poisoned-lock failures. Not output-cue errors. |
| `input.build`, `input.play`, `input.first_callback_wait`, `input.drop` (ms) | Capture stream setup, start, readiness wait, and teardown, not buffer-lock time. |
| `cue.<kind>.device/config/build/play/scheduled_wait/drop` (ms) | Output-device discovery/config, stream build/start, the existing sleep budget, and teardown. Begin lines identify a stage still blocked. |
| `cue.start.deadline_wait` (ms) | Existing bounded start-cue wait; `ok=false` means timeout/disconnection, not proof that playback eventually failed. The worker can finish later under the same recording ID. |
| Cue `callbacks`, `first_from_build_us`, `errors`, `xruns` | Output callbacks, first callback delay from build entry, and output-error counters. `callbacks=0` means no first callback was observed. Scheduled waiting and callbacks do **not** prove audible playback (`playback_verified=false`). |
| `backend.streaming_join` (ms) | Wait for an existing live-preview worker before final inference. |
| `backend.<engine>.total`, `backend.<engine>.call` (ms) | Final backend wrapper versus backend call: Whisper inference helper (state creation/inference-lock wait/inference), cloud request, or Parakeet runtime (model-handle lookup, FluidAudio call, normalization). `ok=false` still records elapsed time without error text. Existing successful stage timings retain resample/state/inference detail. |

Each aggregate reports count, mean, and maximum, never a per-tick trace.
Waveform start is idempotent. Stop invalidates queued timer tasks by generation;
old tasks cannot poll/publish after stop or a later start. A fresh recording has
its own snapshot cache; contention never replaces good bars with empty data.

## Verification and live reproduction

Deterministic Rust tests hold the mutex on the same thread to prove that level
reads do not wait, test poison/cache behavior, variable callback budgets, error
classification, and separate cue counters. Swift tests inject a clock, bridge
reader, and log sink to test lifecycle, cadence summaries, failures, privacy,
correlation, and rate limiting without a microphone, sleeps, or settings writes.

For live reproduction, follow `AGENTS.md` before starting a test instance. Keep
the user's current settings. Compare a C920 take with a built-in microphone
take, recording the relevant session/recording IDs. Inspect input xruns and
callback/lock maxima separately from MainActor/bridge timings, output-cue stage
deadlines, and backend call durations. Do not infer an xrun from a UI stall, an
inference stall from a cue timeout, or resolution merely from passing tests.
