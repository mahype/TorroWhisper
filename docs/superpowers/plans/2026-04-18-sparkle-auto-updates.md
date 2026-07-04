# Sparkle Auto-Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Donny with an in-app auto-updater (Sparkle 2.x) wired into the existing signed-and-notarized release pipeline. Updates are checked in the background every 24 h, downloaded automatically, and applied on user confirmation.

**Architecture:** Sparkle framework embedded via SPM. A gh-pages-hosted `appcast.xml` advertises new releases. The existing `release.yml` workflow is extended to sign each DMG with an Ed25519 key and append an entry to the appcast. No backwards-compat concerns since the v0.1.0 draft will be re-cut with Sparkle included.

**Tech Stack:** Sparkle 2.x (SPM), Swift/AppKit, SwiftUI, bash, Python 3 (for XML munging on the runner), GitHub Actions, GitHub Pages.

**Spec reference:** [docs/superpowers/specs/2026-04-18-sparkle-auto-updates-design.md](../specs/2026-04-18-sparkle-auto-updates-design.md)

---

## Pre-flight

The plan assumes:
- You are on `main` with a clean working tree.
- `gh` CLI is authenticated and points at `mahype/donny`.
- The user has already set the Apple-signing secrets (`APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`).
- The v0.1.0 tag and draft release currently exist and will be recreated after Sparkle is wired in.

Run once before starting:

```bash
git pull --rebase origin main
./scripts/dev.sh  # make sure the baseline builds before we change anything
```

Kill the running app once you've confirmed it launched.

---

## Task 1: Add Sparkle as an SPM dependency

**Files:**
- Modify: `apps/donny-macos/Package.swift`

- [ ] **Step 1: Edit `Package.swift` to add the Sparkle package**

Replace the contents of `apps/donny-macos/Package.swift` with:

```swift
// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Donny",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "Donny", targets: ["Donny"]),
    ],
    dependencies: [
        .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.6.0"),
    ],
    targets: [
        .systemLibrary(
            name: "DonnyBridgeFFI",
            path: "Bridge"
        ),
        .executableTarget(
            name: "Donny",
            dependencies: [
                "DonnyBridgeFFI",
                .product(name: "Sparkle", package: "Sparkle"),
            ],
            path: "Sources/Donny",
            linkerSettings: [
                .unsafeFlags(["-L", "../../target/debug", "-ldonny_bridge"]),
                .linkedLibrary("c++"),
                .linkedFramework("Accelerate"),
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("Carbon"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
    ]
)
```

- [ ] **Step 2: Resolve the new dependency**

Run: `swift package --package-path apps/donny-macos resolve`

Expected: Sparkle is fetched and `Package.resolved` is updated/created. No errors.

- [ ] **Step 3: Verify it still builds**

Run: `./scripts/dev.sh` and wait for the app window / menu-bar item. Quit with the menu-bar *Beenden* item.

Expected: app launches normally.

- [ ] **Step 4: Commit**

```bash
git add apps/donny-macos/Package.swift apps/donny-macos/Package.resolved
git commit -m "deps: add Sparkle 2.x for auto-updates"
```

---

## Task 2: Create `UpdaterController`

**Files:**
- Create: `apps/donny-macos/Sources/Donny/UpdaterController.swift`

- [ ] **Step 1: Create the file**

Write `apps/donny-macos/Sources/Donny/UpdaterController.swift`:

```swift
import Foundation
import Sparkle

/// Thin wrapper around Sparkle's standard updater controller.
/// Holds the Sparkle instance for the lifetime of the app; discarding the
/// controller silently stops background update checks.
@MainActor
final class UpdaterController {
    private let controller: SPUStandardUpdaterController

    init() {
        self.controller = SPUStandardUpdaterController(
            startingUpdater: true,
            updaterDelegate: nil,
            userDriverDelegate: nil
        )
    }

    func checkForUpdates() {
        controller.checkForUpdates(nil)
    }

    var automaticallyChecksForUpdates: Bool {
        get { controller.updater.automaticallyChecksForUpdates }
        set { controller.updater.automaticallyChecksForUpdates = newValue }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `swift build --package-path apps/donny-macos`

Expected: build succeeds.

- [ ] **Step 3: Commit**

```bash
git add apps/donny-macos/Sources/Donny/UpdaterController.swift
git commit -m "feat(updates): add UpdaterController wrapping Sparkle"
```

---

## Task 3: Wire `UpdaterController` into `AppDelegate` and add the menu item

**Files:**
- Modify: `apps/donny-macos/Sources/Donny/AppDelegate.swift`

- [ ] **Step 1: Add the `updaterController` property**

In `AppDelegate`, just below the existing `let model = AppModel()` line (around line 6), insert:

```swift
    let updaterController = UpdaterController()
