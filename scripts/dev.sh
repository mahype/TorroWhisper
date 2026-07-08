#!/usr/bin/env bash
# Fast dev loop: build the Rust bridge (debug), then launch the Swift app via SPM.
# For signed release builds or autostart testing, use scripts/build-macos-app.sh instead.

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

: "${RUST_LOG:=info}"
export RUST_LOG

cat <<'BANNER'
────────────────────────────────────────────────────────────────
 TorroWhisper — dev loop
 This launches outside a .app bundle. Autostart falls back to a
 LaunchAgent plist; SMAppService registration is unavailable.
 For realistic autostart testing, run ./scripts/build-macos-app.sh
────────────────────────────────────────────────────────────────
BANNER

cargo build -p torrowhisper-bridge -p torrowhisper-llm-helper

# The app looks for the LLM helper next to its own executable; in the SPM dev
# loop the helper lives in target/, so point the bridge at it explicitly.
export OW_LLM_HELPER="$repo_root/target/debug/torrowhisper-llm-helper"

swift run --package-path apps/torrowhisper-macos TorroWhisper
