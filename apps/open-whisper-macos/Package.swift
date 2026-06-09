// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "OpenWhisperMac",
    defaultLocalization: "en",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "OpenWhisperMac", targets: ["OpenWhisperMac"]),
    ],
    dependencies: [
        .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.6.0"),
    ],
    targets: [
        .systemLibrary(
            name: "OpenWhisperBridgeFFI",
            path: "Bridge"
        ),
        .executableTarget(
            name: "OpenWhisperMac",
            dependencies: [
                "OpenWhisperBridgeFFI",
                .product(name: "Sparkle", package: "Sparkle"),
            ],
            path: "Sources/OpenWhisperMac",
            // Exclude the localization tables from SwiftPM's resource handling.
            // Their mere presence makes SwiftPM synthesize a Bundle.module
            // accessor even without a `resources:` declaration; excluding them
            // suppresses that. The .strings are shipped into Contents/Resources
            // by scripts/build-macos-app.sh and resolved via Bundle.main.
            exclude: ["Resources"],
            // NOTE: deliberately no `resources:` here. SwiftPM's generated
            // Bundle.module accessor for an executable target hardcodes two
            // lookup paths: `Bundle.main.bundleURL/<bundle>` (the .app ROOT,
            // which codesign rejects — "unsealed contents present in the bundle
            // root") and the absolute build-machine path (which only exists on
            // the build host). Both fail on any other machine, crashing the app
            // at the first localized-string lookup. We instead ship the .lproj
            // files inside Contents/Resources and resolve them through
            // Bundle.main (see Bundle+module.swift and scripts/build-macos-app.sh).
            linkerSettings: [
                .unsafeFlags(["-L", "../../target/debug", "-lopen_whisper_bridge"]),
                .linkedLibrary("c++"),
                .linkedFramework("Accelerate"),
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("Carbon"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("IOKit"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
        .testTarget(
            name: "OpenWhisperMacTests",
            dependencies: ["OpenWhisperBridgeFFI"],
            path: "Tests/OpenWhisperMacTests",
            linkerSettings: [
                .unsafeFlags(["-L", "../../target/debug", "-lopen_whisper_bridge"]),
                .linkedLibrary("c++"),
                .linkedFramework("Accelerate"),
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("Carbon"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("IOKit"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
    ]
)
