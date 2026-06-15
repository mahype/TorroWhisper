import AVFoundation
import SwiftUI

/// Plugins overview (#15): lists the available plugins from the Rust catalog,
/// each with an enable toggle (persisted in `AppSettings.plugins`) and — for
/// configurable plugins — a button that opens the plugin's config dialog.
struct PluginsListView: View {
    @ObservedObject var model: AppModel
    /// Called with the plugin id when its "Configure…" button is tapped.
    var onConfigure: (String) -> Void
    @Environment(\.locale) private var locale

    private let bridge = BridgeClient()
    @State private var catalog: [PluginDescriptorDTO] = []

    var body: some View {
        Group {
            if catalog.isEmpty {
                Section {
                    Text("No plugins available.", bundle: .module)
                        .foregroundStyle(.secondary)
                }
            } else {
                ForEach(catalog) { plugin in
                    Section {
                        Toggle(isOn: model.pluginEnabledBinding(id: plugin.id)) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(plugin.name)
                                Text(plugin.description)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }

                        if plugin.configurable {
                            Button {
                                onConfigure(plugin.id)
                            } label: {
                                Label {
                                    Text("Configure…", bundle: .module)
                                } icon: {
                                    Image(systemName: "gearshape")
                                }
                            }
                            .disabled(!(model.pluginEnabledBinding(id: plugin.id).wrappedValue))
                        }
                    } header: {
                        Text("\(plugin.name) · v\(plugin.version)")
                    }
                }
            }
        }
        .onAppear { catalog = (try? bridge.getPluginCatalog()) ?? [] }
    }
}

/// Configuration dialog for the chat plugin (#17): default model, system prompt,
/// and how answers are spoken (provider + voice + rate). Edits write straight to
/// `AppSettings.chat` through the normal autosave flow.
struct ChatSettingsSheet: View {
    @ObservedObject var model: AppModel
    var onClose: () -> Void
    @Environment(\.locale) private var locale

    private let bridge = BridgeClient()
    @State private var registry: [LlmRegistryEntryDTO] = []
    @State private var hotkeyCapturing = false
    @State private var hotkeyPreview = ""
    @State private var hotkeyError: String?
    /// Per-agent token entry buffers + stored-state (Hermes agents, #agent).
    @State private var hermesKeyInputs: [String: String] = [:]
    @State private var hermesKeyStored: [String: Bool] = [:]
    @State private var hermesStatusLine = ""
    /// Per-agent "Test connection" in-flight set + last result.
    @State private var hermesTesting: Set<String> = []
    @State private var hermesTestResult: [String: HermesTestState] = [:]
    /// Local Piper TTS: available voice ids, selected language, download state.
    @State private var piperVoices: [String] = []
    @State private var piperLanguage = "de_DE"
    @State private var piperReady = false
    @State private var piperPreparing = false
    @State private var piperStatus = ""

    /// Models offered as the chat default: only those the user enabled app-wide
    /// in Language models (an empty curation means "show all", so a fresh install
    /// isn't empty). The currently saved default is always kept present so the
    /// picker never renders blank. Ready-to-run models sort first.
    private var enabledRegistry: [LlmRegistryEntryDTO] {
        let enabled = registry.filter { $0.enabled }
        var pool = enabled.isEmpty ? registry : enabled
        if let current = model.settings.chat.defaultModelRef,
           !pool.contains(where: { $0.modelRef == current }),
           let entry = registry.first(where: { $0.modelRef == current }) {
            pool.append(entry)
        }
        return pool.filter { $0.availability == .ready }
            + pool.filter { $0.availability != .ready }
    }

