#!/usr/bin/env bash
# Signs dist/DonnyWhisper.app with a Developer ID certificate, submits it to
# Apple's notary service, and staples the ticket.
#
# Required environment variables:
#   MACOS_SIGN_IDENTITY           e.g. "Developer ID Application: Sven Wagener (XXXXXXXXXX)"
#   APPLE_ID                      Apple ID tied to your developer account
#   APPLE_TEAM_ID                 10-char Team ID
#   APPLE_APP_SPECIFIC_PASSWORD   From appleid.apple.com → App-Specific Passwords
#
# Run `scripts/build-macos-app.sh` first.

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

app="dist/DonnyWhisper.app"
entitlements="apps/donnywhisper-macos/Resources/DonnyWhisper.entitlements"
notarize_zip="dist/DonnyWhisper-notarize.zip"

: "${MACOS_SIGN_IDENTITY:?MACOS_SIGN_IDENTITY must be set}"
: "${APPLE_ID:?APPLE_ID must be set}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID must be set}"
: "${APPLE_APP_SPECIFIC_PASSWORD:?APPLE_APP_SPECIFIC_PASSWORD must be set}"

if [[ ! -d "$app" ]]; then
    echo "error: $app not found — run scripts/build-macos-app.sh first" >&2
    exit 1
fi

echo "==> Signing $app with hardened runtime"
# Sign the LLM helper explicitly first — `--deep` does not reliably treat a
# second Mach-O in Contents/MacOS as nested code.
codesign --force --timestamp --options=runtime \
    --sign "$MACOS_SIGN_IDENTITY" \
    "$app/Contents/MacOS/donnywhisper-llm-helper"
codesign --force --deep --timestamp --options=runtime \
    --entitlements "$entitlements" \
    --sign "$MACOS_SIGN_IDENTITY" \
    "$app"

codesign --verify --deep --strict --verbose=2 "$app"

echo "==> Zipping for notarization submission"
# ditto preserves xattrs/symlinks; `zip` can strip them and trip up notarization.
rm -f "$notarize_zip"
/usr/bin/ditto -c -k --keepParent "$app" "$notarize_zip"

echo "==> Submitting to Apple notary service (this typically takes 2–15 minutes)"
xcrun notarytool submit "$notarize_zip" \
    --apple-id "$APPLE_ID" \
    --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --wait \
    --timeout 30m

echo "==> Stapling notarization ticket"
xcrun stapler staple "$app"
xcrun stapler validate "$app"

echo "==> Verifying Gatekeeper acceptance"
spctl --assess --type execute --verbose=4 "$app"

rm -f "$notarize_zip"
echo "==> Done: $app is signed, notarized, and stapled"