```

- [ ] **Step 2: Declare the new menu item property**

Just below `private var quitItem: NSMenuItem!` (around line 17), insert:

```swift
    private var checkForUpdatesItem: NSMenuItem!
```

- [ ] **Step 3: Instantiate the menu item inside `applicationDidFinishLaunching`**

In the block where other menu items are constructed (before `statusMenu.delegate = self`), add:

```swift
        checkForUpdatesItem = NSMenuItem(
            title: "Nach Updates suchen...",
            action: #selector(checkForUpdates),
            keyEquivalent: ""
        )
```

- [ ] **Step 4: Insert the menu item before Quit**

Replace the `statusMenu.items = [...]` assignment with:

```swift
        statusMenu.items = [
            dictationItem,
            .separator(),
            settingsItem,
            onboardingItem,
            .separator(),
            modeSummaryItem,
            modeSwitchItem,
            .separator(),
            modelItem,
            statusItemLine,
            .separator(),
            checkForUpdatesItem,
            quitItem,
        ]
```

- [ ] **Step 5: Add the `@objc` action**

Below `@objc private func quitApp()` (around line 115), add:

```swift
    @objc private func checkForUpdates() {
        updaterController.checkForUpdates()
    }
```

- [ ] **Step 6: Verify the app builds and the menu item is wired**

Run: `./scripts/dev.sh`

Manual verification: open the menu-bar icon → confirm the new item **„Nach Updates suchen..."** appears above *Beenden*. Click it → expect the Sparkle "Could not download the feed" or similar dialog (we haven't pointed `SUFeedURL` anywhere yet). That's fine — it proves wiring works.

- [ ] **Step 7: Commit**

```bash
git add apps/donny-macos/Sources/Donny/AppDelegate.swift
git commit -m "feat(updates): add Check-for-Updates menu item"
```

---

## Task 4: Add Updates settings tab

**Files:**
- Modify: `apps/donny-macos/Sources/Donny/AppUIComponents.swift:3-46`
- Modify: `apps/donny-macos/Sources/Donny/SettingsView.swift:41-56`
- Create: `apps/donny-macos/Sources/Donny/UpdatesSettingsView.swift`

- [ ] **Step 1: Extend `SettingsSection` enum**

In `AppUIComponents.swift`, replace the enum (lines 3–46) with:

```swift
enum SettingsSection: String, CaseIterable, Identifiable {
    case recording
    case modes
    case model
    case startup
    case providers
    case updates
    case diagnostics

    var id: String { rawValue }

    var title: String {
        switch self {
        case .recording:
            return "Aufnahme"
        case .modes:
            return "Modi"
        case .model:
            return "Sprachmodell"
        case .startup:
            return "Start & Verhalten"
        case .providers:
            return "Optionale Provider"
        case .updates:
            return "Updates"
        case .diagnostics:
            return "Diagnose"
        }
    }

    var symbolName: String {
        switch self {
        case .recording:
            return "mic.fill"
        case .modes:
            return "square.text.square"
        case .model:
            return "square.stack.3d.up.fill"
        case .startup:
            return "power.circle.fill"
        case .providers:
            return "server.rack"
        case .updates:
            return "arrow.triangle.2.circlepath"
        case .diagnostics:
            return "checklist"
        }
    }
}
```

- [ ] **Step 2: Create `UpdatesSettingsView.swift`**

Write `apps/donny-macos/Sources/Donny/UpdatesSettingsView.swift`:

```swift
import AppKit
import SwiftUI

struct UpdatesSettingsView: View {
    let updaterController: UpdaterController
    @State private var autoCheck: Bool

    init(updaterController: UpdaterController) {
        self.updaterController = updaterController
        _autoCheck = State(initialValue: updaterController.automaticallyChecksForUpdates)
    }

