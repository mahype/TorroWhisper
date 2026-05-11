# Open Whisper

**Dictate anywhere on your Mac — 100% local.**

Press a hotkey, speak, and your words land in whatever app has focus: mail, chat, your editor, the browser. Transcription runs on your machine with [whisper.cpp](https://github.com/ggerganov/whisper.cpp). Nothing leaves your Mac unless you deliberately configure a remote provider.

> **Status:** macOS 14+ is stable. Windows and Linux UI shells are on the roadmap — the Rust core and bridge already compile cross-platform.

---

## How it works

1. **Press** your global hotkey (push-to-talk or toggle).
2. **Speak** — Open Whisper records from your chosen mic.
3. **Clean up** — an optional local LLM pass (Gemma 4 via llama.cpp) fixes punctuation, capitalization, and recognition errors according to the active Mode's prompt.
4. **Done** — the result is pasted into the focused app, with a clipboard fallback if paste is blocked.

Open Whisper lives in your menu bar. No Dock icon, no window clutter.

---

## Install (Users)

**Requires macOS 14+ on Apple Silicon or Intel.**

1. Download the [latest DMG](https://github.com/mahype/open-whisper/releases/latest).
2. Drag **Open Whisper.app** into **Applications** and launch it.
3. Follow the onboarding — mic, model download, hotkey, autostart.

Need permissions help, autostart setup, or uninstall steps? → [docs/INSTALL.md](docs/INSTALL.md)

| Platform | Status |
| --- | --- |
| macOS 14+ (Apple Silicon & Intel) | Stable — [download](https://github.com/mahype/open-whisper/releases/latest) |
| Windows | Planned |
| Linux | Planned |

---

## Features

### Dictation

- **Fully local transcription** with [whisper.cpp](https://github.com/ggerganov/whisper.cpp) — your voice never leaves the machine.
- **Global hotkey** with push-to-talk or toggle mode, plus a built-in recorder that warns about risky single-key bindings.
- **Menu-bar-only** UI — no Dock icon, no window clutter.
- **Guided onboarding** for mic, models, hotkey, and autostart.
- **Autostart at login** via native macOS Login Items. The registered launch path is refreshed automatically on each start, so moving the app (e.g., after a reinstall into `/Applications`) doesn't break Launch-at-Login.

### Transcription models

- Seven Whisper presets ranging from **Tiny (78 MB)** to **Large v3 (3.1 GB)**, including **Large v3 Turbo** and a quantized **Large v3 Turbo Q5_0** for Large-class quality on modest hardware.
- Built-in **Language Models** sheet to download, list, and delete models on demand.
- Per-session language override or fully automatic language detection.

### Post-processing with Modes

- **Modes** are prompt templates applied to the raw transcript. Create, edit, and delete them in-app; a default *Cleanup* Mode ships out of the box.
- **Local LLM backend by default**: quantized **Gemma 4** (Small / Medium / Large) running on-device via [llama-cpp-2](https://crates.io/crates/llama-cpp-2) with Metal acceleration. Models are downloaded and managed alongside your Whisper models; unused models auto-unload after a configurable idle timeout.
- **Custom GGUF models** — bring your own model from a local path or a download URL.
- **Remote providers** — optional Ollama or LM Studio endpoints; per-Mode override lets a single Mode use a different backend than the global default.

### Recording UX

- Live **Waveform indicator** in three styles (centered bars, line, envelope) and eight colors. Separate visual phases for recording, transcribing, post-processing, and "model not ready".
- **Voice-activity-based silence-stop** (VAD) with configurable threshold and silence duration.
- **Automatic paste** into the focused app via simulated keystroke, with a **clipboard fallback** if the app blocks synthetic input.
- **Automatic microphone fallback** — keeps a history of mics you've actively picked and switches to the next-best one when the current device disconnects, even mid-recording. Reconnects automatically when your preferred mic comes back. Optional toast notification can be disabled in Settings.

### System integration

- **Auto-updates** via [Sparkle](https://sparkle-project.org). The Updates tab lets users run a manual *Check Now* or disable background checks. Updates are cryptographically signed with an Ed25519 key.
- **Diagnostics** tab for microphone, accessibility, and input-monitoring permissions, with one-click access to System Settings.
- **Help** tab shows the running app version and bundle identifier and lets users re-run onboarding.
- **English and German UI**, picked automatically from your macOS system language; overridable in Settings → *Start & behavior*.

### Privacy

- Everything runs **locally by default** — transcription, post-processing, and settings all stay on-device. Remote providers are strictly opt-in.

---

## Run it locally (Developers)

Prereqs: **Rust 1.88+**, **Swift 6 / Xcode 16+**, **Xcode Command Line Tools**, and **CMake** (`brew install cmake`).

```bash
git clone git@github.com:mahype/open-whisper.git
cd open-whisper
./scripts/dev.sh
```

`dev.sh` is the fast inner loop: it builds the Rust bridge (`cargo build -p open-whisper-bridge`) and launches the Swift app via SwiftPM. No bundle, no signing — ideal for iterating.

### Build a real `.app` bundle

```bash
./scripts/build-macos-app.sh
open "dist/Open Whisper.app"
```

Universal (Apple Silicon + Intel), release build, ad-hoc signed — good for running on your own Mac. For signed + notarized releases, see [docs/RELEASING.md](docs/RELEASING.md).

Full toolchain, debugging tips, and project walk-through: → [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)

---

## Project layout

```
open-whisper/
├── apps/open-whisper-macos/       # SwiftUI + AppKit menu bar app
├── crates/
│   ├── open-whisper-bridge/       # JSON-over-FFI static library (staticlib + rlib)
│   └── open-whisper-core/         # Shared Rust domain types (settings, presets, DTOs)
├── scripts/                       # Dev, build, sign, DMG packaging
└── docs/                          # Long-form documentation
```

How the Rust core, FFI bridge, and Swift UI fit together: → [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)

---

## Documentation

| Doc | What's inside |
| --- | --- |
| [INSTALL.md](docs/INSTALL.md) | Install, permissions, autostart, uninstall |
| [DEVELOPMENT.md](docs/DEVELOPMENT.md) | Dev setup, build scripts, debugging |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Rust core ↔ FFI bridge ↔ Swift UI |
| [RELEASING.md](docs/RELEASING.md) | Tagging, signing, notarization, publishing, Sparkle |
| [CI.md](docs/CI.md) | GitHub Actions workflows, SwiftLint, CodeQL, cargo-deny |
| [CHANGELOG.md](CHANGELOG.md) | Release-by-release summary of changes |

---

## Roadmap

- [ ] Native UI shells for Windows and Linux on top of the existing Rust bridge
- [ ] Optional cloud transcription providers

Larger feature ideas under discussion — chat / voice-assistant mode, custom dictionary, adaptive learning from manual corrections, auto-correct toggle — are tracked in [ROADMAP.md](ROADMAP.md).

---

## License

MIT — see [LICENSE](LICENSE).
