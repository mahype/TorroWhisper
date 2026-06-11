#!/usr/bin/env bash
# Signs a DMG with the Sparkle Ed25519 key and prepends a matching <item>
# to an existing appcast.xml.
#
# Positional arguments:
#   1  DMG path                       e.g. dist/OpenWhisper-0.1.0.dmg
#   2  Version (no `v` prefix)         e.g. 0.1.0
#   3  Release-notes URL               e.g. https://github.com/.../releases/tag/v0.1.0
#   4  Appcast path                    e.g. gh-pages/appcast.xml
#   5  Release-notes markdown file     optional; when given, the notes are
#                                      embedded as inline HTML <description>
#                                      instead of linking the GitHub page
#
# Required env:
#   SPARKLE_ED_PRIVATE_KEY             The Ed25519 private key content
# Optional env:
#   MIN_SYSTEM_VERSION                 Defaults to "14.0"
#   SIGN_UPDATE                        Override the sign_update binary path

set -euo pipefail

DMG_PATH="${1:?DMG path required}"
VERSION="${2:?version required}"
RELEASE_NOTES_URL="${3:?release notes URL required}"
APPCAST_PATH="${4:?appcast path required}"
RELEASE_NOTES_FILE="${5:-}"
MIN_SYSTEM_VERSION="${MIN_SYSTEM_VERSION:-14.0}"

RELEASE_NOTES_MD=""
if [[ -n "$RELEASE_NOTES_FILE" ]]; then
    if [[ ! -f "$RELEASE_NOTES_FILE" ]]; then
        echo "error: release notes file not found: $RELEASE_NOTES_FILE" >&2
        exit 1
    fi
    RELEASE_NOTES_MD="$(cat "$RELEASE_NOTES_FILE")"
    if [[ -z "$RELEASE_NOTES_MD" ]]; then
        echo "warning: $RELEASE_NOTES_FILE is empty — falling back to releaseNotesLink" >&2
    fi
fi

: "${SPARKLE_ED_PRIVATE_KEY:?SPARKLE_ED_PRIVATE_KEY must be set}"

if [[ ! -f "$DMG_PATH" ]]; then
    echo "error: DMG not found: $DMG_PATH" >&2
    exit 1
fi
if [[ ! -f "$APPCAST_PATH" ]]; then
    echo "error: appcast not found: $APPCAST_PATH" >&2
    exit 1
fi

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

if [[ -z "${SIGN_UPDATE:-}" ]]; then
    SIGN_UPDATE="$(find "$repo_root/apps/open-whisper-macos/.build" \
        -type f -name sign_update 2>/dev/null | head -1)"
fi
if [[ ! -x "${SIGN_UPDATE:-}" ]]; then
    echo "error: sign_update binary not found under $repo_root/apps/open-whisper-macos/.build. Run 'swift build --package-path apps/open-whisper-macos' first to materialise the Sparkle artifacts." >&2
    exit 1
fi

keyfile="$(mktemp)"
trap 'rm -f "$keyfile"' EXIT
printf '%s' "$SPARKLE_ED_PRIVATE_KEY" > "$keyfile"
chmod 600 "$keyfile"

sig_line="$("$SIGN_UPDATE" -f "$keyfile" "$DMG_PATH")"
#   sign_update prints: sparkle:edSignature="..." length="..."
ed_sig="$(echo "$sig_line" | sed -nE 's/.*sparkle:edSignature="([^"]+)".*/\1/p')"
length="$(echo "$sig_line" | sed -nE 's/.*length="([0-9]+)".*/\1/p')"

if [[ -z "$ed_sig" || -z "$length" ]]; then
    echo "error: could not parse sign_update output: $sig_line" >&2
    exit 1
fi

dmg_filename="$(basename "$DMG_PATH")"
dmg_url="https://github.com/mahype/open-whisper/releases/download/v${VERSION}/${dmg_filename}"
pub_date="$(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S +0000')"

set +e
APPCAST_PATH="$APPCAST_PATH" \
VERSION="$VERSION" \
RELEASE_NOTES_URL="$RELEASE_NOTES_URL" \
RELEASE_NOTES_MD="$RELEASE_NOTES_MD" \
DMG_URL="$dmg_url" \
DMG_LENGTH="$length" \
DMG_ED_SIGNATURE="$ed_sig" \
MIN_SYSTEM_VERSION="$MIN_SYSTEM_VERSION" \
PUB_DATE="$pub_date" \
python3 "$repo_root/scripts/_appcast_insert.py"
py_rc=$?
set -e

case "$py_rc" in
    0) echo "appcast updated: added version $VERSION" ;;
    2) echo "appcast unchanged: version $VERSION already present" ;;
    *) exit "$py_rc" ;;
esac
