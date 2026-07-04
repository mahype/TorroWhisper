# Installation

## macOS

### Requirements

- macOS 14 (Sonoma) or later
- Apple Silicon (M1 or newer) or Intel x86_64

### Install

1. Download **`DonnyWhisper-<version>.dmg`** from the [latest release](https://github.com/mahype/DonnyWhisper/releases/latest).
2. Open the DMG and drag **DonnyWhisper.app** into the **Applications** folder.
3. Launch DonnyWhisper from Launchpad or Spotlight.
4. On first launch, macOS will verify the notarized signature. If you see a Gatekeeper warning instead of a regular launch, see [Troubleshooting](#troubleshooting) below.
5. Follow the in-app onboarding — it walks you through mic selection, model download, and startup behavior.

DonnyWhisper runs as a **menu bar app**. Look for its icon in the top-right of your screen — there is no Dock icon.

### Permissions

DonnyWhisper needs three macOS permissions. You'll be prompted for each the first time it's needed; grant all three for the full feature set.

| Permission | Why | Where to re-enable |
| --- | --- | --- |
| **Microphone** | Record what you say | System Settings → Privacy & Security → Microphone |
| **Accessibility** | Insert transcribed text into the active app via simulated paste | System Settings → Privacy & Security → Accessibility |
| **Input Monitoring** | Register the global hotkey | System Settings → Privacy & Security → Input Monitoring |

If you deny a permission by accident, quit DonnyWhisper, flip the toggle in System Settings, and relaunch. The in-app **Permissions** panel shows the current status of each.

### Start at login (autostart)

You have two equivalent ways to enable this:

- **In the app:** open Settings → *Startup* and choose **Launch at login**. The app registers itself as a macOS Login Item and launches hidden (menu bar only) on every sign-in.
- **In System Settings:** open System Settings → General → Login Items → toggle **DonnyWhisper** under *Open at Login*.

To disable autostart, flip either switch back off. You can also choose **Ask on first launch** in Settings to have DonnyWhisper show the prompt the next time you start it manually.

> **Note:** Autostart only works when DonnyWhisper is installed in `/Applications` or `~/Applications`. If you run it from a different folder (e.g., your Downloads), move the app first. If you move the app to a new location later (for example after a reinstall), the registered launch path is refreshed automatically on the next start — you don't need to re-toggle the Login Item.

### Update

Download the new DMG and drag the updated app over the old one. Your settings (in `~/Library/Application Support/donnywhisper/`) are preserved.

### Uninstall

1. Quit DonnyWhisper (menu bar icon → *Quit*).
2. In Settings → *Startup*, switch to **Manual launch** to unregister the Login Item. (Alternatively, remove it under System Settings → General → Login Items.)
3. Move **DonnyWhisper.app** from Applications to the Trash.
4. Optional — remove user data:
   ```bash
   rm -rf ~/Library/Application\ Support/donnywhisper
   rm -rf ~/Library/Caches/donnywhisper
   ```
5. Optional — revoke permissions under System Settings → Privacy & Security.

### Troubleshooting

**"DonnyWhisper can't be opened because Apple cannot check it for malicious software."**
This means you downloaded an unsigned development build (e.g., a CI artifact) instead of a notarized release. Either grab the official DMG from [GitHub Releases](https://github.com/mahype/DonnyWhisper/releases), or right-click the app → *Open* → *Open* to bypass Gatekeeper for unsigned builds at your own risk.

**Hotkey doesn't trigger recording.**
Most often a missing **Input Monitoring** permission. Open System Settings → Privacy & Security → Input Monitoring and confirm DonnyWhisper is listed and enabled. If it is and things still don't work, toggle it off and on again.

**Transcribed text doesn't appear in the target app.**
That's the **Accessibility** permission — the app needs it to simulate the paste shortcut. Same location in System Settings.

**App didn't start after login even though autostart is on.**
Check that DonnyWhisper is in `/Applications` or `~/Applications`. macOS's `SMAppService` API refuses to register Login Items for apps outside those locations.

**Recording fails with "Input device not found" after switching between workstations.**
DonnyWhisper now falls back to the next-best mic from your selection history when the configured device disappears, and to the system default if no preferred mic is plugged in. If you've never picked the desired mic explicitly, open Settings → Recording and select it once so it becomes part of the preference history.

---

## Windows

Coming soon. The Rust bridge already compiles on Windows; a native UI shell is on the roadmap.

## Linux

Coming soon. Same story as Windows — the core is ready, the UI is the gating piece.
