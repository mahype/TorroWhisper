# Releasing

This document is for maintainers. It describes how DonnyWhisper is versioned, built, signed, and published.

## TL;DR

```bash
# 1. Update versions
# 2. Tag and push
git tag v0.2.2
git push origin v0.2.2
# 3. Watch the release workflow on GitHub — it also appends the Sparkle appcast entry
# 4. Review the auto-generated release notes on GitHub, edit if needed
```

## Versioning

DonnyWhisper follows **SemVer**: `MAJOR.MINOR.PATCH`, optionally `-rc.N` for pre-releases.

The **Git tag is the source of truth** for the version. Before tagging:

1. Update `version` in the root [Cargo.toml](../Cargo.toml) under `[workspace.package]`.
2. The build script injects the version into the macOS bundle's `Info.plist` via `git describe --tags`.
3. Commit the Cargo.toml bump, then create the tag from that commit.

## Release tag format

- Stable: `v0.2.0`, `v0.2.1`, `v0.3.0`, `v1.0.0`
- Release candidate: `v0.2.2-rc.1`
- Any tag matching `v*` triggers [.github/workflows/release.yml](../.github/workflows/release.yml).

## What the release workflow does

On push of a `v*` tag, the GitHub Actions release workflow:

1. Runs the full CI suite (Rust fmt/clippy/test, macOS app build).
2. Builds a universal (arm64 + x86_64) Rust static library.
3. Builds a universal Swift executable and assembles `DonnyWhisper.app`.
4. Injects the version from the tag into `Info.plist`.
5. Signs the bundle with the **Developer ID Application** certificate.
6. Submits to Apple's notary service via `notarytool` and staples the ticket.
7. Packages the result into `DonnyWhisper-<version>.dmg`.
8. Runs a **DMG smoke test** ([scripts/smoke-test-dmg.sh](../scripts/smoke-test-dmg.sh)) that mounts the finished DMG and verifies `codesign`, Gatekeeper (`spctl`), and a valid stapled notarization ticket. A broken artifact halts the workflow **before** anything is published.
9. Uploads the DMG and a `SHA256SUMS.txt` to a **published** GitHub Release with auto-generated release notes.
10. Signs the DMG with the Sparkle Ed25519 key and appends a new `<item>` to `appcast.xml` on the `gh-pages` branch — with the release notes embedded as inline HTML — so existing installs see the update on their next check (see [Sparkle auto-updates](#sparkle-auto-updates)). The release is created first so the appcast never advertises a download URL that does not exist yet.

You can edit the auto-generated release notes afterwards on GitHub.

## Secrets required in the GitHub repository

Configure these under **Settings → Secrets and variables → Actions**:

| Secret | Purpose |
| --- | --- |
| `MACOS_CERTIFICATE_P12` | Base64-encoded `.p12` export of your *Developer ID Application* cert |
| `MACOS_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12` |
| `APPLE_ID` | Apple ID email tied to your developer account |
| `APPLE_TEAM_ID` | 10-character Team ID from [developer.apple.com → Membership](https://developer.apple.com/account/#MembershipDetailsCard) |
| `APPLE_APP_SPECIFIC_PASSWORD` | App-specific password generated at [appleid.apple.com](https://appleid.apple.com/account/manage) → *Sign-In and Security* → *App-Specific Passwords* |
| `SPARKLE_ED_PRIVATE_KEY` | Sparkle Ed25519 private key (generated once with `generate_keys`). Used by `scripts/update-appcast.sh` to sign each DMG. |

### Generating `MACOS_CERTIFICATE_P12`

1. In **Keychain Access**, find your *Developer ID Application: Your Name (TEAMID)* certificate.
2. Right-click → **Export…** → save as `DeveloperID.p12`, set a strong password.
3. Base64-encode it:
   ```bash
   base64 -i DeveloperID.p12 | pbcopy
   ```
4. Paste into the `MACOS_CERTIFICATE_P12` secret. Paste the export password into `MACOS_CERTIFICATE_PASSWORD`.

### Generating `APPLE_APP_SPECIFIC_PASSWORD`

`notarytool` does not accept your main Apple ID password. Go to [appleid.apple.com](https://appleid.apple.com/) → *Sign-In and Security* → *App-Specific Passwords* → generate one labeled e.g. `donnywhisper-notarization`.

## Test a release build locally

Before cutting a tag, verify the full pipeline on your machine:

```bash
# 1. Build the universal .app
./scripts/build-macos-app.sh

# 2. Sign + notarize (needs the env vars below set locally)
export MACOS_SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APPLE_ID="you@example.com"
export APPLE_TEAM_ID="XXXXXXXXXX"
export APPLE_APP_SPECIFIC_PASSWORD="xxxx-xxxx-xxxx-xxxx"
./scripts/codesign-macos.sh

# 3. Package the DMG
./scripts/build-dmg.sh

# 4. Verify Gatekeeper accepts the result
spctl --assess --type open --context context:primary-signature "dist/DonnyWhisper.app"
# Expected: "accepted, source=Notarized Developer ID"
```

## Sparkle auto-updates

DonnyWhisper ships with [Sparkle](https://sparkle-project.org/); every released DMG is signed with an Ed25519 key and advertised through an appcast on the `gh-pages` branch.

### One-time setup

Only needed once per project, or when rotating the signing key:

```bash
# From the checkout root, with Sparkle resolved via SwiftPM:
./apps/donnywhisper-macos/.build/.../generate_keys
```

The tool writes the key pair into your keychain and prints the **public key**. The public key is already embedded in [Info.plist](../apps/donnywhisper-macos/Resources/Info.plist) as `SUPublicEDKey`. Store the **private key** as the `SPARKLE_ED_PRIVATE_KEY` GitHub secret — never commit it.

### What happens on every release tag

The release workflow invokes [scripts/update-appcast.sh](../scripts/update-appcast.sh), which:

1. Signs the built DMG with Sparkle's `sign_update` using `SPARKLE_ED_PRIVATE_KEY`.
2. Prepends a new `<item>` to `appcast.xml` on the `gh-pages` branch, including the version, pub-date, minimum-system-version (14.0), signature, and the release notes converted to inline HTML (`<description>`). Embedding the notes keeps the Sparkle dialog clean — a `releaseNotesLink` to github.com would render the entire GitHub web page inside the update window.
3. Commits and pushes the updated appcast.

Version comparison: Sparkle compares the appcast's `sparkle:version` against the installed app's `CFBundleVersion`. The build script strips `git describe` suffixes (`0.4.0-4-gabc123[-dirty]` → `0.4.0`) from `CFBundleVersion`; the full describe string stays visible in `CFBundleShortVersionString`. Without the strip, a locally installed dev build compares as newer than every released version and silently blocks updates.

Users running a previous version will see the new release on their next scheduled check (every 24 h) or when they click *Settings → Updates → Check Now*.

### Rollback for a bad Sparkle release

If you need to pull an update without reverting the GitHub release itself, revert the appcast commit on `gh-pages` and push. Existing installs will stop seeing the bad version on their next check; users who already upgraded will not be auto-downgraded — they need to reinstall the previous DMG manually.

### Verifying the feed locally

```bash
curl -s https://mahype.github.io/DonnyWhisper/appcast.xml | head -40
```

The top `<item>` should match your latest tag with a non-empty `sparkle:edSignature` attribute.

## Rollback

If a release has a critical bug:

1. **Mark the GitHub release as pre-release** (or delete it). This removes it from the "Latest release" link users follow.
2. Revert the matching appcast `<item>` on `gh-pages` so new installs and existing users stop being offered the bad version (see [Sparkle auto-updates](#sparkle-auto-updates)).
3. Re-tag with a patch bump (`v0.2.2`) after pushing the fix. Do **not** re-use or re-point the broken tag — users and the notary service have already seen it.
4. If the DMG is already notarized and in the wild, you cannot revoke it remotely. The only signal users get is that a newer release exists; make the release notes clear about the issue.

## Troubleshooting a failed release workflow

**Signing step fails with "no identity found".**
`MACOS_CERTIFICATE_P12` was not imported correctly. Re-run the export + base64 process and double-check you're exporting the *Developer ID Application* cert, not *Developer ID Installer* or *Apple Development*.

**Notarization returns "Invalid".**
Run `xcrun notarytool log <submission-id> --apple-id … --team-id … --password …` to see the detailed rejection. The most common causes are missing hardened runtime (`--options=runtime`) or an unsigned nested binary inside the bundle.

**DMG upload step fails.**
Check that the workflow has `permissions: contents: write`. GitHub Actions requires explicit write permission on the repo to create releases.

**Smoke-test step fails.**
Inspect the Actions log for the output of `codesign`, `spctl`, or `xcrun stapler validate`. The three common causes are: (a) signing identity did not cover a nested binary inside the bundle, (b) notarization ticket was not stapled (the earlier `codesign-macos.sh` step reports submission timeouts in its own output), or (c) Gatekeeper rejects because hardened runtime was disabled. Resolve the earlier step, re-tag (do not re-use the broken tag — notary sees it as a duplicate).
