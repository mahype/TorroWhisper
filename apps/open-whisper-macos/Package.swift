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
            resources: [
                .process("Resources"),
            ],
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