    var body: some View {
        Section("Automatische Updates") {
            Toggle("Automatisch nach Updates suchen", isOn: Binding(
                get: { autoCheck },
                set: { newValue in
                    autoCheck = newValue
                    updaterController.automaticallyChecksForUpdates = newValue
                }
            ))

            HStack {
                Text("Manuell prüfen:")
                Spacer()
                Button("Jetzt nach Updates suchen") {
                    updaterController.checkForUpdates()
                }
            }
        }

        Section {
            Text("""
            Donny prüft beim Start und danach alle 24 Stunden auf neue \
            Versionen. Updates werden im Hintergrund heruntergeladen und installiert, \
            sobald du neu startest.
            """)
            .font(.callout)
            .foregroundStyle(.secondary)
        }
    }
}
```

- [ ] **Step 3: Wire it into `SettingsView.detailContent`**

In `SettingsView.swift`, the view needs access to the `UpdaterController`. Replace the `@ViewBuilder private func detailContent(for section:)` block (lines 40–56) with:

```swift
    @ViewBuilder
    private func detailContent(for section: SettingsSection) -> some View {
        switch section {
        case .recording:
            recordingContent
        case .modes:
            modesContent
        case .model:
            modelContent
        case .startup:
            startupContent
        case .providers:
            providersContent
        case .updates:
            UpdatesSettingsView(updaterController: updaterController)
        case .diagnostics:
            diagnosticsContent
        }
    }
```

At the top of `SettingsView` (just below `@ObservedObject var model: AppModel`), add:

```swift
    let updaterController: UpdaterController
```

- [ ] **Step 4: Pass the controller in from `AppDelegate`**

In `AppDelegate.swift`, find `@objc private func showSettings(_ sender: Any?)` (around line 82) and change the `rootView:` argument:

Old:
```swift
            rootView: SettingsView(model: model)
```

New:
```swift
            rootView: SettingsView(model: model, updaterController: updaterController)
```

- [ ] **Step 5: Build and verify**

Run: `./scripts/dev.sh`

Manual: open Settings → sidebar shows a new **„Updates"** entry with the `arrow.triangle.2.circlepath` glyph. Click it → the toggle and "Jetzt nach Updates suchen"-button render. Flip the toggle off → confirm it stays off after reopening Settings (Sparkle persists this in `NSUserDefaults`).

- [ ] **Step 6: Commit**

```bash
git add apps/donny-macos/Sources/Donny/AppUIComponents.swift \
        apps/donny-macos/Sources/Donny/SettingsView.swift \
        apps/donny-macos/Sources/Donny/UpdatesSettingsView.swift
git commit -m "feat(updates): add Updates tab in Settings"
```

---

## Task 5: Generate Ed25519 keypair and wire Info.plist

**Files:**
- Modify: `apps/donny-macos/Resources/Info.plist`

This task requires the **developer** (not the agent) to handle the private key.

- [ ] **Step 1: Locate Sparkle's `generate_keys` binary**

After Task 1 resolved Sparkle via SPM, the binaries live under the SPM checkout:

```bash
find apps/donny-macos/.build -type f -name generate_keys 2>/dev/null | head -1
```

Expected: a path like `apps/donny-macos/.build/artifacts/sparkle/Sparkle/Sparkle.app/Contents/MacOS/generate_keys`. If not found, run `swift build --package-path apps/donny-macos` first (Sparkle artifacts are pulled lazily).

- [ ] **Step 2: Generate the keypair (developer action)**

```bash
GEN="$(find apps/donny-macos/.build -type f -name generate_keys | head -1)"
"$GEN"
```

Expected output:
```
A key has been generated and saved in your keychain.
Public key (SUPublicEDKey value):
<base64 string>
```

**Copy the public-key Base64 string** — you need it in Step 3.

- [ ] **Step 3: Add the Sparkle keys to `Info.plist`**

Edit `apps/donny-macos/Resources/Info.plist`. Insert the following keys immediately before the closing `</dict>` (replace `<THE_PUBLIC_KEY_FROM_STEP_2>` with the actual Base64 string):

```xml
    <key>SUFeedURL</key>
    <string>https://mahype.github.io/donny/appcast.xml</string>
    <key>SUPublicEDKey</key>
    <string><THE_PUBLIC_KEY_FROM_STEP_2></string>
    <key>SUEnableAutomaticChecks</key>
    <true/>
    <key>SUAutomaticallyUpdate</key>
    <true/>
    <key>SUScheduledCheckInterval</key>
    <integer>86400</integer>
