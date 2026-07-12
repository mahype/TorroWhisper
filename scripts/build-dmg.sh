#!/usr/bin/env bash
# Packages dist/TorroWhisper.app into a distribution DMG with a drag-to-Applications
# window layout. If signing credentials are set, the DMG itself is signed, notarized,
# and stapled so the downloaded container passes Gatekeeper cleanly.
#
# Requires: create-dmg (`brew install create-dmg`).
#
# Environment:
#   VERSION                                Overrides derived version (same rules as build-macos-app.sh).
#   MACOS_SIGN_IDENTITY (optional)         Triggers sign + notarize + staple of the DMG.
#   APPLE_ID, APPLE_TEAM_ID,               Required alongside MACOS_SIGN_IDENTITY.
#   APPLE_APP_SPECIFIC_PASSWORD

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

app="dist/TorroWhisper.app"

if [[ ! -d "$app" ]]; then
    echo "error: $app not found — run scripts/build-macos-app.sh first" >&2
    exit 1
fi

if ! command -v create-dmg >/dev/null 2>&1; then
    echo "error: create-dmg not found. Install with: brew install create-dmg" >&2
    exit 1
fi

# --- Version ------------------------------------------------------------------

if [[ -z "${VERSION:-}" ]]; then
    if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        VERSION="$(git describe --tags --always --dirty 2>/dev/null | sed 's/^v//')"
    fi
fi
if [[ -z "${VERSION:-}" ]]; then
    VERSION="$(awk -F'"' '/^version/ {print $2; exit}' Cargo.toml)"
fi
export VERSION

dmg="dist/TorroWhisper-$VERSION.dmg"
echo "==> Building $dmg"

# create-dmg refuses to overwrite an existing file.
rm -f "$dmg"

# Staging directory so create-dmg only picks up the .app, not adjacent build
# artifacts in dist/.
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
cp -R "$app" "$staging/"

create-dmg \
    --volname "TorroWhisper $VERSION" \
    --window-size 540 380 \
    --icon-size 128 \
    --icon "TorroWhisper.app" 140 190 \
    --app-drop-link 400 190 \
    --hide-extension "TorroWhisper.app" \
    --no-internet-enable \
    "$dmg" "$staging"

# --- Sign + notarize + staple the DMG itself ---------------------------------

if [[ -n "${MACOS_SIGN_IDENTITY:-}" ]]; then
    : "${APPLE_ID:?APPLE_ID must be set when MACOS_SIGN_IDENTITY is set}"
    : "${APPLE_TEAM_ID:?APPLE_TEAM_ID must be set when MACOS_SIGN_IDENTITY is set}"
    : "${APPLE_APP_SPECIFIC_PASSWORD:?APPLE_APP_SPECIFIC_PASSWORD must be set when MACOS_SIGN_IDENTITY is set}"

    echo "==> Signing DMG"
    codesign --force --sign "$MACOS_SIGN_IDENTITY" --timestamp "$dmg"

    echo "==> Notarizing DMG"
    xcrun notarytool submit "$dmg" \
        --apple-id "$APPLE_ID" \
        --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_SPECIFIC_PASSWORD" \
        --wait \
        --timeout 30m

    echo "==> Stapling DMG"
    xcrun stapler staple "$dmg"
    xcrun stapler validate "$dmg"
fi

# --- Checksum -----------------------------------------------------------------

( cd dist && shasum -a 256 "TorroWhisper-$VERSION.dmg" > SHA256SUMS.txt )
echo "==> SHA256: $(cat dist/SHA256SUMS.txt)"

echo "==> Done: $dmg"
