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
    @State private var openAiKeyPresent = false
    @State private var hotkeyCapturing = false
    @State private var hotkeyPreview = ""
    @State private var hotkeyError: String?
    /// Per-agent token entry buffers + stored-state (Hermes agents, #agent).
    @State private var hermesKeyInputs: [String: String] = [:]
    @State private var hermesKeyStored: [String: Bool] = [:]
    @State private var hermesStatusLine = ""

    /// OpenAI's published TTS voices.
    private let openAiVoices = [
        "alloy", "ash", "ballad", "coral", "echo", "fable", "nova", "onyx", "sage", "shimmer",
    ]

    private var systemVoices: [AVSpeechSynthesisVoice] {
        AVSpeechSynthesisVoice.speechVoices()
            .sorted { ($0.language, $0.name) < ($1.language, $1.name) }
    }

    /// Ready-to-run models first, the rest after — so the usable picks sit at
    /// the top. Order within each group is preserved (presets, custom, cloud).
    private var sortedRegistry: [LlmRegistryEntryDTO] {
        registry.filter { $0.availability == .ready }
            + registry.filter { $0.availability != .ready }
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
            openAiKeyPresent = (try? bridge.getLlmApiKeyStatus())?
                .first(where: { $0.backend == .openAi })?.hasKey ?? false
            refreshHermesKeyStatus()
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
                ForEach(sortedRegistry) { entry in
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
            Picker(selection: model.binding(for: \.chat.tts.provider)) {
                ForEach(ChatTtsProvider.allCases) { provider in
                    Text(provider.label(locale: locale)).tag(provider)
                }
            } label: {
                Text("Voice provider", bundle: .module)
            }

            switch model.settings.chat.tts.provider {
            case .system:
                Picker(selection: model.binding(for: \.chat.tts.systemVoice)) {
                    Text("System default", bundle: .module).tag("")
                    ForEach(systemVoices, id: \.identifier) { voice in
                        Text("\(voice.name) (\(voice.language))").tag(voice.identifier)
                    }
                } label: {
                    Text("Voice", bundle: .module)
                }
            case .openAi:
                Picker(selection: model.binding(for: \.chat.tts.openaiVoice)) {
                    ForEach(openAiVoices, id: \.self) { voice in
                        Text(voice.capitalized).tag(voice)
                    }
                } label: {
                    Text("Voice", bundle: .module)
                }
                if openAiKeyPresent {
                    Text("Uses your OpenAI API key from Cloud models.", bundle: .module)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                } else {
                    Label {
                        Text("No OpenAI API key — speaking with the system voice instead. Add a key under Language models → Cloud models, or pick the System voice provider above.", bundle: .module)
                    } icon: {
                        Image(systemName: "exclamationmark.triangle.fill")
                    }
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
                }
            }

            VStack(alignment: .leading) {
                Text("Speed", bundle: .module)
                Slider(value: model.binding(for: \.chat.tts.rate), in: 0...1)
            }
        } header: {
            Text("Speech output", bundle: .module)
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

    private var agentsSection: some View {
        Section {
            if model.settings.hermesAgents.isEmpty {
                Text("No Hermes agents added yet.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(model.settings.hermesAgents) { agent in
                    agentRow(agent)
                }
            }

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
        } header: {
            Text("Hermes agents", bundle: .module)
        } footer: {
            Text("Voice-chat with a Hermes Agent (NousResearch) over its API server. Enter the agent's address (e.g. http://localhost:8642/v1) and, if the server requires one, its API key. Agents appear in the chat window's model picker. Each conversation keeps its own memory via the agent.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    @ViewBuilder
    private func agentRow(_ agent: HermesAgent) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            TextField(text: agentBinding(agent.id, \.name)) {
                Text("Name", bundle: .module)
            }
            TextField(text: agentBinding(agent.id, \.baseUrl), prompt: Text(verbatim: "http://localhost:8642/v1")) {
                Text("Address", bundle: .module)
            }
            TextField(text: agentBinding(agent.id, \.modelName), prompt: Text(verbatim: "hermes-agent")) {
                Text("Model id", bundle: .module)
            }

            HStack(spacing: 8) {
                SecureField(
                    hermesKeyStored[agent.id] == true ? "••••••••" : L("API key (optional)", locale: locale),
                    text: hermesKeyBinding(agent.id)
                )
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

                Spacer()

                Button(role: .destructive) {
                    removeAgent(agent.id)
                } label: {
                    Text("Remove", bundle: .module)
                }
            }

            if hermesKeyStored[agent.id] == true {
                Text("Key stored", bundle: .module)
                    .font(.caption2)
                    .foregroundStyle(.green)
            }
        }
        .padding(.vertical, 4)
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
        refreshHermesKeyStatus()
    }
}