```

- [ ] **Step 4: Export the private key and push it as a GitHub secret (developer action)**

```bash
GEN="$(find apps/donny-macos/.build -type f -name generate_keys | head -1)"
"$GEN" -x /tmp/sparkle-private.key
gh secret set SPARKLE_ED_PRIVATE_KEY < /tmp/sparkle-private.key
rm -P /tmp/sparkle-private.key
```

Verify:
```bash
gh secret list | grep SPARKLE_ED_PRIVATE_KEY
```

Expected: the secret is listed with a recent timestamp.

**Back up the private key somewhere safe** (password manager, encrypted USB). Losing it means future releases cannot be signed for the current public key — existing installs would then need a one-time manual re-install to pick up a new key.

- [ ] **Step 5: Verify the bundled app has the keys**

```bash
./scripts/build-macos-app.sh
/usr/libexec/PlistBuddy -c "Print :SUFeedURL" "dist/Donny.app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Print :SUPublicEDKey" "dist/Donny.app/Contents/Info.plist"
```

Expected: the feed URL and the public-key Base64 are printed.

- [ ] **Step 6: Commit**

```bash
git add apps/donny-macos/Resources/Info.plist
git commit -m "feat(updates): add Sparkle feed URL and public key to Info.plist"
```

---

## Task 6: Create the `gh-pages` branch and enable GitHub Pages

**Files:**
- Create: `appcast.xml` on the `gh-pages` branch (not on `main`)

- [ ] **Step 1: Create the orphan branch and initial appcast**

From the `main` working tree:

```bash
git worktree add --detach /tmp/ow-gh-pages
cd /tmp/ow-gh-pages
git checkout --orphan gh-pages
git rm -rf . 2>/dev/null || true

cat > appcast.xml <<'XML'
<?xml version="1.0" standalone="yes"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Donny</title>
    <link>https://mahype.github.io/donny/appcast.xml</link>
    <description>Official update feed for Donny.</description>
    <language>en</language>
  </channel>
</rss>
XML

git add appcast.xml
git commit -m "chore(appcast): initialise empty feed"
git push -u origin gh-pages
cd -
git worktree remove /tmp/ow-gh-pages
```

- [ ] **Step 2: Enable GitHub Pages (developer action via gh CLI)**

```bash
gh api --method POST /repos/mahype/donny/pages \
  -f source.branch=gh-pages -f source.path=/
```

Expected: JSON response with `status`, `url` (HTTPS URL), `source`. If Pages is already enabled, the API returns 409 — that's fine, skip this step.

Verify the site is public:

```bash
sleep 30  # Pages takes a minute on first setup
curl -sI https://mahype.github.io/donny/appcast.xml | head -3
```

Expected: `HTTP/2 200`. If 404, wait 2–3 minutes and retry — Pages' initial deploy is slow.

- [ ] **Step 3: Sanity check from the app's perspective**

```bash
curl -s https://mahype.github.io/donny/appcast.xml | head -5
```

Expected: the XML you committed, starting with `<?xml version="1.0" ...`.

- [ ] **Step 4: No commit on `main`**

This task doesn't touch `main`. Move on.

---

## Task 7: Write `scripts/update-appcast.sh`

**Files:**
- Create: `scripts/update-appcast.sh`
- Create: `scripts/_appcast_insert.py` (helper, Python 3)
- Test: manual smoke test

- [ ] **Step 1: Create the Python helper**

Write `scripts/_appcast_insert.py`:

```python
#!/usr/bin/env python3
"""
Prepend a <item> entry to a Sparkle appcast.xml. Everything comes in via
env vars so the caller (a shell script) doesn't have to juggle quoting.

Required env:
  APPCAST_PATH, VERSION, RELEASE_NOTES_URL, DMG_URL, DMG_LENGTH,
  DMG_ED_SIGNATURE, MIN_SYSTEM_VERSION, PUB_DATE
"""
import os
import sys
from xml.etree import ElementTree as ET

