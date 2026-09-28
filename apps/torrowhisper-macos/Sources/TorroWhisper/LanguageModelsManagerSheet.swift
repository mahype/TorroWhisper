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
        TorroSheetFrame(
            symbol: "brain.head.profile",
            title: Text("Manage language models", bundle: .module)
        ) {
            VStack(spacing: 0) {
                Picker("", selection: $selectedTab) {
                    ForEach(LanguageModelsManagerTab.allCases) { tab in
                        Text(tab.title(locale: locale)).tag(tab)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(.horizontal, 20)
                .padding(.top, 14)

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
            }
        } footer: {
            Button(action: onDone) {
                Text("Done", bundle: .module)
            }
            .buttonStyle(.borderedProminent)
            .keyboardShortcut(.defaultAction)
        }
        .frame(minWidth: 640, idealWidth: 700, minHeight: 480, idealHeight: 560)
        .sheet(isPresented: $isShowingUrlDialog) {
            urlAddDialog
        }
    }

    /// Unlike the manager around it, this one really commits something — so it
    /// is the guide's wizard sheet proper: fixed 520 × 420, Cancel plus exactly
    /// one primary in the foot.
    private var urlAddDialog: some View {
        TorroSheetFrame(
            symbol: "link.badge.plus",
            title: Text("Add language model by URL", bundle: .module),
            subtitle: Text("After adding, the file is fetched via the 'Download' button. Hugging Face 'resolve/main' links are recommended.", bundle: .module)
        ) {
            Form {
                TextField(text: $urlDialogName) {
                    Text("Display name", bundle: .module)
                }
                TextField(text: $urlDialogUrl) {
                    Text("Download URL (.gguf)", bundle: .module)
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        } footer: {
            Button {
                isShowingUrlDialog = false
            } label: {
                Text("Cancel", bundle: .module)
            }
            .keyboardShortcut(.cancelAction)
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
        .frame(width: 520, height: 420)
    }

    /// One row of the transcription list.
    private enum TranscriptionEntry: Identifiable {
        case parakeet(ParakeetModelInfoDTO)
        case whisper(ModelPreset)

        var id: String {
            switch self {
            case .parakeet(let info): return "parakeet:\(info.model.rawValue)"
            case .whisper(let preset): return "whisper:\(preset.rawValue)"
            }
        }
    }

    /// Parakeet and Whisper models grouped by tier (#67), empty tiers left out.
    private var transcriptionGroups: [(tier: ModelTier, entries: [TranscriptionEntry])] {
        let entries: [(ModelTier, TranscriptionEntry)] =
            model.parakeetModels.map { ($0.tier, .parakeet($0)) }
            + ModelPreset.allCases.map { (model.whisperTier($0), .whisper($0)) }
        return ModelTier.allCases.compactMap { tier in
            let members = entries.filter { $0.0 == tier }.map(\.1)
            return members.isEmpty ? nil : (tier, members)
        }
    }

    @ViewBuilder
    private var transcriptionContent: some View {
        if let error = model.parakeetStatus.error {
            Section {
                HStack(spacing: 10) {
                    Text(L(model.parakeetStatus.summary, locale: locale))
                        .font(.caption)
                        .foregroundStyle(.orange)
                        .help(error)
                    Spacer()
                    Button {
                        model.prepareParakeet()
                    } label: {
                        Text("Try again", bundle: .module)
                    }
                }
            }
        }

        ForEach(transcriptionGroups, id: \.tier) { group in
            Section {
                ForEach(group.entries) { entry in
                    Group {
                        switch entry {
                        case .parakeet(let info):
                            parakeetTile(info)
                        case .whisper(let preset):
                            let status = model.modelStatusList.first(where: {
                                $0.backendModelName == preset.whisperModel
                            })
                            whisperTile(preset: preset, status: status)
                        }
                    }
                    // Indented below the tier heading, as agreed in #67.
                    .padding(.leading, 16)
                }
            } header: {
                Text(group.tier.title(locale: locale))
            }
        }
    }

    @ViewBuilder
    private var postProcessingContent: some View {
        Section {
            Text(
                "Enable the language models you want available across the whole app. Toggle a model on to make it selectable; choose the active one under Language models.",
                bundle: .module
            )
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }

        Section {
            if let entry = model.llmRegistry.first(where: { $0.backendKind == .appleFoundation }) {
                HStack(spacing: 10) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(entry.displayName)
                            .font(.body.weight(.medium))
                        Text(entry.detail)
                            .font(.caption)
                            .foregroundStyle(entry.availability == .ready
                                ? AnyShapeStyle(.secondary) : AnyShapeStyle(.orange))
                    }
                    Spacer()
                    Text("No download", bundle: .module)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        } header: {
            Text("Provided by macOS", bundle: .module)
        }

        Section {
            // Grouped by tier (#67): a sub-heading per tier, entries indented.
            ForEach(llmTierGroups, id: \.tier) { group in
                Text(group.tier.title(locale: locale))
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                ForEach(group.presets) { preset in
                    llmTile(preset: preset, status: llmStatus(preset))
                        .padding(.leading, 16)
                }
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

    private func parakeetDescription(_ info: ParakeetModelInfoDTO) -> String {
        switch info.model {
        case .ultra:
            return L("Most accurate Parakeet: 25 languages through Core ML and Apple Neural Engine.", locale: locale)
        case .redux:
            return L("Compact 2-bit Parakeet. Slightly less accurate in English.", locale: locale)
        case .v2:
            return L("English-only predecessor of Parakeet v3.", locale: locale)
        case .v3:
            let successor = info.successor.flatMap { model.parakeetInfo($0)?.displayLabel } ?? ""
            return String(format: L("Replaced by %@. Keeps working until you switch.", locale: locale), successor)
        }
    }

    @ViewBuilder
    private func parakeetTile(_ info: ParakeetModelInfoDTO) -> some View {
        let isActive = model.settings.transcriptionBackend == .parakeet
            && model.parakeetStatus.activeModel == info.model
        let isSelected = model.settings.transcriptionBackend == .parakeet
            && model.effectiveParakeetModel == info.model
        let isPreparing = model.parakeetStatus.preparingModel == info.model

        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(model.parakeetLabel(info))
                            .font(.body.weight(.medium))
                        if isActive {
                            TorroStatusChip(text: L("Active", locale: locale), color: .green)
                        }
                    }
                    Text(parakeetDescription(info))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 8)
                VStack(alignment: .trailing, spacing: 2) {
                    Text(
                        ByteCountFormatter.string(
                            fromByteCount: Int64(info.approxSizeBytes),
                            countStyle: .file
                        )
                    )
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    if info.isInstalled {
                        Text("Installed", bundle: .module)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            if isPreparing {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text(String(format: L("Preparing %@ …", locale: locale), info.displayLabel))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            } else if !isSelected {
                HStack {
                    Spacer()
                    Button {
                        model.useParakeetModel(info.model)
                    } label: {
                        if info.isInstalled {
                            Text("Use", bundle: .module)
                        } else {
                            Text("Download and use", bundle: .module)
                        }
                    }
                    .disabled(!info.isSupportedOnThisMac)
                }
            }
        }
        .padding(.vertical, 4)
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

    private func whisperTileTitle(_ preset: ModelPreset) -> String {
        guard model.isRecommendedTranscriptionPreset(preset) else { return preset.displayName }
        return "\(preset.displayName) (\(L("recommended", locale: locale)))"
    }

    @ViewBuilder
    private func whisperTile(preset: ModelPreset, status: ModelStatusDTO?) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(whisperTileTitle(preset))
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

    private func llmTileTitle(_ preset: LlmPreset, status: LlmModelStatusDTO?) -> String {
        switch status?.tier {
        case .recommended?:
            return "\(preset.displayName) (\(L("recommended", locale: locale)))"
        case .deprecated?:
            return "\(preset.displayName) (\(L("deprecated", locale: locale)))"
        default:
            return preset.displayName
        }
    }

    private func llmStatus(_ preset: LlmPreset) -> LlmModelStatusDTO? {
        model.llmStatusList.first { $0.displayLabel == preset.displayName }
    }

    /// Local presets grouped by the tier the bridge reports, empty tiers left out.
    private var llmTierGroups: [(tier: ModelTier, presets: [LlmPreset])] {
        ModelTier.allCases.compactMap { tier in
            let members = LlmPreset.allCases.filter { (llmStatus($0)?.tier ?? .stable) == tier }
            return members.isEmpty ? nil : (tier, members)
        }
    }

    @ViewBuilder
    private func llmTile(preset: LlmPreset, status: LlmModelStatusDTO?) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(llmTileTitle(preset, status: status))
                            .font(.body.weight(.medium))
                        if status?.isLoaded == true {
                            // "Loaded" is a state, and brand red is never a status
                            // color (design guide §Farbe) — green means "in order".
                            TorroStatusChip(
                                text: L("Loaded", locale: locale),
                                color: .green
                            )
                        }
                    }
                    Text(preset.description(locale: locale))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                    if let successor = status?.successorLabel {
                        Text(String(format: L("Successor: %@", locale: locale), successor))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
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
