import SwiftUI

/// Speech-output (TTS) configuration — the "Sprachausgabe" model role (#28).
/// Pulled out of the chat plugin into the Language models settings so it lives
/// next to the transcription and post-processing roles. Today this is the local
/// Piper backend; more providers arrive with #28 AP4.
struct SpeechOutputSettings: View {
    @ObservedObject var model: AppModel
    @Environment(\.locale) private var locale

    private let bridge = BridgeClient()
    /// Local Piper TTS: available voice ids, selected language, download state.
    @State private var piperVoices: [String] = []
    @State private var piperLanguage = "de_DE"
    @State private var piperReady = false
    @State private var piperPreparing = false
    @State private var piperStatus = ""

    var body: some View {
        Section {
            Picker(selection: piperLanguageBinding) {
                ForEach(piperLanguages, id: \.self) { lang in
                    Text(languageLabel(lang)).tag(lang)
                }
            } label: {
                Text("Language", bundle: .module)
            }

            Picker(selection: model.binding(for: \.speechOutput.piperVoice)) {
                ForEach(voicesForSelectedLanguage, id: \.self) { id in
                    Text(voiceLabel(id)).tag(id)
                }
            } label: {
                Text("Voice", bundle: .module)
            }
            .onChange(of: model.settings.speechOutput.piperVoice) { _, _ in refreshPiperReady() }

            HStack(spacing: 8) {
                if piperReady {
                    Label {
                        Text("Voice ready", bundle: .module)
                    } icon: {
                        Image(systemName: "checkmark.circle.fill")
                    }
                    .font(.caption)
                    .foregroundStyle(.green)
                } else {
                    Button {
                        preparePiper()
                    } label: {
                        Text("Download voice", bundle: .module)
                    }
                    .disabled(piperPreparing)
                    if piperPreparing { ProgressView().controlSize(.small) }
                }
                if !piperStatus.isEmpty {
                    Text(piperStatus)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }

            Toggle(isOn: model.binding(for: \.voicesDefaultLanguageOnly)) {
                Text("Only show voices of the default language", bundle: .module)
            }

            VStack(alignment: .leading) {
                Text("Speed", bundle: .module)
                Slider(value: model.binding(for: \.speechOutput.rate), in: 0...1)
            }
        } header: {
            Text("Speech output", bundle: .module)
        } footer: {
            Text("Answers are spoken by a fast, fully-local neural voice (Piper). Pick a language and voice; it downloads once (~25–115 MB). Until a voice is downloaded, the offline system voice is used as a fallback.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .onAppear { loadPiperVoices() }
    }

    // MARK: - Piper voice helpers

    /// Distinct languages (e.g. `de_DE`) from the available voice ids, original order.
    private var piperLanguages: [String] {
        var seen: [String] = []
        for id in piperVoices {
            let lang = String(id.split(separator: "-").first ?? "")
            if !lang.isEmpty, !seen.contains(lang) { seen.append(lang) }
        }
        return seen
    }

    private var voicesForSelectedLanguage: [String] {
        piperVoices.filter { $0.hasPrefix(piperLanguage + "-") }
    }

    private var piperLanguageBinding: Binding<String> {
        Binding(
            get: { piperLanguage },
            set: { newLang in
                piperLanguage = newLang
                // Move the selection to the first voice of the new language.
                if !model.settings.speechOutput.piperVoice.hasPrefix(newLang + "-"),
                   let first = piperVoices.first(where: { $0.hasPrefix(newLang + "-") }) {
                    model.settings.speechOutput.piperVoice = first
                    model.requestAutoSave()
                }
                refreshPiperReady()
            }
        )
    }

    /// `de_DE` → "Deutsch"; disambiguates English regions (US/UK).
    private func languageLabel(_ lang: String) -> String {
        let code = String(lang.prefix(2))
        let base = locale.localizedString(forLanguageCode: code)?.capitalized(with: locale) ?? lang
        let region = lang.split(separator: "_").count > 1 ? String(lang.split(separator: "_")[1]) : ""
        if code == "en", !region.isEmpty { return "\(base) (\(region))" }
        return base
    }

    /// `de_DE-thorsten_emotional-medium` → "Thorsten emotional — medium".
    private func voiceLabel(_ id: String) -> String {
        let parts = id.split(separator: "-")
        let voice = parts.count > 1 ? String(parts[1]).replacingOccurrences(of: "_", with: " ") : id
        let quality = parts.count > 2 ? qualityLabel(String(parts[2])) : ""
        return quality.isEmpty ? voice.capitalized : "\(voice.capitalized) — \(quality)"
    }

    private func qualityLabel(_ quality: String) -> String {
        switch quality {
        case "x_low": return L("very low", locale: locale)
        case "low": return L("low", locale: locale)
        case "medium": return L("medium", locale: locale)
        case "high": return L("high", locale: locale)
        default: return quality
        }
    }

    private func loadPiperVoices() {
        piperVoices = (try? bridge.ttsPiperVoices()) ?? []
        let current = model.settings.speechOutput.piperVoice
        piperLanguage = String(current.split(separator: "-").first ?? "de_DE")
        refreshPiperReady()
    }

    private func refreshPiperReady() {
        let voice = model.settings.speechOutput.piperVoice
        piperReady = (try? bridge.ttsLocalReady(voice: voice)) ?? false
        piperStatus = ""
    }

    /// Downloads the selected voice (+ shared CLI) off the main thread, with a
    /// spinner + inline status.
    private func preparePiper() {
        let voice = model.settings.speechOutput.piperVoice
        piperPreparing = true
        piperStatus = L("Downloading voice…", locale: locale)
        DispatchQueue.global(qos: .userInitiated).async {
            let ok: Bool
            let message: String
            do {
                _ = try BridgeClient().ttsLocalPrepare(voice: voice)
                ok = true
                message = ""
            } catch {
                ok = false
                message = (error as? BridgeError)?.message ?? error.localizedDescription
            }
            DispatchQueue.main.async {
                piperPreparing = false
                piperReady = ok
                piperStatus = message
            }
        }
    }
}
