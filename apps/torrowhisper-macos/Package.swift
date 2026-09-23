// swift-tools-version: 6.0
import Foundation
import PackageDescription

let skipNativeBridge = ProcessInfo.processInfo.environment["TORROWHISPER_SKIP_NATIVE_BRIDGE"] == "1"
let nativeBridgeLinkerSettings: [LinkerSetting] =
    skipNativeBridge
    ? []
    : [.unsafeFlags(["-L", "../../target/debug", "-ltorrowhisper_bridge"])]

let package = Package(
    name: "TorroWhisper",
    defaultLocalization: "en",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "TorroWhisper", targets: ["TorroWhisper"])
    ],
    dependencies: [
        .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.6.0")
    ],
    targets: [
        .systemLibrary(
            name: "TorroWhisperBridgeFFI",
            path: "Bridge"
        ),
        .executableTarget(
            name: "TorroWhisper",
            dependencies: [
                "TorroWhisperBridgeFFI",
                .product(name: "Sparkle", package: "Sparkle"),
            ],
            path: "Sources/TorroWhisper",
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
            linkerSettings: nativeBridgeLinkerSettings + [
                .linkedLibrary("c++"),
                .linkedFramework("Accelerate"),
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("Carbon"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("CoreML"),
                .linkedFramework("IOKit"),
                .linkedFramework("Metal"),
                .linkedFramework("MetalPerformanceShaders"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
        .testTarget(
            name: "TorroWhisperTests",
            dependencies: ["TorroWhisper", "TorroWhisperBridgeFFI"],
            path: "Tests/TorroWhisperTests",
            linkerSettings: nativeBridgeLinkerSettings + [
                .linkedLibrary("c++"),
                .linkedFramework("Accelerate"),
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("Carbon"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("CoreML"),
                .linkedFramework("IOKit"),
                .linkedFramework("Metal"),
                .linkedFramework("MetalPerformanceShaders"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
    ]
)
