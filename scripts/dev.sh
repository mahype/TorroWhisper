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

swift build --package-path apps/torrowhisper-macos --product TorroWhisper
bin_dir="$(swift build --package-path apps/torrowhisper-macos --show-bin-path)"

# Stage the same Contents/Resources payload the packaged .app gets. Outside a
# .app, Bundle.main (== Bundle.module, see Bundle+module.swift) resolves against
# the executable's own directory, so the tables have to sit next to the binary.
#
# Without this the dev binary finds no de.lproj at all: the whole UI silently
# falls back to English while the Rust bridge, which reads the system locale
# itself, keeps sending German status text — that mixed-language window is a
# dev-loop artifact, not a UI bug.
res_dir="$repo_root/apps/torrowhisper-macos/Sources/TorroWhisper/Resources"
for lproj_dir in "$res_dir"/*.lproj; do
    [[ -f "$lproj_dir/Localizable.strings" ]] || continue
    mkdir -p "$bin_dir/$(basename "$lproj_dir")"
    cp "$lproj_dir/Localizable.strings" "$bin_dir/$(basename "$lproj_dir")/Localizable.strings"
done

# The brand display cut is git-ignored (licensed, see AGENTS.md); a checkout
# without it runs fine and falls back to the heavy system cut.
if [[ -f "$res_dir/FrutigerLT-UltraBlack.ttf" ]]; then
    cp "$res_dir/FrutigerLT-UltraBlack.ttf" "$bin_dir/FrutigerLT-UltraBlack.ttf"
fi

exec "$bin_dir/TorroWhisper"
