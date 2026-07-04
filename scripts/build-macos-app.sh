#!/usr/bin/env bash
# Builds a universal (arm64 + x86_64) release .app bundle at dist/DonnyWhisper.app.
#
# By default signs the bundle ad-hoc — good enough to run on the local machine.
# For a signed + notarized release, chain this with scripts/codesign-macos.sh
# and scripts/build-dmg.sh; see docs/RELEASING.md.
#
# Environment:
#   VERSION              Overrides version derived from `git describe`.
#                        Defaults to `git describe --tags --always --dirty` with
#                        the leading `v` stripped. When not in a git checkout, falls
#                        back to the Cargo.toml workspace version.

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

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
echo "==> Building DonnyWhisper $VERSION"

# --- Clean debug artifact so the release linker can't accidentally pick it ---
# Package.swift hard-codes `-L ../../target/debug -ldonnywhisper_bridge` for the
# dev loop; the release build overrides this with a `-Xlinker -L` that points at
# the universal static lib. Removing the debug .a here is a belt-and-braces
# safeguard so the release bundle can never link against a debug Rust lib.
rm -f target/debug/libdonnywhisper_bridge.a

# --- Detect whether we can build universal (requires full Xcode for xcbuild) --

xcode_dev_path="$(xcode-select -p 2>/dev/null || true)"
# A full Xcode developer dir ends in `.app/Contents/Developer` (e.g.
# /Applications/Xcode.app/... OR the versioned /Applications/Xcode_16.2.app/...
# that GitHub-hosted runners use). Command Line Tools live at
# /Library/Developer/CommandLineTools and cannot build universal binaries.
# The previous `*"Xcode.app"*` substring match silently failed on the runner's
# versioned path, so every CI release shipped an arm64-ONLY binary that would
# not launch on Intel Macs. Match the `.app/Contents/Developer` suffix instead.
if [[ "$xcode_dev_path" == *.app/Contents/Developer ]]; then
    build_universal=true
else
    build_universal=false
    echo "==> NOTE: Command Line Tools detected (no full Xcode at $xcode_dev_path)."
    echo "         Building native-architecture only."
    echo "         For a universal release artifact, install Xcode from the App Store"
    echo "         and run \`sudo xcode-select -s /Applications/Xcode.app\`."
fi

native_arch="$(uname -m)"
case "$native_arch" in
    arm64)   native_rust_target="aarch64-apple-darwin" ;;
    x86_64)  native_rust_target="x86_64-apple-darwin" ;;
    *)       echo "error: unsupported host architecture $native_arch" >&2; exit 1 ;;
esac

# --- Rust static library -----------------------------------------------------

if $build_universal; then
    echo "==> Building Rust static library + LLM helper for aarch64-apple-darwin"
    cargo build --release --target aarch64-apple-darwin -p donnywhisper-bridge -p donnywhisper-llm-helper

    echo "==> Building Rust static library + LLM helper for x86_64-apple-darwin"
    cargo build --release --target x86_64-apple-darwin -p donnywhisper-bridge -p donnywhisper-llm-helper

    echo "==> Lipo'ing universal Rust static library"
    mkdir -p target/universal/release
    lipo -create \
        target/aarch64-apple-darwin/release/libdonnywhisper_bridge.a \
        target/x86_64-apple-darwin/release/libdonnywhisper_bridge.a \
        -output target/universal/release/libdonnywhisper_bridge.a

    echo "==> Lipo'ing universal LLM helper"
    lipo -create \
        target/aarch64-apple-darwin/release/donnywhisper-llm-helper \
        target/x86_64-apple-darwin/release/donnywhisper-llm-helper \
        -output target/universal/release/donnywhisper-llm-helper
    rust_lib_dir="$repo_root/target/universal/release"
else
    echo "==> Building Rust static library + LLM helper for $native_rust_target"
    cargo build --release --target "$native_rust_target" -p donnywhisper-bridge -p donnywhisper-llm-helper
    rust_lib_dir="$repo_root/target/$native_rust_target/release"
fi
lipo -info "$rust_lib_dir/libdonnywhisper_bridge.a"
lipo -info "$rust_lib_dir/donnywhisper-llm-helper"

