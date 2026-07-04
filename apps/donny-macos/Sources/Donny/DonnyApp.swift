import SwiftUI

@main
struct DonnyApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    init() {
        applyPersistedUiLanguage()
    }

    var body: some Scene {
        Settings {
            EmptyView()
        }
    }
}

private func applyPersistedUiLanguage() {
    let bridge = BridgeClient()
    guard let settings = try? bridge.loadSettings() else { return }
    switch settings.uiLanguage {
    case .system:
        UserDefaults.standard.removeObject(forKey: "AppleLanguages")
    case .en:
        UserDefaults.standard.set(["en"], forKey: "AppleLanguages")
    case .de:
        UserDefaults.standard.set(["de"], forKey: "AppleLanguages")
    }
}
