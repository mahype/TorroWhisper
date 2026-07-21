# Architecture

TorroWhisper is split into a **shared Rust core**, an **FFI bridge** that exposes JSON-over-C to native UIs, and **platform-specific UI shells** (currently just macOS). The UI layers are kept thin: everything stateful — settings, dictation, model management, hotkeys — lives in the bridge.

## High-level diagram

```
┌─────────────────────────────────────────────────────────┐
│  apps/torrowhisper-macos  (SwiftUI + AppKit menu bar)   │
│                                                         │
│  TorroWhisperApp → AppDelegate → SettingsView, …      │
│       │                                                 │
│       ▼                                                 │
│  BridgeClient  (Swift wrapper around the C functions)   │
└───────────┬─────────────────────────────────────────────┘
            │   C FFI: JSON-in, JSON-out strings
            │   Header: Bridge/TorroWhisperBridgeFFI.h
            ▼
┌─────────────────────────────────────────────────────────┐
│  crates/torrowhisper-bridge  (staticlib + rlib)         │
│                                                         │
│  lib.rs          FFI entry points, BridgeRuntime        │
│  dictation.rs    cpal capture + Parakeet/Whisper routing│
│  parakeet.rs     FluidAudio/Core ML transcription       │
│  model_manager   Whisper model download & cleanup       │
│  llm_model_…     Local LLM GGUF download & cleanup      │
│  local_llm.rs    client for the LLM helper process      │
│  post_processing Local + Ollama / LM Studio dispatcher  │
│  remote_models   Ollama / LM Studio model listing       │
│  autostart.rs    auto-launch wrapper (Dev fallback)     │
│  settings_store  settings.json read/write               │
│  text_inserter   arboard + enigo (clipboard + paste)    │
│  permission_…    TCC / platform-specific probes         │
└──────┬────┬─────────────────────────────────────────────┘
       │    │ line-based JSON over stdin/stdout
       │    ▼
       │ ┌──────────────────────────────────────────────┐
       │ │  crates/torrowhisper-llm-helper (executable) │
       │ │  llama-cpp-2 (Gemma 4, Metal) in its own     │
       │ │  process — whisper-rs and llama-cpp-2 bundle │
       │ │  incompatible ggml revisions and must never  │
       │ │  be linked into the same binary              │
       │ └──────────────────────────────────────────────┘
       │ pure-Rust types & enums
       ▼
┌─────────────────────────────────────────────────────────┐
│  crates/torrowhisper-core  (no_std-friendly domain)     │
│                                                         │
│  AppSettings, ModelPreset, LlmPreset,                   │
│  ProcessingMode, PostProcessingChoice,                  │
│  CustomLlmModel, StartupBehavior, TriggerMode,          │
│  WaveformStyle, WaveformColor, DeviceDto,               │
│  RuntimeStatusDto, …                                    │
└─────────────────────────────────────────────────────────┘
```

## Crates

### `torrowhisper-core`

Pure-Rust domain types shared between the bridge and any future shells or tools. No I/O, no OS calls — just `serde`-friendly DTOs, enums, and configuration structs.

Key types:

- `AppSettings` — the root of everything the user configures.
- `StartupBehavior` — `AskOnFirstLaunch`, `LaunchAtLogin`, `ManualLaunch`.
- `TriggerMode` — `PushToTalk`, `Toggle`.
- `TranscriptionBackend` — `Parakeet` (Apple-Silicon default) or `Whisper` (Intel fallback and optional alternative).
- `ModelPreset` — the seven optional Whisper presets (Tiny, Light, Standard, Quality, Large v3 Turbo Q5_0, Large v3 Turbo, Large v3).
- `LlmModelRef::AppleSystem` — the system-managed Apple Foundation Models post-processing backend.
- `LlmPreset` — local Gemma 4 sizes (`Small`, `Medium`, `Large`).
- `ProcessingMode` — a user-defined post-processing template: `id`, `name`, `prompt`, and an optional `post_processing_choice` that overrides the global backend for this Mode.
- `PostProcessingChoice` — tagged enum: `LocalPreset { preset }`, `LocalCustom { id }`, `Ollama { model_name }`, `LmStudio { model_name }`.
- `CustomLlmModel` + `CustomLlmSource` — user-added local GGUF files (either `LocalPath` or `DownloadUrl`).
- `WaveformStyle`, `WaveformColor` — recording-indicator presentation.
- `DeviceDto`, `RuntimeStatusDto`, `RecordingLevelsDto`, `DiagnosticsDto`.

