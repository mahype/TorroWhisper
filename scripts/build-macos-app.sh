#!/usr/bin/env bash
# Builds a universal (arm64 + x86_64) release .app bundle at dist/Open Whisper.app.
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
echo "==> Building Open Whisper $VERSION"

# --- Clean debug artifact so the release linker can't accidentally pick it ---
# Package.swift hard-codes `-L ../../target/debug -lopen_whisper_bridge` for the
# dev loop; the release build overrides this with a `-Xlinker -L` that points at
# the universal static lib. Removing the debug .a here is a belt-and-braces
# safeguard so the release bundle can never link against a debug Rust lib.
rm -f target/debug/libopen_whisper_bridge.a

# --- Detect whether we can build universal (requires full Xcode for xcbuild) --

xcode_dev_path="$(xcode-select -p 2>/dev/null || true)"
if [[ "$xcode_dev_path" == *"Xcode.app"* ]]; then
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
    echo "==> Building Rust static library for aarch64-apple-darwin"
    cargo build --release --target aarch64-apple-darwin -p open-whisper-bridge

    echo "==> Building Rust static library for x86_64-apple-darwin"
    cargo build --release --target x86_64-apple-darwin -p open-whisper-bridge

    echo "==> Lipo'ing universal Rust static library"
    mkdir -p target/universal/release
    lipo -create \
        target/aarch64-apple-darwin/release/libopen_whisper_bridge.a \
        target/x86_64-apple-darwin/release/libopen_whisper_bridge.a \
        -output target/universal/release/libopen_whisper_bridge.a
    rust_lib_dir="$repo_root/target/universal/release"
else
    echo "==> Building Rust static library for $native_rust_target"
    cargo build --release --target "$native_rust_target" -p open-whisper-bridge
    rust_lib_dir="$repo_root/target/$native_rust_target/release"
fi
lipo -info "$rust_lib_dir/libopen_whisper_bridge.a"

# --- Swift executable --------------------------------------------------------

if $build_universal; then
    echo "==> Building universal Swift executable (arm64 + x86_64)"
    swift build \
        -c release \
        --arch arm64 --arch x86_64 \
        --package-path apps/open-whisper-macos \
        -Xlinker -L -Xlinker "$rust_lib_dir"
    swift_build_bin="apps/open-whisper-macos/.build/apple/Products/Release/OpenWhisperMac"
else
    echo "==> Building Swift executable ($native_arch only)"
    swift build \
        -c release \
        --package-path apps/open-whisper-macos \
        -Xlinker -L -Xlinker "$rust_lib_dir"
    swift_build_bin="apps/open-whisper-macos/.build/release/OpenWhisperMac"
fi

if [[ ! -f "$swift_build_bin" ]]; then
    echo "error: Swift build did not produce $swift_build_bin" >&2
    exit 1
fi
lipo -info "$swift_build_bin" || true

# --- Assemble .app bundle -----------------------------------------------------

app="dist/Open Whisper.app"
echo "==> Assembling $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$swift_build_bin" "$app/Contents/MacOS/OpenWhisperMac"
cp apps/open-whisper-macos/Resources/Info.plist "$app/Contents/Info.plist"

# --- Copy the SwiftPM resource bundle ----------------------------------------
# The OpenWhisperMac target declares `resources: [.process("Resources")]`, so
# SwiftPM emits `OpenWhisperMac_OpenWhisperMac.bundle` next to the executable and
# generates the `Bundle.module` accessor. That accessor calls `fatalError()` if
# the bundle is missing — and ALL in-app localized strings (L(), Text(_,
# bundle: .module)) go through it. Without this copy the app crashes the moment
# `applicationDidFinishLaunching` calls refreshMenuState(), so the menu-bar icon
# and onboarding wizard never appear (the mic prompt still fires earlier, from
# the Rust bridge during AppModel init). It must live at Contents/Resources so
# `Bundle.main.resourceURL` resolves it.
resource_bundle_src="$(dirname "$swift_build_bin")/OpenWhisperMac_OpenWhisperMac.bundle"
if [[ ! -d "$resource_bundle_src" ]]; then
    echo "error: SwiftPM resource bundle not found at $resource_bundle_src" >&2
    exit 1
fi
echo "==> Copying SwiftPM resource bundle (Bundle.module)"
cp -R "$resource_bundle_src" "$app/Contents/Resources/"

if [[ -f apps/open-whisper-macos/Resources/AppIcon.icns ]]; then
    cp apps/open-whisper-macos/Resources/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
fi

# --- Copy InfoPlist localizations into main bundle ---------------------------
# These are looked up by macOS for permission dialogs (NSMicrophoneUsageDescription,
# etc.) and must live at Contents/Resources/{lang}.lproj/InfoPlist.strings —
# SPM's module bundle cannot satisfy that lookup path.
for lproj_dir in apps/open-whisper-macos/Resources/Localizations/*.lproj; do
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

sparkle_framework_src="apps/open-whisper-macos/.build/artifacts/sparkle/Sparkle/Sparkle.xcframework/macos-arm64_x86_64/Sparkle.framework"
if [[ ! -d "$sparkle_framework_src" ]]; then
    echo "error: Sparkle.framework not found at $sparkle_framework_src" >&2
    echo "       run 'swift package --package-path apps/open-whisper-macos resolve' first" >&2
    exit 1
fi

echo "==> Embedding Sparkle.framework"
mkdir -p "$app/Contents/Frameworks"
rm -rf "$app/Contents/Frameworks/Sparkle.framework"
cp -R "$sparkle_framework_src" "$app/Contents/Frameworks/"

# The binary was linked with @rpath/Sparkle.framework/... but SwiftPM does
# not set @executable_path/../Frameworks as an rpath for executableTargets.
# Add it so dyld can find the embedded framework at runtime.
install_name_tool -add_rpath "@executable_path/../Frameworks" "$app/Contents/MacOS/OpenWhisperMac"

/usr/libexec/PlistBuddy \
    -c "Set :CFBundleShortVersionString $VERSION" \
    -c "Set :CFBundleVersion $VERSION" \
    "$app/Contents/Info.plist"

# --- Sign ---------------------------------------------------------------------

entitlements="apps/open-whisper-macos/Resources/OpenWhisper.entitlements"

if [[ -n "${MACOS_SIGN_IDENTITY:-}" ]]; then
    echo "==> Signing with \"$MACOS_SIGN_IDENTITY\" (hardened runtime)"
    codesign --force --deep --timestamp --options=runtime \
        --entitlements "$entitlements" \
        --sign "$MACOS_SIGN_IDENTITY" \
        "$app"
else
    echo "==> Ad-hoc signing (MACOS_SIGN_IDENTITY unset)"
    codesign --force --deep --sign - \
        --entitlements "$entitlements" \
        "$app"
fi

codesign --verify --deep --strict --verbose=2 "$app"

echo "==> Done: $app"
