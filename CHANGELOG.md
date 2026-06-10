# Changelog

All notable changes to Open Whisper are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] — 2026-06-10

### Added
- **Save dictations to disk** — a new *Save to disk* section under Settings → History optionally writes each completed dictation to a folder you choose: the recording as an MP3 and/or the transcript as a `.txt`, with matching timestamped names. Cancelled dictations are not saved ([#7](https://github.com/mahype/open-whisper/issues/7)).
- **Accessible recording bubble** — two independent Settings toggles: a *Large view* (~1.7×) for low-vision users and a *High contrast* mode (bolder text, stronger colors). Both default off and combine freely ([#8](https://github.com/mahype/open-whisper/issues/8)).
- **Onboarding permissions step** — a dedicated step requests microphone and accessibility access up front and confirms once granted, instead of surfacing the prompt only after the first dictation.
- **Permission checks in Diagnostics** — microphone and accessibility authorization now appear as OK/error entries with a hint pointing at the right System Settings pane.
- **File logging, panic hook and structured dictation errors**, with quick access to the log from the Help section, plus an explicit error state in the recording bubble.

### Changed
- **Onboarding no longer auto-downloads models** — the transcription model is downloaded on demand and required to continue; post-processing is optional with a pointer to the language-models manager.
- **Launch at login is a simple yes/no toggle** (in Settings and onboarding) instead of the three-way "ask on first launch" picker.
- **Recording bubble overhaul** — a small stop button replaces the red dot, shortcut hints ("Stop: ⌥⇧S · Cancel: Esc") sit under the model name, a "being cancelled" state is shown, and the layout stays steady across recording/transcribing/post-processing/error so the box no longer jumps. The bubble now reliably appears on the active monitor in multi-display setups ([#9](https://github.com/mahype/open-whisper/issues/9)).
- **Fewer Whisper hallucinations** — non-speech token suppression cuts the spurious "Vielen Dank" / "Untertitel von …" filler Whisper emits on trailing silence.

### Fixed
- **Escape no longer loses a dictation** — cancelling while still recording now transcribes the captured audio and keeps it in history (marked cancelled) instead of discarding it.
- **System language is detected correctly** — with the UI language set to *System*, a German system now shows German.
- **German localization repaired** — a stray ASCII quote had broken the entire German strings table, falling the whole UI back to English; the build now lints every strings file and fails on a syntax error.
- **Released builds reliably pick up Rust changes** — the app binary is now force-relinked against the freshly built Rust library.

## [0.3.3] — 2026-06-09

### Changed
- **Onboarding no longer auto-downloads language models.** Both models on the model step now have an explicit Download button and nothing downloads on its own. The transcription (Whisper) model is required — *Next* stays disabled until the selected model is downloaded, so a speech model must be fetched before continuing, and switching the preset re-arms the requirement. The post-processing (LLM) model is optional and never blocks the wizard; a footer explains what post-processing does (cleans up the transcript — punctuation, capitalization, filler-word removal) and that a model is only needed if you want it and can be added later in Settings ([`68d1954`](https://github.com/mahype/open-whisper/commit/68d1954)).

## [0.3.2] — 2026-06-09

### Fixed
- **Released `.app` now launches on machines other than the build host.** Two release-only bugs left the app crashing at its first localized-string lookup — no menu bar icon, no onboarding wizard (the microphone prompt still appeared, fired earlier from the Rust bridge). First, declaring the localizations as SwiftPM `resources:` synthesized a `Bundle.module` accessor that only resolved the `.app` root (which codesign forbids content in) and the absolute build-machine path (absent on users' machines); the 0.3.1 `Contents/Resources` copy only ever worked because that build path still existed on the build host. The localizations now ship in `Contents/Resources/<lang>.lproj` and resolve through `Bundle.main`. Second, the universal-build guard matched the literal `Xcode.app`, which fails against the CI runner's versioned `Xcode_16.2.app` path, so every release was silently built arm64-only and could not launch on Intel Macs; the build now detects full Xcode by path suffix and hard-fails if a requested universal binary is not fat ([`920321b`](https://github.com/mahype/open-whisper/commit/920321b)).

## [0.3.1] — 2026-06-09

### Added
- **Microphone and accessibility permission controls in Settings** — a new section shows the current authorization status for both permissions and offers one-click actions to fix them: requesting microphone access (or deep-linking to the Microphone privacy pane when denied), triggering the native Accessibility prompt, and a *Reset accessibility permission* action that runs `tccutil reset Accessibility` to clear a stale TCC entry and reopens the pane so the app can be re-added cleanly ([`1b91220`](https://github.com/mahype/open-whisper/commit/1b91220)).
- **VoiceOver announcements and accessible controls** — dictation state changes are announced to VoiceOver, and tray/settings controls expose proper accessibility labels ([`969f617`](https://github.com/mahype/open-whisper/commit/969f617)).

### Changed
- **Menu bar icon is now a megaphone** (`megaphone` when idle, `megaphone.fill` while recording) instead of the waveform/mic glyphs, keeping the empty-to-filled transition as the recording cue ([`f518264`](https://github.com/mahype/open-whisper/commit/f518264)).
- **Onboarding blocks until both models finish downloading** — the whisper and llm models start downloading as soon as the model step is shown, and *Next* stays disabled until both report downloaded, instead of starting the download on click and letting the user advance immediately ([`be4dc87`](https://github.com/mahype/open-whisper/commit/be4dc87)).

### Fixed
- **Release `.app` bundles the SwiftPM resource bundle** so `Bundle.module` resolves at runtime; without it the app crashed on launch (missing localized strings) the moment the menu bar state refreshed ([`68dcffd`](https://github.com/mahype/open-whisper/commit/68dcffd)).

## [0.3.0] — 2026-06-04

### Added
- **Dictation history** — every finished transcript is recorded in `history.json` next to the settings, with timestamp, mode, and a `was_cancelled` flag. Settings gains a *History* tab with an enable toggle, a configurable cap (10–1000, default 100), per-entry copy and delete buttons, and a *Clear all* action with confirmation. The tray menu gains a *Recent dictations* submenu showing the five newest entries (40-char preview, ⚠︎ marker on cancelled ones); clicking copies the full text to the clipboard without auto-pasting. Pressing Escape during dictation no longer drops the in-flight Whisper transcription — it lands in history (cancelled = true) and is simply not inserted, so accidental Escapes are recoverable ([`9ef9aff`](https://github.com/mahype/open-whisper/commit/9ef9aff)).
- **User-defined dictionary** — global word replacements applied to the raw transcript before any post-processing, with per-entry case-sensitive and whole-word toggles. Modes can opt out individually so a mode that needs the raw transcript stays untouched. A new *Dictionary* tab manages entries ([`de7515c`](https://github.com/mahype/open-whisper/commit/de7515c)).
- **Hotkey support for F13–F20, the numeric keypad, and media keys**, plus automatic re-registration when a keyboard is plugged in or out so the global hotkey survives device hotplugs ([`3ab9159`](https://github.com/mahype/open-whisper/commit/3ab9159)).
- **Automatic microphone fallback on hotplug** — Open Whisper keeps a history of input devices you've actively picked. If the current mic disconnects (even mid-recording) the app seamlessly switches to the next-best mic from the history, falling back to the system default; it switches back automatically when the preferred mic returns. A short toast surfaces the change and can be turned off in Settings ([`655fdba`](https://github.com/mahype/open-whisper/commit/655fdba)).
- **English and German UI** with automatic selection based on the macOS system language. Source language is English; a full German translation ships alongside. A new *UI language* picker lives in Settings → Start & behavior (System / English / Deutsch; requires app restart) ([`e2579a4`](https://github.com/mahype/open-whisper/commit/e2579a4)).
- **Microphone switcher submenu** in the tray menu for quick switching without opening Settings ([`7b4f824`](https://github.com/mahype/open-whisper/commit/7b4f824)).
- **Tray menu shows the active recording hotkey** next to the *Start/Stop dictation* entry so the shortcut is always visible ([`36b7e5e`](https://github.com/mahype/open-whisper/commit/36b7e5e)).

### Changed
- Post-processing is now switched on and off via an "Off" entry at the top of the Modes list instead of a separate toggle ([`b1a1f40`](https://github.com/mahype/open-whisper/commit/b1a1f40)).
- F-key hotkey warning is now condensed to a single line under the hotkey field, with the full macOS keyboard-settings explanation moved into a hover tooltip so it no longer gets truncated inside the Settings form ([`a515142`](https://github.com/mahype/open-whisper/commit/a515142)).
- Dictionary settings (section header, *Add entry* button, footer hint) now resolve correctly to German, and the case-sensitive / whole-word toggles use the same localized `.help()` pattern as the rest of the codebase so their tooltips render reliably ([`f53223a`](https://github.com/mahype/open-whisper/commit/f53223a)).

### Fixed
- Escape is now consumed system-wide while the dictation indicator is visible, so it cancels the dictation cleanly without leaking into the underlying app ([`59800b4`](https://github.com/mahype/open-whisper/commit/59800b4)).
- Autostart: the registered `SMAppService` program path is refreshed on launch so Launch-at-Login keeps working after the app is moved or reinstalled into a different folder ([`c1d56d6`](https://github.com/mahype/open-whisper/commit/c1d56d6)).

### CI
- Release workflow publishes a GitHub Release directly instead of creating a draft ([`e1d5966`](https://github.com/mahype/open-whisper/commit/e1d5966)).
- Comprehensive verification pipeline added (Rust fmt/clippy, cargo-deny, CodeQL, SwiftLint) with CI documentation in [`docs/CI.md`](docs/CI.md) ([`4f43485`](https://github.com/mahype/open-whisper/commit/4f43485), [`543030e`](https://github.com/mahype/open-whisper/commit/543030e)).

## [0.2.1] — 2026-04-19

### Changed
- Mode editor refactored with post-processing summaries and a polished sidebar layout ([`2367c99`](https://github.com/mahype/open-whisper/commit/2367c99)).

### Added
- Help tab now shows the running app version and bundle identifier ([`ed5df92`](https://github.com/mahype/open-whisper/commit/ed5df92)).

## [0.2.0] — 2026-04-19

First public release. Everything below has landed since the project was initialised.

### Added — Auto-updates (Sparkle)
- Sparkle 2.x integrated via SwiftPM and embedded in the `.app` bundle ([`0508d38`](https://github.com/mahype/open-whisper/commit/0508d38), [`da23377`](https://github.com/mahype/open-whisper/commit/da23377)).
- `UpdaterController` wrapping `SPUStandardUpdaterController` with safety checks for non-bundle dev runs ([`9267358`](https://github.com/mahype/open-whisper/commit/9267358)).
- *Check for Updates…* menu-bar entry ([`17cf385`](https://github.com/mahype/open-whisper/commit/17cf385)) and a dedicated Updates tab in Settings ([`fd5f403`](https://github.com/mahype/open-whisper/commit/fd5f403)).
- Sparkle feed URL and Ed25519 public key embedded in `Info.plist` ([`c94e6da`](https://github.com/mahype/open-whisper/commit/c94e6da)).
- Release workflow appends a signed appcast entry to `gh-pages` on every tag ([`13fb407`](https://github.com/mahype/open-whisper/commit/13fb407), [`f0edc4d`](https://github.com/mahype/open-whisper/commit/f0edc4d)).

### Added — Post-processing
- Prompt-template Modes: create, rename, and delete post-processing Modes; a default *Cleanup* Mode ships out of the box ([`c0352bc`](https://github.com/mahype/open-whisper/commit/c0352bc)).
- Local LLM post-processing via `llama-cpp-2` with Gemma 4 Small/Medium/Large presets ([`0a24b32`](https://github.com/mahype/open-whisper/commit/0a24b32), [`7aee99f`](https://github.com/mahype/open-whisper/commit/7aee99f)).
- Custom GGUF models: import from a local file ([`4d7c4ad`](https://github.com/mahype/open-whisper/commit/4d7c4ad)) or a download URL ([`60e4a80`](https://github.com/mahype/open-whisper/commit/60e4a80)).
- Ollama and LM Studio models surfaced in the post-processing backend picker ([`56374d2`](https://github.com/mahype/open-whisper/commit/56374d2)).
- Global post-processing backend replaces the old per-Mode override as the default; Modes can still opt into a different backend individually ([`477da53`](https://github.com/mahype/open-whisper/commit/477da53), [`c7ca0b0`](https://github.com/mahype/open-whisper/commit/c7ca0b0)).
- Unified Language Models manager sheet covering both Whisper and local LLM models ([`6fde65e`](https://github.com/mahype/open-whisper/commit/6fde65e)).
- Gemma preset labels show their on-disk size ([`0d9b117`](https://github.com/mahype/open-whisper/commit/0d9b117)).

### Added — Transcription
- Whisper preset catalog expanded with **Tiny** and the **Large v3** family (Large v3, Large v3 Turbo, Large v3 Turbo Q5_0) ([`5915c55`](https://github.com/mahype/open-whisper/commit/5915c55)).
- Onboarding merges model selection and download into a single step ([`26614d9`](https://github.com/mahype/open-whisper/commit/26614d9)).
- Missing transcription model is surfaced directly on the recording indicator ([`9d5f081`](https://github.com/mahype/open-whisper/commit/9d5f081)).

### Added — Recording UX
- Recording indicator redesigned with a blinking dot and the active model / Mode labels ([`2467ba2`](https://github.com/mahype/open-whisper/commit/2467ba2), [`790133a`](https://github.com/mahype/open-whisper/commit/790133a)).
- Waveform style options (centered bars, line, envelope) and a color picker ([`78806e4`](https://github.com/mahype/open-whisper/commit/78806e4), [`2590969`](https://github.com/mahype/open-whisper/commit/2590969)).
- Top-center recording overlay with a distinct transcription phase ([`94f91bd`](https://github.com/mahype/open-whisper/commit/94f91bd)); post-processing phase made clearly visible ([`7bbb30a`](https://github.com/mahype/open-whisper/commit/7bbb30a)).
- Dictation cancellation, downloaded-model picker, and tray model switcher ([`22ebdfd`](https://github.com/mahype/open-whisper/commit/22ebdfd)).

### Added — Core functionality
- Local audio capture and `whisper.cpp` transcription ([`62d5ab5`](https://github.com/mahype/open-whisper/commit/62d5ab5)).
- Tray icon and global hotkey integration, including single-key hotkeys with a safety warning ([`21edc42`](https://github.com/mahype/open-whisper/commit/21edc42), [`2f1030c`](https://github.com/mahype/open-whisper/commit/2f1030c)).
- Native macOS menu-bar app with System-Settings-style UI ([`f2f6c6f`](https://github.com/mahype/open-whisper/commit/f2f6c6f), [`205fed5`](https://github.com/mahype/open-whisper/commit/205fed5)).
- Active-app text insertion via simulated paste ([`9db4ffc`](https://github.com/mahype/open-whisper/commit/9db4ffc)); clipboard fallback when paste is blocked ([`4b7d131`](https://github.com/mahype/open-whisper/commit/4b7d131)).
- Onboarding flow and permission diagnostics ([`3710095`](https://github.com/mahype/open-whisper/commit/3710095)); Help section to relaunch onboarding ([`9c950f7`](https://github.com/mahype/open-whisper/commit/9c950f7)).
- Model downloads and autostart support ([`cd560a5`](https://github.com/mahype/open-whisper/commit/cd560a5)).
- Auto-save settings and initial recording indicator ([`4e7f145`](https://github.com/mahype/open-whisper/commit/4e7f145)).
- Hotkey recorder UI ([`c272357`](https://github.com/mahype/open-whisper/commit/c272357)).

### Fixed
- `LocalLlm` now applies Mode prompts to the transcript instead of echoing them back ([`876c6fa`](https://github.com/mahype/open-whisper/commit/876c6fa)).
- Settings window `styleMask` is clamped so SwiftUI cannot re-enable `fullSizeContentView` ([`d456c06`](https://github.com/mahype/open-whisper/commit/d456c06), [`fd4b4a9`](https://github.com/mahype/open-whisper/commit/fd4b4a9)).
- Tray menu cleaned up by removing redundant status entries ([`329440c`](https://github.com/mahype/open-whisper/commit/329440c)).
- Hard-check `sign_update` and separate Quit entry in the tray menu ([`d632990`](https://github.com/mahype/open-whisper/commit/d632990)).

### CI & infrastructure
- GitHub Actions CI and release workflows plus MIT LICENSE ([`4bda7cf`](https://github.com/mahype/open-whisper/commit/4bda7cf)).
- macOS packaging scripts and app icon ([`056d39a`](https://github.com/mahype/open-whisper/commit/056d39a)).
- CI runner bumped to `macos-15` for a newer Metal.framework ([`47caf7d`](https://github.com/mahype/open-whisper/commit/47caf7d)); Xcode 16 pinned on `macos-14` for Swift 6 ([`a1a2b63`](https://github.com/mahype/open-whisper/commit/a1a2b63)).
- Legacy egui desktop app removed ([`82a3f6d`](https://github.com/mahype/open-whisper/commit/82a3f6d)).

[Unreleased]: https://github.com/mahype/open-whisper/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/mahype/open-whisper/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/mahype/open-whisper/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/mahype/open-whisper/releases/tag/v0.2.0