NS_SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", NS_SPARKLE)


def require(name):
    value = os.environ.get(name)
    if not value:
        sys.exit(f"error: {name} is required")
    return value


def main():
    path = require("APPCAST_PATH")
    version = require("VERSION")
    notes_url = require("RELEASE_NOTES_URL")
    dmg_url = require("DMG_URL")
    dmg_len = require("DMG_LENGTH")
    ed_sig = require("DMG_ED_SIGNATURE")
    min_sys = require("MIN_SYSTEM_VERSION")
    pub_date = require("PUB_DATE")

    tree = ET.parse(path)
    channel = tree.getroot().find("channel")
    if channel is None:
        sys.exit("error: <channel> not found in appcast")

    if any(
        elem.findtext(f"{{{NS_SPARKLE}}}shortVersionString") == version
        for elem in channel.findall("item")
    ):
        print(f"appcast already contains version {version}; skipping", file=sys.stderr)
        return

    item = ET.Element("item")
    ET.SubElement(item, "title").text = f"Version {version}"
    ET.SubElement(item, "pubDate").text = pub_date
    ET.SubElement(item, f"{{{NS_SPARKLE}}}shortVersionString").text = version
    ET.SubElement(item, f"{{{NS_SPARKLE}}}version").text = version
    ET.SubElement(item, f"{{{NS_SPARKLE}}}releaseNotesLink").text = notes_url
    ET.SubElement(item, f"{{{NS_SPARKLE}}}minimumSystemVersion").text = min_sys
    ET.SubElement(
        item,
        "enclosure",
        {
            "url": dmg_url,
            f"{{{NS_SPARKLE}}}edSignature": ed_sig,
            "length": dmg_len,
            "type": "application/octet-stream",
        },
    )

    last_meta_idx = 0
    for idx, elem in enumerate(list(channel)):
        if elem.tag != "item":
            last_meta_idx = idx
    channel.insert(last_meta_idx + 1, item)

    ET.indent(tree, space="  ")
    tree.write(path, encoding="UTF-8", xml_declaration=True)


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Create the shell wrapper**

Write `scripts/update-appcast.sh`:

```bash
#!/usr/bin/env bash
# Signs a DMG with the Sparkle Ed25519 key and prepends a matching <item>
# to an existing appcast.xml.
#
# Positional arguments:
#   1  DMG path                       e.g. dist/Donny-0.1.0.dmg
#   2  Version (no `v` prefix)         e.g. 0.1.0
#   3  Release-notes URL               e.g. https://github.com/.../releases/tag/v0.1.0
#   4  Appcast path                    e.g. gh-pages/appcast.xml
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
MIN_SYSTEM_VERSION="${MIN_SYSTEM_VERSION:-14.0}"

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
    SIGN_UPDATE="$(find "$repo_root/apps/donny-macos/.build" \
        -type f -name sign_update 2>/dev/null | head -1)"
fi
if [[ ! -x "${SIGN_UPDATE:-}" ]]; then
    echo "error: sign_update binary not found (tried $SIGN_UPDATE). Run 'swift build --package-path apps/donny-macos' first." >&2
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
dmg_url="https://github.com/mahype/donny/releases/download/v${VERSION}/${dmg_filename}"
pub_date="$(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S +0000')"

APPCAST_PATH="$APPCAST_PATH" \
VERSION="$VERSION" \
RELEASE_NOTES_URL="$RELEASE_NOTES_URL" \
DMG_URL="$dmg_url" \
DMG_LENGTH="$length" \
DMG_ED_SIGNATURE="$ed_sig" \
MIN_SYSTEM_VERSION="$MIN_SYSTEM_VERSION" \
PUB_DATE="$pub_date" \
python3 "$repo_root/scripts/_appcast_insert.py"

echo "appcast updated: added version $VERSION"
```

- [ ] **Step 3: Make both scripts executable**

```bash
chmod +x scripts/update-appcast.sh scripts/_appcast_insert.py
```

- [ ] **Step 4: Smoke-test `_appcast_insert.py` in isolation**