# --- Force the Swift executable to re-link against the fresh Rust lib ---------
# The Rust static library is pulled in via raw `-Xlinker -L` flags, so SwiftPM
# does NOT track it as a build input. When only the Rust side changes, `swift
# build` sees the Swift sources unchanged and skips re-linking — shipping a
# binary built against a STALE libdonnywhisper_bridge.a (e.g. missing newly
# added settings fields). Delete the linked executables so SwiftPM must relink.
rm -f \
    "apps/donnywhisper-macos/.build/release/DonnyWhisper" \
    "apps/donnywhisper-macos/.build/apple/Products/Release/DonnyWhisper" \
    2>/dev/null || true

# --- Swift executable --------------------------------------------------------

if $build_universal; then
    echo "==> Building universal Swift executable (arm64 + x86_64)"
    swift build \
        -c release \
        --arch arm64 --arch x86_64 \
        --package-path apps/donnywhisper-macos \
        -Xlinker -L -Xlinker "$rust_lib_dir"
    swift_build_bin="apps/donnywhisper-macos/.build/apple/Products/Release/DonnyWhisper"
else
    echo "==> Building Swift executable ($native_arch only)"
    swift build \
        -c release \
        --package-path apps/donnywhisper-macos \
        -Xlinker -L -Xlinker "$rust_lib_dir"
    swift_build_bin="apps/donnywhisper-macos/.build/release/DonnyWhisper"
fi

if [[ ! -f "$swift_build_bin" ]]; then
    echo "error: Swift build did not produce $swift_build_bin" >&2
    exit 1
fi
lipo -info "$swift_build_bin" || true

# Fail loudly if a universal build was requested but the binary is not fat.
# This is the backstop for the bug where a misdetected toolchain shipped an
# arm64-only release that could not launch on Intel Macs.
if $build_universal; then
    archs="$(lipo -archs "$swift_build_bin" 2>/dev/null || true)"
    if [[ "$archs" != *arm64* || "$archs" != *x86_64* ]]; then
        echo "error: universal build requested but binary archs are '$archs'" >&2
        echo "       expected both arm64 and x86_64" >&2
        exit 1
    fi
    echo "==> Verified universal binary: $archs"
fi

# --- Assemble .app bundle -----------------------------------------------------

app="dist/DonnyWhisper.app"
echo "==> Assembling $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$swift_build_bin" "$app/Contents/MacOS/DonnyWhisper"
cp apps/donnywhisper-macos/Resources/Info.plist "$app/Contents/Info.plist"

# The local LLM runs in its own process (ggml symbol isolation from whisper);
# the bridge looks for the helper next to the main executable.
cp "$rust_lib_dir/donnywhisper-llm-helper" "$app/Contents/MacOS/donnywhisper-llm-helper"
chmod +x "$app/Contents/MacOS/donnywhisper-llm-helper"

if [[ -f apps/donnywhisper-macos/Resources/AppIcon.icns ]]; then
    cp apps/donnywhisper-macos/Resources/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
fi