### `torrowhisper-bridge`

Built as both `staticlib` (for Swift linkage) and `rlib` (for Rust-side testing). All public surface goes through `extern "C"` functions in `src/lib.rs` that take and return UTF-8 JSON strings.

Module responsibilities:

| Module | Responsibility |
| --- | --- |
| [lib.rs](../crates/torrowhisper-bridge/src/lib.rs) | FFI entry points, the `BridgeRuntime` aggregate that owns all subsystems |
| [dictation.rs](../crates/torrowhisper-bridge/src/dictation.rs) | Mic capture via `cpal`, VAD-based silence detection, and routing to Parakeet or Whisper |
| [parakeet.rs](../crates/torrowhisper-bridge/src/parakeet.rs) | NVIDIA Parakeet TDT v3 through FluidAudio, Core ML, and Apple Neural Engine; first-run preparation and process-lifetime state |
| [model_manager.rs](../crates/torrowhisper-bridge/src/model_manager.rs) | Download, list, and delete Whisper `.bin` models |
| [llm_model_manager.rs](../crates/torrowhisper-bridge/src/llm_model_manager.rs) | Download, list, and delete local LLM GGUF files (Gemma 4 presets and user-added custom models) |
| [local_llm.rs](../crates/torrowhisper-bridge/src/local_llm.rs) | Client for the `torrowhisper-llm-helper` process (line-based JSON over stdin/stdout); idle-based auto-unload and cancellation by killing the helper. llama-cpp-2 lives only in the helper: its bundled ggml is incompatible with whisper-rs's, and linking both into one binary mixes the duplicated symbols and crashes the app |
| [llm/apple_foundation.rs](../crates/torrowhisper-bridge/src/llm/apple_foundation.rs) | System-managed Apple Foundation Models provider for on-device post-processing |
| [post_processing.rs](../crates/torrowhisper-bridge/src/post_processing.rs) | Applies the active `ProcessingMode`'s prompt via the resolved model provider; 45 s timeout; cancellable |
| [remote_models.rs](../crates/torrowhisper-bridge/src/remote_models.rs) | Lists models exposed by a running Ollama or LM Studio endpoint for the backend picker |
| [autostart.rs](../crates/torrowhisper-bridge/src/autostart.rs) | `auto-launch` crate wrapper; writes a `LaunchAgent` plist on macOS, XDG autostart on Linux, registry on Windows. Used as a fallback when the app is **not** running from a `.app` bundle. |
| [settings_store.rs](../crates/torrowhisper-bridge/src/settings_store.rs) | Reads/writes `~/Library/Application Support/torrowhisper/settings.json` |
| [text_inserter.rs](../crates/torrowhisper-bridge/src/text_inserter.rs) | `arboard` (clipboard) + `enigo` (simulated paste); clipboard fallback when paste fails |
| [permission_diagnostics.rs](../crates/torrowhisper-bridge/src/permission_diagnostics.rs) | Platform-specific permission probing (TCC on macOS) |

### `apps/torrowhisper-macos`

Swift Package (SPM) producing a single executable, `TorroWhisper`. Uses SwiftUI for onboarding/settings windows, AppKit for the menu bar integration. Links to `libtorrowhisper_bridge.a` from the target directory via unsafe linker flags in `Package.swift`.

Key files:

| File | Responsibility |
| --- | --- |
| [TorroWhisperApp.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/TorroWhisperApp.swift) | `@main` entry point |
| [AppDelegate.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/AppDelegate.swift) | Menu bar icon, window lifecycle, login-item bootstrap |
| [BridgeClient.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/BridgeClient.swift) | Typed wrapper around every `ow_*` FFI call |
| [BridgeModels.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/BridgeModels.swift) | `Codable` DTOs matching the Rust side |
| [AppModel.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/AppModel.swift) | Observable state for the UI |
| [SettingsView.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/SettingsView.swift) | Settings window with a sidebar-driven `SettingsSection` switch (Recording, Modes, Language Models, Start & Behavior, Updates, Diagnostics, Help) |
| [AppUIComponents.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/AppUIComponents.swift) | Shared settings primitives and the Mode-Editor sheet (create / edit / delete a `ProcessingMode`) |
| [LanguageModelsManagerSheet.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/LanguageModelsManagerSheet.swift) | Unified Whisper + local LLM model management (download / delete / import custom GGUF) |
| [RecordingIndicatorView.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/RecordingIndicatorView.swift) | Floating live waveform indicator with phase-specific styling (recording / transcribing / post-processing / model-not-ready) |
| [HotkeyRecorderField.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/HotkeyRecorderField.swift) / [HotkeyAssignmentAdvisor.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/HotkeyAssignmentAdvisor.swift) | Inline hotkey capture with collision and single-key warnings |
| [UpdaterController.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/UpdaterController.swift), [UpdatesSettingsView.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/UpdatesSettingsView.swift) | Sparkle integration (see *Auto-updates* below) |
| [OnboardingView.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/OnboardingView.swift) | First-run guided setup; re-launchable from the Help tab |
| [Localization.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/Localization.swift) | `L(_:locale:)` string helper, `LocalizedRoot` environment wrapper, and `AppSettings.effectiveLocale` — drives the DE/EN UI (see *Localization* below) |
| [FeedbackView.swift](../apps/torrowhisper-macos/Sources/TorroWhisper/FeedbackView.swift) | Feedback dialog surfaced from the Help tab |

## FFI contract

All functions in [TorroWhisperBridgeFFI.h](../apps/torrowhisper-macos/Bridge/TorroWhisperBridgeFFI.h) follow the same shape:

```c
char *ow_do_something(const char *input_json);   // or no args
void  ow_string_free(char *raw);
```

- **Input**: a UTF-8 `char *` to JSON, or `NULL` if the function takes no args.
- **Output**: a newly allocated UTF-8 `char *` to JSON. Ownership transfers to the caller.
- **Freeing**: the caller **must** call `ow_string_free` with the returned pointer. Don't use `free(3)` — the allocator may differ.
- **Errors**: errors are encoded inside the JSON response (typically a top-level `error` or `status` field), not via a separate error channel.

### Thread-safety

The `BridgeRuntime` lives in a `thread_local! RefCell` on the bridge side. All FFI calls from Swift must happen from the **same thread** (today: the Swift main thread / AppKit event loop). If you need concurrent access from Swift, serialize the calls on a single queue.

## Runtime state and persistence

| Location | Content |
| --- | --- |
| `~/Library/Application Support/torrowhisper/settings.json` | `AppSettings` — hotkey, input device, Whisper / LLM presets, Modes, post-processing backend, startup behavior, waveform, VAD |
| FluidAudio cache under Application Support | Downloaded and Core-ML-compiled Parakeet assets on Apple Silicon |
| `~/Library/Application Support/torrowhisper/models/` | Optional Whisper `.bin` files **and** local LLM `.gguf` files (Gemma 4 presets plus any user-added custom models) |
| `~/Library/LaunchAgents/torrowhisper.plist` | *Dev fallback* — written by the `auto-launch` crate when the app runs outside a `.app` bundle |
| macOS Login Items database (`sfltool dumpbtm`) | *Production* — registered via `SMAppService` from the Swift side when the app runs from a signed bundle |

The bridge is the source of truth for settings; the Swift UI reads via `ow_load_settings` and writes via `ow_save_settings`. The UI does not keep its own copy.

## Autostart: two paths, one user-facing switch

macOS offers two autostart mechanisms, and TorroWhisper uses both depending on how it was launched:

1. **`SMAppService.mainApp`** (production, macOS 13+) — used when the executable lives inside `TorroWhisper.app/Contents/MacOS/`. The Swift layer calls `register()`/`unregister()` directly; the user-facing switch in Settings routes to this path.
2. **`LaunchAgent` plist** (dev fallback) — used when running via `swift run` during development. The Rust `auto-launch` crate writes `~/Library/LaunchAgents/torrowhisper.plist` pointing at the raw executable.

The Swift UI picks the mechanism based on the `autostart_mechanism` field returned by the bridge. Users never see the distinction — they just see *Launch at login* in Settings.

## Post-processing pipeline

After a raw transcript is produced, `post_processing::process_text` runs the **active** `ProcessingMode`'s prompt against a language model. Selection works in two layers:

1. **Which Mode?** Exactly one Mode is active at any time; users switch Modes from the menu bar or the Modes tab. Modes are pure prompt templates — removing a Mode does not remove its associated LLM.
2. **Which backend?** A Mode may carry an optional `post_processing_choice`. If set, that choice wins. Otherwise the global post-processing backend from settings is used. The global backend can itself be *off*, in which case the raw transcript is returned unchanged.

`PostProcessingChoice` is a tagged enum that maps 1:1 to a backend:

| Variant | Backend |
| --- | --- |
| `LocalPreset { preset }` | `local_llm` loads the matching Gemma 4 GGUF via `llama-cpp-2` (Metal-accelerated) and runs inference in-process. The model stays resident and auto-unloads after the `local_llm_auto_unload_secs` idle window. |
| `LocalCustom { id }` | Same code path as `LocalPreset`, but resolves to a user-provided GGUF file registered in `CustomLlmModel`. |
| `Ollama { model_name }` | HTTP POST to the configured Ollama endpoint (default `http://127.0.0.1:11434/api/chat`). |
| `LmStudio { model_name }` | HTTP POST to the configured LM Studio endpoint (default `http://127.0.0.1:1234/v1/chat/completions`, OpenAI-compatible). |

All backends respect a **45-second timeout** and a shared `Arc<AtomicBool>` cancellation flag, so the UI can abort post-processing if the user cancels dictation mid-run.

## Auto-updates (Sparkle)

TorroWhisper embeds [Sparkle 2.x](https://sparkle-project.org) as a Swift Package Manager dependency. The framework is copied into `TorroWhisper.app/Contents/Frameworks/` during `scripts/build-macos-app.sh`.

- `UpdaterController.swift` is a thin wrapper around `SPUStandardUpdaterController`. It refuses to start when Sparkle would otherwise show *"Unable to check for updates"* — specifically when the process is not running from a `.app` bundle (dev runs) or when `SUFeedURL` is missing from the `Info.plist`.
- `UpdatesSettingsView.swift` drives the Updates tab: a *Check Now* button, an *Automatically check for updates* toggle, and a read-out of the current feed URL and version. A *Check for Updates…* entry is also available from the menu bar.
- Relevant `Info.plist` keys:
  - `SUFeedURL` → `https://mahype.github.io/TorroWhisper/appcast.xml`
  - `SUPublicEDKey` → the Ed25519 public key bundled with the app
  - `SUEnableAutomaticChecks` → `true`
  - `SUScheduledCheckInterval` → `86400` (24 h)
- The appcast is hosted on the repository's `gh-pages` branch. Release publishing and signing live in [scripts/update-appcast.sh](../scripts/update-appcast.sh); see [RELEASING.md](RELEASING.md#sparkle-auto-updates) for the maintainer workflow.

## Localization

The app ships with **English (source)** and **German** translations. The Rust bridge emits English-only status and error strings; the Swift layer owns all user-visible translation.

- String catalogs live next to the Swift sources: `Resources/Localizable.xcstrings` is the source of truth; the `en.lproj` and `de.lproj` `Localizable.strings` files are generated siblings. `InfoPlist.strings` per-locale handles the permission prompt copy.
- `Localization.swift` exposes `L(_:locale:)` for imperative lookups and `LocalizedRoot` for setting the SwiftUI `\.locale` environment on every hosting root (Settings, Onboarding, Feedback, the recording indicator panel).
- Enum `label(locale:)` functions on core types (`StartupBehavior`, `TriggerMode`, `WaveformStyle`, `WaveformColor`, `ModelPreset`, `LlmPreset`, `PostProcessingBackend`, `DiagnosticStatus`, `PostProcessingChoice`, …) produce localized display strings on demand — no pre-computed labels live in state.
- Language selection: `AppSettings.ui_language` is a `UiLanguage` enum (`System` / `En` / `De`). `TorroWhisperApp.init` writes `AppleLanguages` into `UserDefaults` on launch so the bundle resolves the right `.lproj`. Changing the picker requires an app restart; system-language detection at launch is transparent.

## Cross-platform outlook

The bridge already has `#[cfg(target_os = …)]` blocks for Linux and Windows in `autostart.rs` and `permission_diagnostics.rs`. What's missing is a UI shell for each. The same `libtorrowhisper_bridge` static library and the same FFI contract will back those shells — only the UI layer changes.
