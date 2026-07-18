import AppKit
import SwiftUI

enum LanguageModelsManagerTab: String, CaseIterable, Identifiable {
    case transcription
    case postProcessing

    var id: String { rawValue }

    func title(locale: Locale) -> String {
        switch self {
        case .transcription: return L("Transcription", locale: locale)
        case .postProcessing: return L("Language models", locale: locale)
        }
    }
}

struct LanguageModelsManagerSheet: View {
    @ObservedObject var model: AppModel
    @Binding var selectedTab: LanguageModelsManagerTab
    let onDone: () -> Void

    @State private var isShowingUrlDialog: Bool = false
    @State private var urlDialogName: String = ""
    @State private var urlDialogUrl: String = ""
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Manage language models", bundle: .module)
                    .font(.title3.weight(.semibold))
                Spacer()
            }

            Picker("", selection: $selectedTab) {
                ForEach(LanguageModelsManagerTab.allCases) { tab in
                    Text(tab.title(locale: locale)).tag(tab)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            Form {
                switch selectedTab {
                case .transcription:
                    transcriptionContent
                case .postProcessing:
                    postProcessingContent
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)

            HStack {
                Spacer()
                Button(action: onDone) {
                    Text("Done", bundle: .module)
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(minWidth: 640, idealWidth: 700, minHeight: 480, idealHeight: 560)
        .sheet(isPresented: $isShowingUrlDialog) {
            urlAddDialog
        }
    }

    private var urlAddDialog: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Add language model by URL", bundle: .module)
                .font(.headline)

            Form {
                TextField(text: $urlDialogName) {
                    Text("Display name", bundle: .module)
                }
                TextField(text: $urlDialogUrl) {
                    Text("Download URL (.gguf)", bundle: .module)
                }
            }
            .formStyle(.grouped)

            Text("After adding, the file is fetched via the 'Download' button. Hugging Face 'resolve/main' links are recommended.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack {
                Spacer()
                Button {
                    isShowingUrlDialog = false
                } label: {
                    Text("Cancel", bundle: .module)
                }
                Button {
                    let trimmedName = urlDialogName.trimmingCharacters(in: .whitespacesAndNewlines)
                    let trimmedUrl = urlDialogUrl.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !trimmedName.isEmpty, !trimmedUrl.isEmpty else { return }
                    model.addCustomUrlLlm(name: trimmedName, url: trimmedUrl)
                    urlDialogName = ""
                    urlDialogUrl = ""
                    isShowingUrlDialog = false
                } label: {
                    Text("Add", bundle: .module)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(
                    urlDialogName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        || urlDialogUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                )
            }
        }
        .padding(20)
        .frame(minWidth: 420, idealWidth: 480, minHeight: 240)
    }

    @ViewBuilder
    private var transcriptionContent: some View {
        Section {
            ForEach(ModelPreset.allCases) { preset in
                let status = model.modelStatusList.first(where: { $0.backendModelName == preset.whisperModel })
                whisperTile(preset: preset, status: status)
            }
        } header: {
            Text("Whisper presets", bundle: .module)
        }
    }

    @ViewBuilder
    private var postProcessingContent: some View {
        Section {
            Text(
                "Enable the language models you want available across the whole app as post-processing options. Toggle a model on to make it selectable everywhere; which one post-processing uses is chosen under Post-processing.",
                bundle: .module
            )
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }

        Section {
            ForEach(LlmPreset.allCases) { preset in
                let status = model.llmStatusList.first(where: { $0.displayLabel == preset.displayName })
                llmTile(preset: preset, status: status)
            }
        } header: {
            Text("Local language models", bundle: .module)
        }

        Section {
            if model.settings.customLlmModels.isEmpty {
                Text("No custom language models added yet.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(model.settings.customLlmModels) { entry in
                    customLlmTile(entry: entry)
                }
            }

            HStack(spacing: 10) {
                Button {
                    presentCustomLlmFilePicker()
                } label: {
                    Text("+ Choose file…", bundle: .module)
                }
                Button {
                    urlDialogName = ""
                    urlDialogUrl = ""
                    isShowingUrlDialog = true
                } label: {
                    Text("+ Load from URL", bundle: .module)
                }
            }
        } header: {
            Text("Custom models", bundle: .module)
        }

        Section {
            ForEach(model.llmRegistry.filter { $0.backendKind.isCloud }) { entry in
                cloudModelTile(entry: entry)
            }
            Text("Cloud models need an API key, set under \"Cloud models & API keys\". Enabling one here only makes it selectable; it stays unavailable until a key is stored.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        } header: {
            Text("Cloud", bundle: .module)
        }

        Section {
            TextField(text: model.binding(for: \.ollama.endpoint)) {
                Text("Endpoint", bundle: .module)
            }
            HStack(spacing: 10) {
                Button {
                    model.refreshRemoteModels(backend: .ollama)
                } label: {
                    Text("Fetch models", bundle: .module)
                }
                if let err = model.ollamaModelsError {
                    Text(err)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .lineLimit(2)
                }
            }
            if model.ollamaModels.isEmpty && model.ollamaModelsError == nil {
                Text("No model list fetched yet. A running Ollama server is required.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(model.ollamaModels) { entry in
                    remoteModelTile(entry: entry)
                }
            }
        } header: {
            Text("Ollama", bundle: .module)
        }

        Section {
            TextField(text: model.binding(for: \.lmStudio.endpoint)) {
                Text("Endpoint", bundle: .module)
            }
            HStack(spacing: 10) {
                Button {
                    model.refreshRemoteModels(backend: .lmStudio)
                } label: {
                    Text("Fetch models", bundle: .module)
                }
                if let err = model.lmStudioModelsError {
                    Text(err)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .lineLimit(2)
                }
            }
            if model.lmStudioModels.isEmpty && model.lmStudioModelsError == nil {
                Text("No model list fetched yet. A running LM Studio server is required.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(model.lmStudioModels) { entry in
                    remoteModelTile(entry: entry)
                }
            }
        } header: {
            Text("LM Studio", bundle: .module)
        }
    }

    @ViewBuilder
    private func customLlmTile(entry: CustomLlmModel) -> some View {
        let status = model.customLlmStatusList.first(where: { $0.id == entry.id })
        let needsDownload = status?.needsDownload ?? false
        let isDownloading = status?.isDownloading ?? false
        let isDownloaded = status?.isDownloaded ?? false
        let stableId = LlmModelRefDTO.localCustom(id: entry.id).stableId

        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(entry.name)
                        .font(.body.weight(.medium))
                    Text(status?.sourceLabel ?? entry.source.summaryText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }

                Spacer()

                enableToggle(stableId: stableId)

                if needsDownload {
                    if isDownloaded {
                        Button {
                            model.deleteCustomLlmFile(id: entry.id)
                        } label: {
                            Text("Delete file", bundle: .module)
                        }
                        .disabled(isDownloading)
                    } else {
                        Button {
                            model.startCustomLlmDownload(id: entry.id)
                        } label: {
                            Text(isDownloading ? "Loading…" : "Download", bundle: .module)
                        }
                        .disabled(isDownloading)
                    }
                }

                Button {
                    model.removeCustomLlm(id: entry.id)
                } label: {
                    Text("Remove", bundle: .module)
                }
            }

            if isDownloading, let basisPoints = status?.progressBasisPoints {
                ProgressView(value: Double(basisPoints) / 10_000.0)
                    .accessibilityLabel(String(format: L("Downloading %@", locale: locale), entry.name))
                    .accessibilityValue("\(basisPoints / 100)%")
            }
        }
        .padding(.vertical, 2)
    }

    private func presentCustomLlmFilePicker() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = []
        panel.allowsOtherFileTypes = true
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = L("Add", locale: locale)
        panel.title = L("Choose custom language model", locale: locale)
        panel.message = L("Select a GGUF or GGML model file.", locale: locale)

        guard panel.runModal() == .OK, let url = panel.url else {
            return
        }

        let name = url.deletingPathExtension().lastPathComponent
        model.addCustomLocalLlm(name: name, path: url.path)
    }

    /// Stable id of a fetched remote model, matching Rust `LlmModelRef::stable_id`.
    private func remoteStableId(_ entry: RemoteModelDTO) -> String {
        switch entry.backend {
        case .ollama: return LlmModelRefDTO.ollama(entry.name).stableId
        case .lmStudio: return LlmModelRefDTO.lmStudio(entry.name).stableId
        }
    }

    @ViewBuilder
    private func remoteModelTile(entry: RemoteModelDTO) -> some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.name)
                    .font(.body.weight(.medium))
                Text(entry.summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            enableToggle(stableId: remoteStableId(entry))
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private func cloudModelTile(entry: LlmRegistryEntryDTO) -> some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.displayName)
                    .font(.body.weight(.medium))
                Text(entry.availability == .needsApiKey
                    ? L("Needs API key", locale: locale)
                    : entry.detail)
                    .font(.caption)
                    .foregroundStyle(entry.availability == .needsApiKey
                        ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                    .lineLimit(1)
            }

            Spacer()

            enableToggle(stableId: entry.stableId)
        }
        .padding(.vertical, 2)
    }

    /// App-wide "available everywhere" switch for one model, bound to
    /// `AppModel.enabledModelIds` via its stable id.
    @ViewBuilder
    private func enableToggle(stableId: String) -> some View {
        Toggle(isOn: model.modelEnabledBinding(stableId: stableId)) {
            Text("Enabled", bundle: .module)
        }
        .labelsHidden()
        .toggleStyle(.switch)
        .accessibilityLabel(L("Available everywhere", locale: locale))
    }

    @ViewBuilder
    private func whisperTile(preset: ModelPreset, status: ModelStatusDTO?) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(preset.displayName)
                        .font(.body.weight(.medium))
                    Text(preset.description(locale: locale))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                Spacer(minLength: 8)

                Text(preset.downloadSizeText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }

            if let status, status.isDownloading, let basisPoints = status.progressBasisPoints {
                ProgressView(value: Double(basisPoints) / 10_000.0)
                    .accessibilityLabel(String(format: L("Downloading %@", locale: locale), preset.displayName))
                    .accessibilityValue("\(basisPoints / 100)%")
            }

            HStack(spacing: 10) {
                if status?.isCorrupt == true {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                        .accessibilityHidden(true)
                }

                Text(status.map { L($0.summary, locale: locale) } ?? L("Status unknown.", locale: locale))
                    .font(.caption)
                    .foregroundStyle(status?.isCorrupt == true ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                    .lineLimit(2)

                Spacer()

                if status?.isDownloaded == true {
                    Button {
                        model.deleteModel(preset: preset)
                    } label: {
                        Text("Delete", bundle: .module)
                    }
                    .disabled(status?.isDownloading == true)
                } else {
                    Button {
                        model.startModelDownload(preset: preset)
                    } label: {
                        if status?.isDownloading == true {
                            Text("Loading…", bundle: .module)
                        } else if status?.isCorrupt == true {
                            Text("Download again", bundle: .module)
                        } else {
                            Text("Download", bundle: .module)
                        }
                    }
                    .disabled(status?.isDownloading == true)
                }
            }
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private func llmTile(preset: LlmPreset, status: LlmModelStatusDTO?) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(preset.displayName)
                            .font(.body.weight(.medium))
                        if status?.isLoaded == true {
                            Text("Loaded", bundle: .module)
                                .font(.caption2.weight(.semibold))
                                .padding(.vertical, 2)
                                .padding(.horizontal, 6)
                                .background(Color.torroAccent.opacity(0.14), in: Capsule())
                                .foregroundStyle(Color.torroAccent)
                        }
                    }
                    Text(preset.description(locale: locale))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                Spacer(minLength: 8)

                Text(preset.approxSizeLabel)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()

                enableToggle(stableId: LlmModelRefDTO.localPreset(preset).stableId)
            }

            if let status, status.isDownloading, let basisPoints = status.progressBasisPoints {
                ProgressView(value: Double(basisPoints) / 10_000.0)
                    .accessibilityLabel(String(format: L("Downloading %@", locale: locale), preset.displayName))
                    .accessibilityValue("\(basisPoints / 100)%")
            }

            HStack(spacing: 10) {
                Text(status.map { L($0.summary, locale: locale) } ?? L("Status unknown.", locale: locale))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)

                Spacer()

                if status?.isDownloaded == true {
                    Button {
                        model.deleteLlmModel(preset: preset)
                    } label: {
                        Text("Delete", bundle: .module)
                    }
                    .disabled(status?.isDownloading == true)
                } else {
                    Button {
                        model.startLlmDownload(preset: preset)
                    } label: {
                        Text(status?.isDownloading == true ? "Loading…" : "Download", bundle: .module)
                    }
                    .disabled(status?.isDownloading == true)
                }
            }
        }
        .padding(.vertical, 4)
    }
}
