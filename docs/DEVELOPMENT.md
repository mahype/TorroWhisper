# Development

## Prerequisites

You need a Mac running macOS 14+ with:

- **Xcode Command Line Tools** — `xcode-select --install`
- **Swift 6** — bundled with Xcode 16+. Verify with `swift --version`.
- **Rust toolchain** — install via [rustup.rs](https://rustup.rs/). Edition 2024 plus transitive deps (`whisper-rs-sys`, `llama-cpp-sys-2`, `image`) require stable **1.88+**.
- **CMake** — required to build the bundled `whisper.cpp` and `llama.cpp`: `brew install cmake`.
- For universal binaries (release builds): full Xcode (not just Command Line Tools) and both Rust targets installed:
  ```bash
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  ```

Optional but recommended:
- `create-dmg` (for local DMG packaging): `brew install create-dmg`

## Clone and run

```bash
git clone git@github.com:mahype/DonnyWhisper.git
cd donnywhisper
./scripts/dev.sh
```

`dev.sh` is the fast iteration loop:

1. `cargo build -p donnywhisper-bridge` — produces `target/debug/libdonnywhisper_bridge.a`
2. `swift run --package-path apps/donnywhisper-macos DonnyWhisper` — launches the app

The Swift package links against `target/debug/libdonnywhisper_bridge.a` via the hard-coded linker flag in [Package.swift](../apps/donnywhisper-macos/Package.swift). Release builds go through the build script below instead.

> **Heads-up:** when launched via `swift run`, the executable runs outside a `.app` bundle. The SMAppService-based autostart path is disabled in that mode — a LaunchAgent fallback is used. For realistic autostart testing, build the bundle (next section).

## Build a local `.app` bundle

```bash
./scripts/build-macos-app.sh
open dist/Open\ Whisper.app
```

This produces a **universal** (Apple Silicon + Intel), **release** build, and assembles a complete bundle at `dist/DonnyWhisper.app` with Info.plist, entitlements, and resources. The bundle is signed **ad-hoc** by default — good enough to run on your own machine, but Gatekeeper will flag it on other Macs. For a proper signed + notarized build, see [RELEASING.md](RELEASING.md).

## Project layout

```
apps/donnywhisper-macos/
├── Package.swift                       # SPM manifest
├── Bridge/DonnyWhisperBridgeFFI.h       # C header for the Rust FFI surface
├── Resources/                          # Info.plist, entitlements, AppIcon.icns
└── Sources/DonnyWhisper/             # Swift UI and AppDelegate

crates/
├── donnywhisper-bridge/                # staticlib + rlib; cdylib-free
│   └── src/
│       ├── lib.rs                      # FFI entry points, runtime wiring
│       ├── dictation.rs                # Audio capture + transcription loop
│       ├── model_manager.rs            # Whisper model download & bookkeeping
│       ├── autostart.rs                # auto-launch crate wrapper (Dev fallback)
│       ├── permission_diagnostics.rs   # Platform-specific permission probes
│       ├── post_processing.rs          # Ollama/LM Studio post-processing
│       ├── settings_store.rs           # settings.json read/write
│       └── text_inserter.rs            # Clipboard + simulated paste
└── donnywhisper-core/                  # Pure-Rust shared types (no I/O)
    └── src/lib.rs                      # AppSettings, presets, enums

scripts/
├── dev.sh                              # Fast dev loop (debug, swift run)
├── build-macos-app.sh                  # Build universal .app bundle
├── codesign-macos.sh                   # Sign + notarize (requires Apple creds)
└── build-dmg.sh                        # Package .app into a distribution DMG
```

## FFI header

The C header at [apps/donnywhisper-macos/Bridge/DonnyWhisperBridgeFFI.h](../apps/donnywhisper-macos/Bridge/DonnyWhisperBridgeFFI.h) is **hand-maintained** — we don't currently auto-generate it with cbindgen. When you add or change an `extern "C"` function in `crates/donnywhisper-bridge/src/lib.rs`, update the header by hand and keep the shape in sync.

Convention: every function takes UTF-8 JSON strings and returns a newly allocated UTF-8 JSON string owned by the caller. The caller must free it via `ow_string_free`.

## Debugging

### Rust logs

Set `RUST_LOG` before launching:

```bash
RUST_LOG=info ./scripts/dev.sh
RUST_LOG=donnywhisper_bridge=debug ./scripts/dev.sh
```

Logs go to stderr, which Xcode / Console.app / your terminal will display depending on how you launched.

### Swift side

Attach a debugger with Xcode:

1. Run `./scripts/dev.sh` to produce the build artifacts.
2. In Xcode, **Debug → Attach to Process by PID or Name…** → `DonnyWhisper`.

Or add breakpoints and launch from Xcode directly by opening the Swift package folder.

### Settings and model files

Runtime data is written to:

- `~/Library/Application Support/donnywhisper/settings.json`
- `~/Library/Application Support/donnywhisper/models/*.bin`

Deleting these resets the app to a fresh-install state — handy for testing onboarding.

## Running tests

```bash
cargo test --workspace
swift test --package-path apps/donnywhisper-macos
```

Swift tests require Xcode.app (not only CommandLineTools) because `XCTest` ships with the Xcode developer toolchain. CI switches to Xcode 16 via `maxim-lobanov/setup-xcode`.

## Style and CI

- Rust: `cargo fmt` + `cargo clippy --workspace -- -D warnings`
- Swift: [SwiftLint](https://github.com/realm/SwiftLint) + `swift format lint` (configs at [`.swiftlint.yml`](../.swiftlint.yml) and [`.swift-format`](../.swift-format)). Install locally: `brew install swiftlint`.
- Commits: keep the subject line imperative; reference any related issue.

Optional local pre-commit hook that mirrors the CI checks:

```bash
brew install lefthook
lefthook install
```

See [`lefthook.yml`](../lefthook.yml) for the hook config.

For a full description of every workflow, what each check does, when it runs, and how to respond to failures, see [**CI.md**](./CI.md).
