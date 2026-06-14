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

    /// OpenAI's published TTS voices.
    private let openAiVoices = [
        "alloy", "ash", "ballad", "coral", "echo", "fable", "nova", "onyx", "sage", "shimmer",
    ]

    private var systemVoices: [AVSpeechSynthesisVoice] {
        AVSpeechSynthesisVoice.speechVoices()
            .sorted { ($0.language, $0.name) < ($1.language, $1.name) }
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
                modelSection
                voiceSection
                promptSection
            }
            .formStyle(.grouped)
        }
        .frame(width: 540, height: 620)
        .onAppear { registry = (try? bridge.getLlmRegistry()) ?? [] }
    }

    private var modelSection: some View {
        Section {
            Picker(selection: model.binding(for: \.chat.defaultModelRef)) {
                Text("Local default", bundle: .module).tag(LlmModelRefDTO?.none)
                ForEach(registry) { entry in
                    Text(entry.displayName).tag(LlmModelRefDTO?.some(entry.modelRef))
                }
            } label: {
                Text("Default model", bundle: .module)
            }
            Text("Used for new conversations. A pick in the chat window overrides it for that session.", bundle: .module)
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
                Text("Uses your OpenAI API key (set it under Language models → Cloud models). Falls back to the system voice if no key is stored.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
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
}
