import SwiftUI

/// Cloud-model picker + API-key entry, driven by the unified model registry
/// (`ow_get_llm_registry`) and the Keychain-backed key endpoints. Lets the user
/// store per-provider API keys and select any registry model (local or cloud)
/// as the post-processing model.
struct CloudModelsSheet: View {
    @ObservedObject var model: AppModel
    var onClose: () -> Void
    @Environment(\.locale) private var locale

    private let bridge = BridgeClient()
    @State private var registry: [LlmRegistryEntryDTO] = []
    @State private var keyStored: [LlmBackendKind: Bool] = [:]
    @State private var keyInputs: [LlmBackendKind: String] = [:]
    @State private var statusLine: String = ""

    /// Cloud backends that have an API-key slot, in display order.
    private let cloudBackends: [LlmBackendKind] = [
        .anthropic, .openAi, .mistral, .deepSeek, .grok, .gemini,
    ]

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Cloud models & API keys", bundle: .module)
                    .font(.headline)
                Spacer()
                Button {
                    onClose()
                } label: {
                    Text("Done", bundle: .module)
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding()

            Divider()

            Form {
                modelSection
                apiKeysSection
                if !statusLine.isEmpty {
                    Section {
                        Text(statusLine)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)
        }
        .frame(width: 540, height: 600)
        .onAppear(perform: reload)
    }

    /// Models offered for post-processing: the app-wide enabled set the user
    /// curated in the language-models manager (empty = show all, so this is never
    /// empty on a fresh install).
    private var selectableRegistry: [LlmRegistryEntryDTO] {
        let enabled = registry.filter { $0.enabled }
        return enabled.isEmpty ? registry : enabled
    }

    private var modelSection: some View {
        Section {
            Picker(selection: selectionBinding) {
                Text("Default (local / legacy)", bundle: .module)
                    .tag(String?.none)
                ForEach(selectableRegistry) { entry in
                    Text(entryLabel(entry)).tag(String?.some(entry.stableId))
                }
            } label: {
                Text("Post-processing model", bundle: .module)
            }

            Text(
                "Pick a post-processing model from the ones enabled in the language-models manager. Already-downloaded local models are reused; cloud models need an API key below.",
                bundle: .module
            )
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        } header: {
            Text("Model", bundle: .module)
        }
    }

    private var apiKeysSection: some View {
        Section {
            ForEach(cloudBackends, id: \.self) { backend in
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 8) {
                        Text(backend.displayName)
                            .frame(width: 100, alignment: .leading)
                        SecureField(
                            keyStored[backend] == true ? "••••••••" : "API key",
                            text: keyBinding(backend)
                        )
                        Button {
                            saveKey(backend)
                        } label: {
                            Text("Save", bundle: .module)
                        }
                        .disabled((keyInputs[backend] ?? "").trimmingCharacters(in: .whitespaces).isEmpty)
                        if keyStored[backend] == true {
                            Button(role: .destructive) {
                                deleteKey(backend)
                            } label: {
                                Image(systemName: "trash")
                            }
                            .help(L("Remove stored key", locale: locale))
                        }
                    }
                    if keyStored[backend] == true {
                        Text("Key stored", bundle: .module)
                            .font(.caption2)
                            .foregroundStyle(.green)
                    }
                }
            }
        } header: {
            Text("API keys (stored in macOS Keychain)", bundle: .module)
        }
    }

    // MARK: - Bindings

    private var selectionBinding: Binding<String?> {
        Binding(
            get: { stableId(of: model.settings.activePostProcessingModel) },
            set: { newID in
                model.settings.activePostProcessingModel =
                    registry.first(where: { $0.stableId == newID })?.modelRef
                _ = model.saveSettings()
            }
        )
    }

    private func keyBinding(_ backend: LlmBackendKind) -> Binding<String> {
        Binding(
            get: { keyInputs[backend] ?? "" },
            set: { keyInputs[backend] = $0 }
        )
    }

    // MARK: - Helpers

    private func entryLabel(_ entry: LlmRegistryEntryDTO) -> String {
        let suffix: String
        switch entry.availability {
        case .ready: suffix = ""
        case .needsApiKey: suffix = " · " + L("needs key", locale: locale)
        case .downloadable: suffix = " · " + L("not downloaded", locale: locale)
        case .downloading: suffix = " · " + L("downloading…", locale: locale)
        case .corrupt: suffix = " · " + L("damaged", locale: locale)
        }
        return "\(entry.displayName)\(suffix)"
    }

    private func stableId(of ref: LlmModelRefDTO?) -> String? {
        guard let ref else { return nil }
        return registry.first(where: { $0.modelRef == ref })?.stableId
    }

    private func reload() {
        registry = (try? bridge.getLlmRegistry()) ?? []
        refreshKeyStatus()
    }

    private func refreshKeyStatus() {
        let statuses = (try? bridge.getLlmApiKeyStatus()) ?? []
        keyStored = Dictionary(uniqueKeysWithValues: statuses.map { ($0.backend, $0.hasKey) })
    }

    private func saveKey(_ backend: LlmBackendKind) {
        let key = (keyInputs[backend] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !key.isEmpty else { return }
        statusLine = (try? bridge.setLlmApiKey(backend: backend, key: key))
            ?? L("Key could not be saved.", locale: locale)
        keyInputs[backend] = ""
        refreshKeyStatus()
        reload()
    }

    private func deleteKey(_ backend: LlmBackendKind) {
        _ = try? bridge.deleteLlmApiKey(backend: backend)
        keyInputs[backend] = ""
        refreshKeyStatus()
        reload()
    }
}