```bash
tmp="$(mktemp -d)"
cat > "$tmp/appcast.xml" <<'XML'
<?xml version="1.0" standalone="yes"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Donny</title>
    <link>https://mahype.github.io/donny/appcast.xml</link>
    <description>Official update feed for Donny.</description>
    <language>en</language>
  </channel>
</rss>
XML

APPCAST_PATH="$tmp/appcast.xml" \
VERSION="0.1.0" \
RELEASE_NOTES_URL="https://github.com/mahype/donny/releases/tag/v0.1.0" \
DMG_URL="https://github.com/mahype/donny/releases/download/v0.1.0/Donny-0.1.0.dmg" \
DMG_LENGTH="1234567" \
DMG_ED_SIGNATURE="FakeSignatureBase64==" \
MIN_SYSTEM_VERSION="14.0" \
PUB_DATE="Sat, 18 Apr 2026 09:00:00 +0000" \
python3 scripts/_appcast_insert.py

grep -q 'shortVersionString>0.1.0' "$tmp/appcast.xml" && echo PASS || echo FAIL

rm -rf "$tmp"
```

Expected: prints `PASS`.

- [ ] **Step 5: Full `update-appcast.sh` smoke-test is deferred**

A full run requires a real DMG and the Ed25519 private key, which we don't want to feed into a terminal unnecessarily. Task 11's end-to-end run on the GitHub runner is the authoritative test.

- [ ] **Step 6: Commit**

```bash
git add scripts/update-appcast.sh scripts/_appcast_insert.py
git commit -m "build(appcast): add script to sign DMG and update Sparkle feed"
```

---

## Task 8: Extend `release.yml` to publish the appcast

**Files:**
- Modify: `.github/workflows/release.yml`

- [ ] **Step 1: Add the `gh-pages` checkout + appcast publish steps**

In `.github/workflows/release.yml`, after the existing **Build DMG** step and **before** **Create draft GitHub Release**, insert these four steps:

```yaml
      - name: Build Sparkle tools
        run: |
          # Ensure sign_update and friends are built so update-appcast.sh can find them.
          swift build --package-path apps/donny-macos --product Sparkle >/dev/null 2>&1 || true
          find apps/donny-macos/.build -type f -name sign_update | head -1

      - name: Checkout gh-pages for appcast
        uses: actions/checkout@v4
        with:
          ref: gh-pages
          path: gh-pages

      - name: Update appcast.xml
        env:
          SPARKLE_ED_PRIVATE_KEY: ${{ secrets.SPARKLE_ED_PRIVATE_KEY }}
          TAG: ${{ github.ref_name }}
        run: |
          set -euo pipefail
          version="${TAG#v}"
          ./scripts/update-appcast.sh \
            "dist/Donny-${version}.dmg" \
            "$version" \
            "https://github.com/mahype/donny/releases/tag/${TAG}" \
            "gh-pages/appcast.xml"

      - name: Publish appcast
        working-directory: gh-pages
        env:
          TAG: ${{ github.ref_name }}
        run: |
          set -euo pipefail
          git config user.name  "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          git add appcast.xml
          if git diff --cached --quiet; then
            echo "appcast unchanged — nothing to publish"
            exit 0
          fi
          git commit -m "appcast: publish ${TAG}"
          git push origin gh-pages
```

- [ ] **Step 2: Verify workflow YAML is syntactically valid**

```bash
gh workflow view release.yml >/dev/null
```

