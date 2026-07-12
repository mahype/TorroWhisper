# Sparkle Auto-Updates — Design

**Status:** Approved — ready for implementation plan
**Owner:** Sven Wagener
**Date:** 2026-04-18

## Goal

TorroWhisper checks for new releases automatically and offers them to the user. Users should see a pending update within a day of publication, with a one-click restart to install — without the developer having to maintain custom update plumbing.

## Non-Goals

- Beta / pre-release channel (single stable channel only)
- Delta updates (full DMG re-download per version)
- Bundled release-notes UI beyond Sparkle's default
- Automatic rollback to the previous version
- Windows / Linux update flow (those platforms don't have a UI shell yet)

## Constraints

- Distribution stays on GitHub Releases (signed + notarized DMG built by `.github/workflows/release.yml`)
- Must not require additional macOS entitlements
- Must not leak the Ed25519 private key into the repository or CI logs
- Update check must be a single network request to a stable URL (no GitHub-API rate limits)

## Architecture

```
┌────────────────────────────┐       ┌─────────────────────────────────────────┐
│  TorroWhisper (running)    │       │  https://mahype.github.io/TorroWhisper/ │
│                            │──GET─▶│  appcast.xml  (gh-pages branch)         │
│  SPUStandardUpdaterController      └───────────────────┬─────────────────────┘
│  (Sparkle, check every 24h)│                           │
└──────────┬─────────────────┘                           │ written by
           │ compare CFBundleShortVersionString          │ release workflow
           │ vs enclosure version                        │
           ▼                                             │
     newer available                                     │
           │                                             ▼
           │                          ┌─────────────────────────────────────────┐
           ├─────────────────────────▶│  github.com/mahype/TorroWhisper/        │
           │  download DMG            │  releases/download/vX.Y.Z/              │
           │                          │  TorroWhisper-X.Y.Z.dmg                  │
           │                          └─────────────────────────────────────────┘
           ▼
     verify Ed25519 signature against
     SUPublicEDKey in Info.plist
           │
           ▼
     auto-stage update (SUAutomaticallyUpdate = YES)
           │
           ▼
     prompt: "Update ready — Install on Quit / Install and Relaunch"
           │
           ▼
     Sparkle replaces .app, relaunches
```

## Components

### 1. Swift app side

**New SPM dependency** in `apps/torrowhisper-macos/Package.swift`:
- Product: `Sparkle` from `https://github.com/sparkle-project/Sparkle`
- Version rule: `from: "2.6.0"` (latest stable 2.x at design time; any 2.x is acceptable as long as SPM support is present)

**New file** `apps/torrowhisper-macos/Sources/TorroWhisper/UpdaterController.swift`:
- Thin wrapper around `SPUStandardUpdaterController`
- Owns the updater instance for the lifetime of the app (required — Sparkle will stop checking if the controller is deallocated)
- Exposes:
  - `checkForUpdates()` — manual trigger
  - `automaticallyChecksForUpdates: Bool` — pass-through to `updater.automaticallyChecksForUpdates`

**AppDelegate integration** (`AppDelegate.swift`, optionally reflected in `TorroWhisperApp.swift`):
- Instantiate `UpdaterController` in `applicationDidFinishLaunching`
- Hold the reference on the delegate (property, not local)

**Menu-bar menu** (wherever the `NSStatusItem` menu is built — currently in `AppDelegate.swift`):
- New `NSMenuItem` titled **“Check for Updates…”** above the *Quit* item
- Action wired to `UpdaterController.checkForUpdates()`

**Settings UI** (new `UpdatesSettingsView.swift` added as a new tab to the existing `SettingsView.swift`):
- Section header: *Updates*
- Toggle: **Automatically check for updates** — bound to `UpdaterController.automaticallyChecksForUpdates`, default `true`
- Button: **Check Now** — calls `UpdaterController.checkForUpdates()`
- No version label in this spec (keep minimal; can be added later if needed)

**`Info.plist` additions** (`apps/torrowhisper-macos/Resources/Info.plist`):

| Key | Value |
| --- | --- |
| `SUFeedURL` | `https://mahype.github.io/TorroWhisper/appcast.xml` |
| `SUPublicEDKey` | Base64 of Ed25519 public key (literal string, from `generate_keys` output) |
| `SUEnableAutomaticChecks` | `YES` |
| `SUAutomaticallyUpdate` | `YES` |
| `SUScheduledCheckInterval` | `86400` |

Sparkle persists user preferences under its own keys in `NSUserDefaults` — no custom persistence needed.

### 2. Appcast hosting

- **Branch:** `gh-pages`, orphan (no shared history with `main`)
- **Files:** `appcast.xml` at the branch root. Optional `index.html` that 200s with a human-readable pointer (not required by Sparkle)
- **Initial state:** a valid but empty appcast:
  ```xml
  <?xml version="1.0" standalone="yes"?>
  <rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
    <channel>
      <title>TorroWhisper</title>
      <link>https://mahype.github.io/TorroWhisper/appcast.xml</link>
      <description>Official update feed for TorroWhisper.</description>
      <language>en</language>
    </channel>
  </rss>
  ```
- **GitHub Pages:** Repo → *Settings → Pages* → Source `gh-pages`, folder `/ (root)`
- **Public URL:** `https://mahype.github.io/TorroWhisper/appcast.xml`

### 3. Key management

- One-time, on the developer's Mac:
  ```bash
  /path/to/Sparkle.app/Contents/MacOS/generate_keys
  ```
- The command prints the Ed25519 public key (Base64) and stores the private key in the macOS Keychain under its own service entry
- **Public key** → literal string in `Info.plist` (committed)
- **Private key** → exported and stored as GitHub Actions secret **`SPARKLE_ED_PRIVATE_KEY`**
  ```bash
  /path/to/Sparkle.app/Contents/MacOS/generate_keys -x private.key
  gh secret set SPARKLE_ED_PRIVATE_KEY < private.key
  rm private.key
  ```
- The private key is **never** checked into the repo

### 4. Release pipeline additions

**New script** `scripts/update-appcast.sh`:
- Inputs (via arguments or env vars):
  - `DMG_PATH` — path to the built, signed, notarized DMG
  - `VERSION` — semver without `v` prefix (e.g. `0.1.0`)
  - `RELEASE_NOTES_URL` — URL to the GitHub Release page
  - `APPCAST_PATH` — path to `appcast.xml` in the checked-out `gh-pages` worktree
  - `ED_PRIVATE_KEY` — Ed25519 private key (from secret)
- Steps:
  1. Write `ED_PRIVATE_KEY` to a temp file with mode `0600`
  2. Run `sign_update -f <keyfile> <DMG_PATH>` → parse Ed25519 signature and byte length from output
  3. Build a new `<item>` XML block with:
     - `<title>Version <version></title>`
     - `<pubDate>` — RFC 822 formatted current time
     - `<sparkle:version>` — `CFBundleVersion` of the DMG (matches `CFBundleShortVersionString`; we do not use a separate build number)
     - `<sparkle:shortVersionString>` — `VERSION`
     - `<sparkle:releaseNotesLink>` — `RELEASE_NOTES_URL`
     - `<sparkle:minimumSystemVersion>` — `14.0`
     - `<enclosure url="..." sparkle:edSignature="..." length="..." type="application/octet-stream" />`
  4. Insert the new `<item>` immediately after `<channel>`'s metadata elements (newest-first)
  5. Shred the temp key file

Implementation: pure shell + `awk`/`sed` for XML munging is brittle. Use a small Python one-liner (`macos-14` runners ship Python 3) or a here-doc template. Lean on Python for correctness.

**Workflow changes** in `.github/workflows/release.yml`, inserted **after** the existing `Build DMG` step and **before** the existing `Create draft GitHub Release` step:

1. **Checkout `gh-pages` into a separate worktree**
   ```yaml
   - name: Checkout gh-pages for appcast
     uses: actions/checkout@v4
     with:
       ref: gh-pages
       path: gh-pages
   ```
2. **Update appcast**
   ```yaml
   - name: Update appcast.xml
     env:
       SPARKLE_ED_PRIVATE_KEY: ${{ secrets.SPARKLE_ED_PRIVATE_KEY }}
       TAG: ${{ github.ref_name }}
     run: |
       version="${TAG#v}"
       ./scripts/update-appcast.sh \
         "dist/TorroWhisper-${version}.dmg" \
         "$version" \
         "https://github.com/mahype/TorroWhisper/releases/tag/${TAG}" \
         "gh-pages/appcast.xml"
   ```
3. **Commit and push the appcast update**
   ```yaml
   - name: Publish appcast
     working-directory: gh-pages
     run: |
       git config user.name "github-actions[bot]"
       git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
       git add appcast.xml
       git commit -m "appcast: publish ${GITHUB_REF_NAME}"
       git push origin gh-pages
   ```

The Sparkle CLI binaries (`sign_update`, `generate_keys`) are part of the Sparkle SPM checkout and appear under the build products directory. The script should locate them via `swift package --package-path apps/torrowhisper-macos describe --type json` or by downloading the Sparkle release tarball. Downloading the pinned release archive is simpler and faster — a step earlier in the workflow caches it.

**Note on permissions:** `.github/workflows/release.yml` already sets `permissions.contents: write`. That scope is sufficient to push to `gh-pages`.

## Data flow on update

1. `SPUStandardUpdaterController` fires 1x on launch + every 86400 s afterwards
2. HTTP GET against `SUFeedURL`
3. Parse XML, compare highest `sparkle:shortVersionString` vs running `CFBundleShortVersionString`
4. If newer: download enclosure URL to `~/Library/Caches/Sparkle/...`
5. Verify `sparkle:edSignature` against `SUPublicEDKey`
6. With `SUAutomaticallyUpdate = YES`, Sparkle stages the update and shows a non-modal notification "A new version is ready — Install on Quit / Install and Relaunch"
7. On user confirmation, Sparkle relaunches the app from the staged update bundle

## Error handling

| Failure mode | Sparkle behavior |
| --- | --- |
| No internet | Silent; retries on next scheduled check |
| Appcast XML malformed | Error shown only for manual "Check for Updates…"; automatic checks keep polling |
| Ed25519 signature mismatch | **Installation refused**; error dialog shown. Explicit security alert — never silently proceed |
| Download interrupted (network drop) | Retried on next scheduled check |
| `SUFeedURL` unreachable (404) | Same as no internet |
| User has disabled automatic checks | Only manual "Check for Updates…" triggers a check |

No custom error surface is added — Sparkle's defaults are sufficient and user-familiar.

## Rollout plan

Because no 0.1.0 user install exists yet (the current draft release is untouched), Sparkle can be baked into the very first published release. There is no legacy-installs migration to handle.

1. Implement the Swift side (SPM dep, `UpdaterController`, menu item, Settings tab, `Info.plist` entries)
2. Generate Ed25519 keypair locally; commit the public key into `Info.plist`; push `SPARKLE_ED_PRIVATE_KEY` as a GitHub secret
3. Create the orphan `gh-pages` branch with the empty-but-valid `appcast.xml`; enable GitHub Pages
4. Add `scripts/update-appcast.sh`; extend `.github/workflows/release.yml` with the three new steps
5. Delete the current draft release and the `v0.1.0` tag (local + remote)
6. Re-tag `v0.1.0` on the commit that includes Sparkle; workflow runs end-to-end
7. **Smoke test:** locally build an artifact with `CFBundleShortVersionString = 0.0.9`, launch it, trigger "Check for Updates…", confirm Sparkle shows 0.1.0 available, proceeds through download/verify/stage, and relaunches into 0.1.0
8. Publish the v0.1.0 draft

## Testing

**Manual smoke tests** (no automated test harness planned — the interesting failure modes involve networks, signatures, and app-replacement which are painful to mock):

- Fresh build with low `CFBundleShortVersionString` → "Check for Updates…" finds the published version
- Tamper with the DMG after signing → signature check fails, install blocked
- Deliberately break `SUFeedURL` → graceful no-op in background, error in manual check
- Disable automatic checks via Settings → restart app → no 24h check fires (verify via Sparkle log if needed)

**Unit tests** are not in scope for this spec; the Sparkle integration is mostly configuration.

## Risks

- **Losing the Ed25519 private key.** If the GitHub secret is deleted and no local backup exists, future releases cannot be signed for the current public key. Recovery: generate a new keypair, ship it in a new release. Users on the old public key *cannot* update to the new key automatically — they must manually download once. **Mitigation:** the developer must keep an offline backup of the private key (password manager, encrypted USB).
- **Appcast outage.** If `gh-pages` deployment breaks, updates silently stall. Mitigation: the 24 h interval means delays, not failures. Manual "Check for Updates…" will surface the error.
- **Sparkle CVEs.** Sparkle has had historical CVEs (pre-2.0). Using 2.x with Ed25519 enclosure signatures is the current recommended hardening. Pin to an explicit minimum version; follow Sparkle's security advisories.

## Open questions

None. All ambiguity resolved during brainstorming.
