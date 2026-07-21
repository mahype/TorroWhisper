# TorroWhisper

**Dictate anywhere on your Mac — 100% local.**

Press a hotkey, speak, and your words land in whatever app has focus: mail, chat, your editor, the browser. On Apple Silicon, transcription uses NVIDIA Parakeet TDT v3 through Core ML and the Apple Neural Engine; Intel Macs use [whisper.cpp](https://github.com/ggerganov/whisper.cpp). Nothing leaves your Mac unless you deliberately configure a remote provider.

> **Status:** macOS 14+ is stable. Windows and Linux UI shells are on the roadmap — the Rust core and bridge already compile cross-platform.

---

## How it works

1. **Press** your global hotkey (push-to-talk or toggle).
2. **Speak** — TorroWhisper records from your chosen mic.
3. **Clean up** — an optional on-device language-model pass (Apple Foundation Models by default, with downloadable Gemma alternatives) fixes punctuation, capitalization, and recognition errors according to the active Mode's prompt.
4. **Done** — the result is pasted into the focused app, with a clipboard fallback if paste is blocked.

TorroWhisper lives in your menu bar. No Dock icon, no window clutter.

---

## Install (Users)

**Requires macOS 14+ on Apple Silicon or Intel.**

1. Download the [latest DMG](https://github.com/mahype/TorroWhisper/releases/latest).
2. Drag **TorroWhisper.app** into **Applications** and launch it.
3. Follow the onboarding — TorroWhisper downloads and prepares the compatible transcription model automatically.

Need permissions help, autostart setup, or uninstall steps? → [docs/INSTALL.md](docs/INSTALL.md)

| Platform | Status |
| --- | --- |
| macOS 14+ (Apple Silicon & Intel) | Stable — [download](https://github.com/mahype/TorroWhisper/releases/latest) |
| Windows | Planned |
| Linux | Planned |

---

## Features

### Dictation

- **Fully local transcription** with NVIDIA Parakeet TDT v3 on Apple Silicon and [whisper.cpp](https://github.com/ggerganov/whisper.cpp) on Intel — your voice never leaves the machine.
- **Global hotkey** with push-to-talk or toggle mode, plus a built-in recorder that warns about risky single-key bindings.
- **Menu-bar-only** UI — no Dock icon, no window clutter.
- **Guided zero-choice model setup**: the required transcription model is downloaded automatically; alternative models stay in Settings.
- **Autostart at login** via native macOS Login Items. The registered launch path is refreshed automatically on each start, so moving the app (e.g., after a reinstall into `/Applications`) doesn't break Launch-at-Login.

### Transcription models

- **NVIDIA Parakeet TDT v3 (~600 MB)** is the Apple-Silicon default, running through FluidAudio, Core ML, and the Apple Neural Engine.
- Seven optional Whisper presets range from **Tiny (78 MB)** to **Large v3 (3.1 GB)**, including **Large v3 Turbo** and a quantized **Large v3 Turbo Q5_0**.
- Built-in **Language Models** sheet to download, list, and delete models on demand.
- Per-session language override or fully automatic language detection.

### Post-processing with Modes

- **Modes** are prompt templates applied to the raw transcript. Create, edit, and delete them in-app; a default *Cleanup* Mode ships out of the box.
- **System model by default**: Apple Foundation Models provides on-device post-processing on supported Apple-Silicon Macs with Apple Intelligence; macOS manages it, so TorroWhisper does not present a separate download.
- Optional quantized **Gemma 4** models (Small / Medium / Large) run on-device via [llama-cpp-2](https://crates.io/crates/llama-cpp-2) with Metal acceleration and are downloaded only from Settings.
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
git clone git@github.com:mahype/TorroWhisper.git
cd torrowhisper
./scripts/dev.sh
```

`dev.sh` is the fast inner loop: it builds the Rust bridge (`cargo build -p torrowhisper-bridge`) and launches the Swift app via SwiftPM. No bundle, no signing — ideal for iterating.

### Build a real `.app` bundle

```bash
./scripts/build-macos-app.sh
open "dist/TorroWhisper.app"
```

Universal (Apple Silicon + Intel), release build, ad-hoc signed — good for running on your own Mac. For signed + notarized releases, see [docs/RELEASING.md](docs/RELEASING.md).

Full toolchain, debugging tips, and project walk-through: → [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)

---

## Project layout

```
torrowhisper/
├── apps/torrowhisper-macos/       # SwiftUI + AppKit menu bar app
├── crates/
│   ├── torrowhisper-bridge/       # JSON-over-FFI static library (staticlib + rlib)
│   └── torrowhisper-core/         # Shared Rust domain types (settings, presets, DTOs)
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

Copyright (C) 2026 Sven Wagener.

TorroWhisper is free software: you can redistribute it and/or modify it under
the terms of the **GNU General Public License** as published by the Free
Software Foundation, either version 3 of the License, or (at your option) any
later version. See [LICENSE](LICENSE) for the full text.

It is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
PURPOSE. The MP3 export uses LAME via `mp3lame-encoder` (LGPL-3.0). Parakeet
TDT v3 is an NVIDIA model distributed under CC BY 4.0 and is downloaded at
runtime; the Core ML integration uses FluidAudio (Apache-2.0). Other software
dependencies are permissively licensed and GPL-compatible.
