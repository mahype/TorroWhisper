import Foundation
import SwiftUI

extension UiLanguage {
    var displayLabel: String {
        switch self {
        case .system: return String(localized: "System", bundle: .module)
        case .en: return "English"
        case .de: return "Deutsch"
        }
    }

    var explicitLocale: Locale? {
        switch self {
        case .system: return nil
        case .en: return Locale(identifier: "en")
        case .de: return Locale(identifier: "de")
        }
    }
}

extension AppSettings {
    var effectiveLocale: Locale {
        if let explicit = uiLanguage.explicitLocale {
            return explicit
        }
        // .system — derive from the user's preferred languages directly.
        // `Locale.current` is filtered through the app's own bundle
        // localizations and falls back to the development region (en) when the
        // app isn't recognized as German-localized, so it reports English even
        // though the system language is German. `Locale.preferredLanguages`
        // reflects the real system setting.
        if let preferred = Locale.preferredLanguages.first {
            return Locale(identifier: preferred)
        }
        return Locale.current
    }
}

func L(_ key: String, locale: Locale? = nil) -> String {
    let effective = locale ?? .current
    let bundle = localeBundle(for: effective)
    return bundle.localizedString(forKey: key, value: key, table: nil)
}

private func localeBundle(for locale: Locale) -> Bundle {
    let requested = locale.language.languageCode?.identifier ?? "en"
    let available = Bundle.module.localizations
    let resolved = available.contains(requested)
        ? requested
        : (Bundle.module.developmentLocalization ?? "en")
    if let path = Bundle.module.path(forResource: resolved, ofType: "lproj"),
       let bundle = Bundle(path: path) {
        return bundle
    }
    return Bundle.module
}

struct LocalizedRoot<Content: View>: View {
    @ObservedObject var model: AppModel
    @ViewBuilder var content: () -> Content

    var body: some View {
        content()
            .environment(\.locale, model.settings.effectiveLocale)
            // Every window goes through here, so this is the one place that has
            // to carry the brand accent — it tints the controls that read the
            // system accent on their own (prominent buttons, toggles, pickers).
            .tint(.torroAccent)
    }
}
