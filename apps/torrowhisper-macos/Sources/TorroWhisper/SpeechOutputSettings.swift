import SwiftUI

/// Speech-output (TTS) configuration — the "Sprachausgabe" model role (#28).
/// Pulled out of the chat plugin into the Language models settings so it lives
/// next to the transcription and post-processing roles. Today this is the local
/// Piper backend; more providers arrive with #28 AP4.
struct SpeechOutputSettings: View {
    @ObservedObject var model: AppModel
    @Environment(\.locale) private var locale

    private let bridge = BridgeClient()
    /// Local Piper TTS: available voice ids + download state. The voice list is
    /// narrowed to the app-wide default language (`transcription_language`) when
    /// that filter is on.
    @State private var piperVoices: [String] = []
    @State private var piperReady = false
    @State private var piperPreparing = false
    @State private var piperStatus = ""

    var body: some View {
        Section {
            // Default language first, then the filter toggle, then the voice it
            // narrows — so the controls read top-to-bottom in the order they apply.
            Picker(selection: model.languageBinding()) {
                ForEach(model.availableLanguageOptions) { option in
                    Text(option.label(locale: locale)).tag(option.code)
                }
            } label: {
                Text("Default language", bundle: .module)
            }

            Toggle(isOn: model.binding(for: \.voicesDefaultLanguageOnly)) {
                Text("Only show voices of the default language", bundle: .module)
            }

            Picker(selection: model.binding(for: \.speechOutput.piperVoice)) {
                ForEach(voiceChoices, id: \.self) { id in
                    Text(voiceLabel(id)).tag(id)
                }
            } label: {
                Text("Default voice", bundle: .module)
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

            VStack(alignment: .leading) {
                Text("Speed", bundle: .module)
                Slider(value: model.binding(for: \.speechOutput.rate), in: 0...1)
            }
        } header: {
            Text("Speech output", bundle: .module)
        } footer: {
            Text("Answers are spoken by a fast, fully-local neural voice (Piper). It downloads once (~25–115 MB). Until a voice is downloaded, the offline system voice is used as a fallback.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .onAppear { loadPiperVoices() }
    }

    // MARK: - Piper voice helpers

    /// Voices to offer for the default voice picker: narrowed to the app-wide
    /// default language when the filter is on (never to empty), with the current
    /// pick always kept present.
    private var voiceChoices: [String] {
        var ids = piperVoices
        let lang = model.settings.transcriptionLanguage.lowercased()
        if model.settings.voicesDefaultLanguageOnly, lang != "auto", !lang.isEmpty {
            let filtered = ids.filter { Self.voiceLanguageCode($0) == lang }
            if !filtered.isEmpty { ids = filtered }
        }
        let current = model.settings.speechOutput.piperVoice
        if !current.isEmpty, !ids.contains(current) { ids.insert(current, at: 0) }
        return ids
    }

    /// ISO 639-1 language code of a Piper voice id: `de_DE-thorsten-high` → `de`.
    private static func voiceLanguageCode(_ id: String) -> String {
        let locale = id.split(separator: "-").first.map(String.init) ?? id
        return String(locale.split(separator: "_").first ?? Substring(locale)).lowercased()
    }

    /// `de_DE-thorsten_emotional-medium` → "Thorsten emotional — medium · DE".
    private func voiceLabel(_ id: String) -> String {
        let parts = id.split(separator: "-")
        let voice = parts.count > 1 ? String(parts[1]).replacingOccurrences(of: "_", with: " ") : id
        let quality = parts.count > 2 ? qualityLabel(String(parts[2])) : ""
        let country = parts.first?.split(separator: "_").last.map { String($0).uppercased() } ?? ""
        let base = quality.isEmpty ? voice.capitalized : "\(voice.capitalized) — \(quality)"
        return country.isEmpty ? base : "\(base) · \(country)"
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