Expected: no error; metadata is printed. (If `gh` complains, there's a YAML syntax error to fix.)

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: append appcast entry to gh-pages on every release"
```

- [ ] **Step 4: Push the accumulated commits so CI validates the Swift side**

```bash
git push origin main
```

Wait for the **CI** workflow (not Release — Release only fires on tags) to go green. Watch with:

```bash
gh run watch --exit-status
```

If CI fails, fix before continuing.

---

## Task 9: Tear down the existing v0.1.0 draft and tag

This is **destructive**. Only proceed once Tasks 1–8 are committed and pushed to `main`.

- [ ] **Step 1: Delete the draft release**

```bash
gh release list --limit 5
gh release delete v0.1.0 --yes --cleanup-tag   # --cleanup-tag also removes the remote tag
```

Expected: the draft disappears from `gh release list`, and the remote tag is gone.

- [ ] **Step 2: Delete the local tag**

```bash
git tag -d v0.1.0
```

Expected: `Deleted tag 'v0.1.0'`.

- [ ] **Step 3: Confirm clean state**

```bash
git tag -l "v*"            # should be empty
gh release list --limit 5  # no v0.1.0
```

---

## Task 10: Local build sanity check

Before cutting the release tag, verify the end-to-end build chain still works locally with all the Sparkle wiring in place.

- [ ] **Step 1: Build a local bundle**

```bash
VERSION="0.0.1" ./scripts/build-macos-app.sh
open "dist/Donny.app"
```

Expected: app launches as a menu-bar app. The new *Nach Updates suchen...* item is present. Clicking it triggers a Sparkle network check against `https://mahype.github.io/donny/appcast.xml` — with the current empty appcast, Sparkle will report "You're up to date". That's the right answer: the check itself works, there just isn't a newer version yet.

- [ ] **Step 2: Quit the local test build**

Menu bar → *Beenden*.

The real end-to-end flow (check finds a real newer version, downloads, verifies, installs) is exercised in Task 11 once the actual v0.1.0 is published to the appcast.

---

## Task 11: Re-cut v0.1.0 with Sparkle and verify end-to-end

- [ ] **Step 1: Create the annotated tag on the current `main` HEAD**

```bash
git tag -a v0.1.0 -m "Donny 0.1.0 — first macOS release with auto-updates"
git push origin v0.1.0
```

- [ ] **Step 2: Watch the release workflow**

```bash
run_id="$(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
gh run watch "$run_id" --exit-status
```

Expected: all steps green, including the three new steps (*Build Sparkle tools*, *Update appcast.xml*, *Publish appcast*). The *Checkout gh-pages for appcast* step must also succeed. Total runtime: 8–12 min.

If any step fails, read the log, fix, re-tag (`git tag -f` is OK because the tag is fresh), and push again.

- [ ] **Step 3: Confirm appcast now advertises 0.1.0**

```bash
sleep 30   # give Pages' CDN a moment to refresh
curl -s https://mahype.github.io/donny/appcast.xml | grep -A2 shortVersionString
```

Expected: the appcast contains `<sparkle:shortVersionString>0.1.0</sparkle:shortVersionString>` plus a real `edSignature`.

- [ ] **Step 4: Smoke-test on the developer machine**

```bash
VERSION="0.0.1" ./scripts/build-macos-app.sh
open "dist/Donny.app"
```

Manual verification:
- Open the menu bar icon → **Nach Updates suchen...**
- Sparkle shows a dialog "Version 0.1.0 is available".
- Click **Install Update** — Sparkle downloads the signed DMG, verifies the Ed25519 signature, and prompts for relaunch.
- Confirm relaunch. The app restarts as 0.1.0 (check *About* dialog or `defaults read ...`).

If the signature check fails, the most likely cause is a mismatch between the public key in Info.plist and the private key in `SPARKLE_ED_PRIVATE_KEY`. Confirm they were generated in the same `generate_keys` invocation.

- [ ] **Step 5: Publish the draft release**

Review the notes in the draft release on GitHub, edit if needed, then publish:

```bash
gh release edit v0.1.0 --draft=false
```

Expected: `Updated release ...`. The release is now public and the DMG is linked from the repo's *Releases* page.

- [ ] **Step 6: Final sanity check**

```bash
gh release view v0.1.0 --json isDraft,assets
```

Expected: `"isDraft": false`, assets include `Donny-0.1.0.dmg` and `SHA256SUMS.txt`.

- [ ] **Step 7: No commit — the release itself is the artifact**

---

## Rollback notes

If something goes wrong mid-task:
- **Task 1–4 issues** — `git reset --hard origin/main` in the local tree to revert (pre-push).
- **Key compromise / wrong keypair committed** — bump to a new keypair, re-publish Info.plist, force-push only if the bad commit is not on any remote. If it reached `origin/main`, consider it leaked and rotate (old private key is now untrusted).
- **Bad appcast entry (wrong signature or URL)** — manually edit `gh-pages/appcast.xml` and push a correction; Sparkle clients will pick it up on their next 24 h check or immediate manual check.
- **Broken release workflow after tag push** — fix the workflow on `main`, then `gh run rerun <id>` the failed release job. No need to recreate the tag if the commit itself is good.
