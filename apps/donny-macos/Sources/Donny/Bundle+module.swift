import Foundation

extension Foundation.Bundle {
    /// Resources (the `*.lproj/Localizable.strings` localization tables) are
    /// copied into the app bundle's `Contents/Resources` at packaging time by
    /// scripts/build-macos-app.sh, so they resolve through the main bundle.
    ///
    /// We define `module` ourselves instead of declaring `resources:` in
    /// Package.swift because SwiftPM's auto-generated `Bundle.module` accessor
    /// for an executable target looks for its resource bundle at
    /// `Bundle.main.bundleURL/<name>.bundle` — i.e. the `.app` ROOT — and at the
    /// absolute build-machine path. The `.app` root is not a codesign-able
    /// location, and the build path does not exist on a user's machine, so the
    /// generated accessor `fatalError`s at the first localized-string lookup on
    /// any machine other than the one that built it. Routing through
    /// `Bundle.main` keeps every existing `Text(_, bundle: .module)` and `L()`
    /// call site working while loading from a location that signs and ships.
    static let module: Bundle = .main
}