# --- Copy in-app localization tables (Bundle.module) --------------------------
# The DonnyWhisper target does NOT declare `resources:` in Package.swift —
# SwiftPM's generated Bundle.module accessor for an executable target points at
# the .app ROOT (uncodesignable) and the build-machine path (absent on user
# machines), so it crashed every shipped release at the first localized-string
# lookup. Instead, Bundle.module is defined as Bundle.main (Bundle+module.swift)
# and we ship the Localizable.strings tables in Contents/Resources/<lang>.lproj
# where the main bundle resolves them. Keep these in sync with the .xcstrings
# editing source.
echo "==> Copying in-app localization tables into Contents/Resources"
copied_localizable=false
for lproj_dir in apps/donnywhisper-macos/Sources/DonnyWhisper/Resources/*.lproj; do
    if [[ -f "$lproj_dir/Localizable.strings" ]]; then
        lang_name="$(basename "$lproj_dir")"
        # A single bad escape (e.g. a stray ASCII quote in a value) makes macOS
        # silently fail to parse the WHOLE table, so every key falls back to the
        # base language. Fail the build instead of shipping a dead translation.
        if ! plutil -lint "$lproj_dir/Localizable.strings" >/dev/null 2>&1; then
            echo "error: $lproj_dir/Localizable.strings has a syntax error:" >&2
            plutil -lint "$lproj_dir/Localizable.strings" >&2 || true
            exit 1
        fi
        mkdir -p "$app/Contents/Resources/$lang_name"
        cp "$lproj_dir/Localizable.strings" "$app/Contents/Resources/$lang_name/Localizable.strings"
        copied_localizable=true
    fi
done
if ! $copied_localizable; then
    echo "error: no Localizable.strings found under Sources/DonnyWhisper/Resources/*.lproj" >&2
    exit 1
fi

# --- Copy InfoPlist localizations into main bundle ---------------------------
# These are looked up by macOS for permission dialogs (NSMicrophoneUsageDescription,
# etc.) and must live at Contents/Resources/{lang}.lproj/InfoPlist.strings.
for lproj_dir in apps/donnywhisper-macos/Resources/Localizations/*.lproj; do
    if [[ -d "$lproj_dir" ]]; then
        lang_name="$(basename "$lproj_dir")"
        mkdir -p "$app/Contents/Resources/$lang_name"
        cp -R "$lproj_dir"/* "$app/Contents/Resources/$lang_name/"
    fi
done

# --- Embed Sparkle.framework ------------------------------------------------
# The Swift executable links against Sparkle with an @rpath load command; the
# framework is not copied automatically by `swift build` into an app bundle.
# SwiftPM only resolves it into the XCFramework artifact tree. Copy the
# universal variant into Contents/Frameworks/ and add the conventional rpath.

sparkle_framework_src="apps/donnywhisper-macos/.build/artifacts/sparkle/Sparkle/Sparkle.xcframework/macos-arm64_x86_64/Sparkle.framework"
if [[ ! -d "$sparkle_framework_src" ]]; then
    echo "error: Sparkle.framework not found at $sparkle_framework_src" >&2
    echo "       run 'swift package --package-path apps/donnywhisper-macos resolve' first" >&2
    exit 1
fi

echo "==> Embedding Sparkle.framework"
mkdir -p "$app/Contents/Frameworks"
rm -rf "$app/Contents/Frameworks/Sparkle.framework"
cp -R "$sparkle_framework_src" "$app/Contents/Frameworks/"

# The binary was linked with @rpath/Sparkle.framework/... but SwiftPM does
# not set @executable_path/../Frameworks as an rpath for executableTargets.
# Add it so dyld can find the embedded framework at runtime.
install_name_tool -add_rpath "@executable_path/../Frameworks" "$app/Contents/MacOS/DonnyWhisper"

# Sparkle compares the appcast's sparkle:version against CFBundleVersion.
# `git describe` suffixes (e.g. 0.4.0-4-g1a06bd2[-dirty]) compare as NEWER than
# the released 0.4.0 — and even than 0.4.0 hotfix-less follow-ups — in Sparkle's
# standard comparator, so an installed dev build silently blocked every update
# until the next minor release. Keep the descriptive string for display, but
# strip the describe suffix for the version Sparkle compares.
BUNDLE_VERSION="$(printf '%s' "$VERSION" | sed -E 's/-[0-9]+-g[0-9a-f]+(-dirty)?$//; s/-dirty$//')"

/usr/libexec/PlistBuddy \
    -c "Set :CFBundleShortVersionString $VERSION" \
    -c "Set :CFBundleVersion $BUNDLE_VERSION" \
    "$app/Contents/Info.plist"

# --- Sign ---------------------------------------------------------------------

entitlements="apps/donnywhisper-macos/Resources/DonnyWhisper.entitlements"

# The helper is a second Mach-O in Contents/MacOS; `codesign --deep` does not
# reliably treat it as nested code, so sign it explicitly before the bundle.
if [[ -n "${MACOS_SIGN_IDENTITY:-}" ]]; then
    echo "==> Signing with \"$MACOS_SIGN_IDENTITY\" (hardened runtime)"
    codesign --force --timestamp --options=runtime \
        --sign "$MACOS_SIGN_IDENTITY" \
        "$app/Contents/MacOS/donnywhisper-llm-helper"
    codesign --force --deep --timestamp --options=runtime \
        --entitlements "$entitlements" \
        --sign "$MACOS_SIGN_IDENTITY" \
        "$app"
else
    echo "==> Ad-hoc signing (MACOS_SIGN_IDENTITY unset)"
    codesign --force --sign - "$app/Contents/MacOS/donnywhisper-llm-helper"
    codesign --force --deep --sign - \
        --entitlements "$entitlements" \
        "$app"
fi

codesign --verify --deep --strict --verbose=2 "$app"

echo "==> Done: $app"