    /// Short reason a model is not ready, appended to its picker row. `nil` for
    /// ready models (shown by name alone).
    private func availabilityNote(_ availability: LlmAvailability) -> String? {
        switch availability {
        case .ready: return nil
        case .downloadable: return L("not downloaded", locale: locale)
        case .downloading: return L("downloading…", locale: locale)
        case .corrupt: return L("file damaged", locale: locale)
        case .needsApiKey: return L("needs API key", locale: locale)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Chat settings", bundle: .module)
                    .font(.headline)
                Spacer()
                Button { onClose() } label: {
                    Text("Done", bundle: .module)
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding()

            Divider()

            Form {
                shortcutSection
                modelSection
                agentsSection
                voiceSection
                promptSection
            }
            .formStyle(.grouped)
        }
        .frame(width: 540, height: 680)
        .onAppear {
            registry = (try? bridge.getLlmRegistry()) ?? []
            refreshHermesKeyStatus()
            loadPiperVoices()
        }
    }

    private var shortcutSection: some View {
        Section {
            HotkeyRecorderField(
                title: L("Chat shortcut", locale: locale),
                currentHotkey: model.settings.chat.chatHotkey,
                isCapturing: hotkeyCapturing,
                preview: hotkeyPreview,
                errorText: hotkeyError,
                warningText: nil,
                warningDetails: nil,
                onStartCapture: {
                    hotkeyCapturing = true
                    hotkeyError = nil
                    hotkeyPreview = ""
                },
                onCommit: { hotkey in
                    hotkeyCapturing = false
                    hotkeyPreview = ""
                    // Same combo as dictation would clobber that registration —
                    // reject with feedback instead of silently failing.
                    guard hotkey != model.settings.hotkey else {
                        hotkeyError = L("This shortcut is already used for dictation.", locale: locale)
                        return
                    }
                    hotkeyError = nil
                    model.settings.chat.chatHotkey = hotkey
                    model.requestAutoSave()
                },
                onCancel: {
                    hotkeyCapturing = false
                    hotkeyPreview = ""
                },
                onClear: {
                    model.settings.chat.chatHotkey = ""
                    model.requestAutoSave()
                    hotkeyCapturing = false
                    hotkeyPreview = ""
                },
                onPreview: { hotkeyPreview = $0 },
                onInvalid: { hotkeyError = $0 }
            )
            Text("Opens the chat window from anywhere. Separate from the dictation hotkey.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        } header: {
            Text("Shortcut", bundle: .module)
        }
    }

    private var modelSection: some View {
        Section {
            Picker(selection: model.binding(for: \.chat.defaultModelRef)) {
                Text("Local default", bundle: .module).tag(LlmModelRefDTO?.none)
                ForEach(enabledRegistry) { entry in
                    let note = availabilityNote(entry.availability)
                    Text(note.map { "\(entry.displayName) — \($0)" } ?? entry.displayName)
                        .tag(LlmModelRefDTO?.some(entry.modelRef))
                }
            } label: {
                Text("Default model", bundle: .module)
            }
            Text("Used for new conversations. A pick in the chat window overrides it for that session. Local models, your custom models and cloud models all appear here — entries marked “not downloaded” or “needs API key” aren’t usable until set up under Language models.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        } header: {
            Text("Model", bundle: .module)
        }
    }

    private var voiceSection: some View {
        Section {
            Picker(selection: piperLanguageBinding) {
                ForEach(piperLanguages, id: \.self) { lang in
                    Text(languageLabel(lang)).tag(lang)
                }
            } label: {
                Text("Language", bundle: .module)
            }

            Picker(selection: model.binding(for: \.chat.tts.piperVoice)) {
                ForEach(voicesForSelectedLanguage, id: \.self) { id in
                    Text(voiceLabel(id)).tag(id)
                }
            } label: {
                Text("Voice", bundle: .module)
            }
            .onChange(of: model.settings.chat.tts.piperVoice) { _, _ in refreshPiperReady() }

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
                Slider(value: model.binding(for: \.chat.tts.rate), in: 0...1)
            }
        } header: {
            Text("Speech output (local Piper)", bundle: .module)
        } footer: {
            Text("Answers are spoken by a fast, fully-local neural voice (Piper). Pick a language and voice; it downloads once (~25–115 MB). Until a voice is downloaded, the offline system voice is used as a fallback.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
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
                if !model.settings.chat.tts.piperVoice.hasPrefix(newLang + "-"),
                   let first = piperVoices.first(where: { $0.hasPrefix(newLang + "-") }) {
                    model.settings.chat.tts.piperVoice = first
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
        let current = model.settings.chat.tts.piperVoice
        piperLanguage = String(current.split(separator: "-").first ?? "de_DE")
        refreshPiperReady()
    }

    private func refreshPiperReady() {
        let voice = model.settings.chat.tts.piperVoice
        piperReady = (try? bridge.ttsLocalReady(voice: voice)) ?? false
        piperStatus = ""
    }

    /// Downloads the selected voice (+ shared CLI) off the main thread, with a
    /// spinner + inline status.
    private func preparePiper() {
        let voice = model.settings.chat.tts.piperVoice
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

    private var promptSection: some View {
        Section {
            TextField(
                L("System prompt", locale: locale),
                text: model.binding(for: \.chat.systemPrompt),
                prompt: Text("Leave empty for the built-in assistant prompt.", bundle: .module),
                axis: .vertical
            )
            .lineLimit(3...8)
        } header: {
            Text("Assistant", bundle: .module)
        }
    }

    // MARK: - Hermes agents (#agent)

    @ViewBuilder
    private var agentsSection: some View {
        Section {
            if model.settings.hermesAgents.isEmpty {
                Text("No Hermes agents added yet.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } header: {
            Text("Hermes agents", bundle: .module)
        } footer: {
            Text("Voice-chat with a Hermes Agent (NousResearch) over its API server. Enter the agent's address (e.g. http://localhost:8642/v1) and, if the server requires one, its API key. Agents appear in the chat window's model picker. Each conversation keeps its own memory via the agent.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }

        // One grouped card per agent, so each is clearly its own editable unit.
        ForEach(model.settings.hermesAgents) { agent in
            Section {
                agentFields(agent)
            } header: {
                Text(agent.name.isEmpty ? L("New agent", locale: locale) : agent.name)
            }
        }

        Section {
            Button {
                let id = model.addHermesAgent()
                hermesKeyInputs[id] = ""
                refreshHermesKeyStatus()
            } label: {
                Text("+ Add Hermes agent", bundle: .module)
            }
            if !hermesStatusLine.isEmpty {
                Text(hermesStatusLine)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    /// The editable fields for one agent. Each input is a bordered field with its
    /// label above it (the default Form row renders fields borderless + right
    /// aligned, which reads as static text); the API key sits on its own row.
    @ViewBuilder
    private func agentFields(_ agent: HermesAgent) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text("Name", bundle: .module).font(.caption).foregroundStyle(.secondary)
            TextField("Name", text: agentBinding(agent.id, \.name), prompt: Text(verbatim: "Hermes (Server)"))
                .textFieldStyle(.roundedBorder)
                .labelsHidden()
        }

        VStack(alignment: .leading, spacing: 3) {
            Text("Address", bundle: .module).font(.caption).foregroundStyle(.secondary)
            TextField("Address", text: agentBinding(agent.id, \.baseUrl), prompt: Text(verbatim: "http://localhost:8642/v1"))
                .textFieldStyle(.roundedBorder)
                .labelsHidden()
                .autocorrectionDisabled(true)
        }

        VStack(alignment: .leading, spacing: 3) {
            Text("Model id", bundle: .module).font(.caption).foregroundStyle(.secondary)
            TextField("Model id", text: agentBinding(agent.id, \.modelName), prompt: Text(verbatim: "hermes-agent"))
                .textFieldStyle(.roundedBorder)
                .labelsHidden()
                .autocorrectionDisabled(true)
        }

        VStack(alignment: .leading, spacing: 3) {
            Text("API key (optional)", bundle: .module).font(.caption).foregroundStyle(.secondary)
            HStack(spacing: 8) {
                SecureField(
                    "API key (optional)",
                    text: hermesKeyBinding(agent.id),
                    prompt: Text(verbatim: hermesKeyStored[agent.id] == true ? "••••••••" : "")
                )
                .textFieldStyle(.roundedBorder)
                .labelsHidden()
                Button {
                    saveHermesKey(agent.id)
                } label: {
                    Text("Save", bundle: .module)
                }
                .disabled((hermesKeyInputs[agent.id] ?? "").trimmingCharacters(in: .whitespaces).isEmpty)
                if hermesKeyStored[agent.id] == true {
                    Button(role: .destructive) {
                        deleteHermesKey(agent.id)
                    } label: {
                        Image(systemName: "trash")
                    }
                    .help(L("Remove stored key", locale: locale))
                }
            }
            if hermesKeyStored[agent.id] == true {
                Text("Key stored", bundle: .module)
                    .font(.caption2)
                    .foregroundStyle(.green)
            }
        }

        HStack(spacing: 8) {
            Button {
                testConnection(agent)
            } label: {
                Text("Test connection", bundle: .module)
            }
            .disabled(
                hermesTesting.contains(agent.id)
                    || agent.baseUrl.trimmingCharacters(in: .whitespaces).isEmpty
            )
            if hermesTesting.contains(agent.id) {
                ProgressView().controlSize(.small)
            }
            if let result = hermesTestResult[agent.id] {
                Label(
                    result.message,
                    systemImage: result.ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill"
                )
                .font(.caption)
                .foregroundStyle(result.ok ? Color.green : Color.orange)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            Button(role: .destructive) {
                removeAgent(agent.id)
            } label: {
                Text("Remove", bundle: .module)
            }
        }
    }

    private func agentBinding(
        _ id: String,
        _ keyPath: WritableKeyPath<HermesAgent, String>
    ) -> Binding<String> {
        Binding(
            get: { model.settings.hermesAgents.first(where: { $0.id == id })?[keyPath: keyPath] ?? "" },
            set: { newValue in
                if let index = model.settings.hermesAgents.firstIndex(where: { $0.id == id }) {
                    model.settings.hermesAgents[index][keyPath: keyPath] = newValue
                    model.requestAutoSave()
                }
            }
        )
    }

    private func hermesKeyBinding(_ id: String) -> Binding<String> {
        Binding(
            get: { hermesKeyInputs[id] ?? "" },
            set: { hermesKeyInputs[id] = $0 }
        )
    }

    private func refreshHermesKeyStatus() {
        let statuses = (try? bridge.getHermesApiKeyStatus()) ?? []
        hermesKeyStored = Dictionary(uniqueKeysWithValues: statuses.map { ($0.id, $0.hasKey) })
    }

    private func saveHermesKey(_ id: String) {
        let key = (hermesKeyInputs[id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !key.isEmpty else { return }
        hermesStatusLine = (try? bridge.setHermesApiKey(id: id, key: key))
            ?? L("Key could not be saved.", locale: locale)
        hermesKeyInputs[id] = ""
        refreshHermesKeyStatus()
    }

    private func deleteHermesKey(_ id: String) {
        _ = try? bridge.deleteHermesApiKey(id: id)
        hermesKeyInputs[id] = ""
        refreshHermesKeyStatus()
    }

    private func removeAgent(_ id: String) {
        model.removeHermesAgent(id: id)
        hermesKeyInputs[id] = nil
        hermesTestResult[id] = nil
        refreshHermesKeyStatus()
    }

    /// Tests the agent's address + stored token on a background thread (the call
    /// can block up to ~15s), then reports the result inline.
    private func testConnection(_ agent: HermesAgent) {
        let id = agent.id
        let baseUrl = agent.baseUrl
        hermesTesting.insert(id)
        hermesTestResult[id] = nil
        DispatchQueue.global(qos: .userInitiated).async {
            let result: HermesTestState
            do {
                result = HermesTestState(ok: true, message: try BridgeClient().testHermesAgent(id: id, baseUrl: baseUrl))
            } catch {
                result = HermesTestState(ok: false, message: error.localizedDescription)
            }
            DispatchQueue.main.async {
                hermesTesting.remove(id)
                hermesTestResult[id] = result
            }
        }
    }
}

/// Result of a Hermes "Test connection" attempt, shown inline in the agent row.
private struct HermesTestState {
    var ok: Bool
    var message: String
}
