import AppKit
import ApplicationServices
import AVFoundation
import Foundation
import SwiftUI

@MainActor
final class AppModel: ObservableObject {
    @Published var settings: AppSettings = .default
    @Published var devices: [DeviceDTO] = []
    @Published var modelStatus: ModelStatusDTO = .empty
    @Published var modelStatusList: [ModelStatusDTO] = []
    @Published var llmStatusList: [LlmModelStatusDTO] = []
    @Published var customLlmStatusList: [CustomLlmStatusDTO] = []
    @Published var ollamaModels: [RemoteModelDTO] = []
    @Published var lmStudioModels: [RemoteModelDTO] = []
    @Published var ollamaModelsError: String?
    @Published var lmStudioModelsError: String?
    @Published var diagnostics: DiagnosticsDTO = .empty
    @Published var runtime: RuntimeStatusDTO = .empty
    @Published var bridgeError: String?
    @Published var onboardingStep: Int = 0
    @Published var editingModeID: String = "cleanup"
    @Published var isCapturingHotkey = false
    @Published var hotkeyCapturePreview = ""
    @Published var hotkeyCaptureError: String?
    @Published var history: [HistoryEntry] = []
    private var lastSeenHistoryRevision: UInt64 = 0

    var onStateChanged: (() -> Void)?
    var onMicSwitched: ((MicSwitchNotification) -> Void)?

    private let bridge = BridgeClient()
    private var timer: Timer?
    private var hotkeyBeforeCapture = AppSettings.default.hotkey
    private var persistedSettingsSnapshot: AppSettings = .default
    private var pendingAutoSaveTask: Task<Void, Never>?
    private static let autoSaveDebounceNanoseconds: UInt64 = 500_000_000
    private var lastSeenMicSwitchEventCount: UInt64 = 0
    private var lastSeenDictationErrorCount: UInt64 = 0
    private var dictationErrorOccurredAt: Date?
    /// How long the red error bubble stays visible after a dictation failure.
    private static let dictationErrorDisplaySeconds: TimeInterval = 6
    private var lastSeenDictationSuccessCount: UInt64 = 0
    private var dictationSuccessOccurredAt: Date?
    /// How long the brief green "done" bubble stays visible after a successful
    /// dictation. Short on purpose — just enough that a fast completion reads as
    /// "finished" instead of the bubble silently vanishing.
    private static let dictationDoneDisplaySeconds: TimeInterval = 1.2

    /// Message of a recent dictation failure while it should still be shown
    /// in the recording bubble; nil once the display window has elapsed.
    var currentDictationErrorMessage: String? {
        guard let dictationErrorOccurredAt,
              Date().timeIntervalSince(dictationErrorOccurredAt) < Self.dictationErrorDisplaySeconds,
              !runtime.lastDictationError.isEmpty
        else {
            return nil
        }
        return runtime.lastDictationError
    }

    /// True for a short window right after a dictation completed successfully,
    /// so the recording bubble can flash a green "done" state instead of just
    /// disappearing (which on fast machines looked like a crash).
    var isShowingDictationDone: Bool {
        guard let dictationSuccessOccurredAt else { return false }
        return Date().timeIntervalSince(dictationSuccessOccurredAt) < Self.dictationDoneDisplaySeconds
    }

    init() {
        reloadAll()
        startPolling()
    }

    var modelDownloadProgress: Double? {
        guard let basisPoints = modelStatus.progressBasisPoints else {
            return nil
        }
        return Double(basisPoints) / 10_000.0
    }

    var hotkeyDisplayText: String {
        runtime.hotkeyRegistered ? runtime.hotkeyText : settings.hotkey
    }

    var hotkeyFieldTitle: String {
        let locale = settings.effectiveLocale
        return isCapturingHotkey
            ? L("Press your keyboard shortcut now", locale: locale)
            : L("Global hotkey", locale: locale)
    }

    var selectedLanguageCode: String {
        TranscriptionLanguageOption.option(for: settings.transcriptionLanguage)?.code ?? "auto"
    }

    var availableLanguageOptions: [TranscriptionLanguageOption] {
        if let current = TranscriptionLanguageOption.option(for: settings.transcriptionLanguage) {
            return TranscriptionLanguageOption.common.contains(current)
                ? TranscriptionLanguageOption.common
                : [current] + TranscriptionLanguageOption.common
        }
        return TranscriptionLanguageOption.common
    }

    var activeProviderLabel: String {
        runtime.providerSummary
    }

    var selectedModelDisplayName: String {
        settings.localModel.displayName
    }

    var selectedPostProcessingDisplayName: String {
        postProcessingChoiceLabel(postProcessingChoiceBinding.wrappedValue)
    }

    var selectedModelStatusText: String {
        let locale = settings.effectiveLocale
        if modelStatus.isDownloading {
            return L("Downloading", locale: locale)
        }
        return modelStatus.isDownloaded
            ? L("Ready", locale: locale)
            : L("Not yet loaded", locale: locale)
    }

    var selectedTranscriptionSummaryText: String {
        let preset = settings.localModel
        let locale = settings.effectiveLocale
        return "\(preset.description(locale: locale)) \u{2013} \(preset.downloadSizeText) \u{2013} \(selectedModelStatusText)"
    }

    var postProcessingSummaryText: String {
        let locale = settings.effectiveLocale
        switch postProcessingChoiceBinding.wrappedValue {
        case .localPreset(let preset):
            return "\(preset.description(locale: locale)) \u{2013} \(preset.approxSizeLabel)"
        case .localCustom(let id):
            let name = settings.customLlmModels.first(where: { $0.id == id })?.name
                ?? L("unknown", locale: locale)
            return "\(L("Custom local model", locale: locale)): \(name)"
        case .ollamaModel(let name):
            let endpoint = settings.ollama.endpoint
            let model = name.isEmpty ? L("no model", locale: locale) : name
            return "Ollama \u{2013} \(endpoint) / \(model)"
        case .lmStudioModel(let name):
            let endpoint = settings.lmStudio.endpoint
            let model = name.isEmpty ? L("no model", locale: locale) : name
            return "LM Studio \u{2013} \(endpoint) / \(model)"
        }
    }

    var selectedModelSizeText: String {
        let locale = settings.effectiveLocale
        if modelStatus.isDownloaded,
           let actual = actualModelFileSize() {
            return "\(Self.formatByteCount(actual)) (\(L("loaded", locale: locale)))"
        }
        let expected = modelStatus.expectedSizeBytes == 0
            ? settings.localModel.downloadSizeBytes
            : modelStatus.expectedSizeBytes
        return "\(L("approx.", locale: locale)) \(Self.formatByteCount(expected)) (\(L("download", locale: locale)))"
    }

    private func actualModelFileSize() -> UInt64? {
        let path = modelStatus.path.isEmpty ? settings.localModelPath : modelStatus.path
        guard !path.isEmpty,
              let attrs = try? FileManager.default.attributesOfItem(atPath: path),
              let size = attrs[.size] as? UInt64 else {
            return nil
        }
        return size
    }

    private static func formatByteCount(_ bytes: UInt64) -> String {
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        formatter.allowedUnits = [.useMB, .useGB]
        formatter.includesUnit = true
        formatter.isAdaptive = true
        return formatter.string(fromByteCount: Int64(bytes))
    }

    var hotkeyRiskHint: String? {
        let source = isCapturingHotkey && !hotkeyCapturePreview.isEmpty ? hotkeyCapturePreview : settings.hotkey
        var hints: [String] = []
        if isSingleKeyHotkey(source) {
            hints.append(L("A single global key may collide with regular typing. Combinations stay safer.", locale: settings.effectiveLocale))
        }
        if hotkeyContainsFunctionKey(source) {
            hints.append(L("F-keys may need the fn modifier or a macOS keyboard setting.", locale: settings.effectiveLocale))
        }
        return hints.isEmpty ? nil : hints.joined(separator: "\n")
    }

    var hotkeyRiskHintDetails: String? {
        let source = isCapturingHotkey && !hotkeyCapturePreview.isEmpty ? hotkeyCapturePreview : settings.hotkey
        guard hotkeyContainsFunctionKey(source) else { return nil }
        return L(
            "If your keyboard maps the F-keys to brightness/volume by default, enable macOS Settings → Keyboard → 'Use F1, F2, etc. keys as standard function keys', or hold the fn key while pressing the shortcut.",
            locale: settings.effectiveLocale
        )
    }

    var availableModes: [ProcessingMode] {
        settings.modes
    }

    var activeMode: ProcessingMode {
        settings.modes.first(where: { $0.id == settings.activeModeId }) ?? settings.modes.first ?? .cleanup
    }

    var editingMode: ProcessingMode {
        settings.modes.first(where: { $0.id == editingModeID }) ?? activeMode
    }

    var activeModeName: String {
        runtime.activeModeName.isEmpty ? activeMode.name : runtime.activeModeName
    }

    var canDeleteModes: Bool {
        settings.modes.count > 1
    }

    var persistedModes: [ProcessingMode] {
        persistedSettingsSnapshot.modes
    }

    var persistedActiveModeID: String {
        persistedSettingsSnapshot.activeModeId
    }

    var persistedPostProcessingEnabled: Bool {
        persistedSettingsSnapshot.postProcessingEnabled
    }

    func binding<Value>(for keyPath: WritableKeyPath<AppSettings, Value>) -> Binding<Value> {
        Binding(
            get: { self.settings[keyPath: keyPath] },
            set: { newValue in
                self.settings[keyPath: keyPath] = newValue
                self.requestAutoSave()
            }
        )
    }

    /// A plain on/off binding for "launch at login", mapping the on state to
    /// `.launchAtLogin` and off to `.manualLaunch`. The `.askOnFirstLaunch`
    /// behavior is no longer offered in the UI — a yes/no choice is clearer.
    var launchAtLoginBinding: Binding<Bool> {
        let base = binding(for: \.startupBehavior)
        return Binding(
            get: { base.wrappedValue == .launchAtLogin },
            set: { base.wrappedValue = $0 ? .launchAtLogin : .manualLaunch }
        )
    }

    func modeBinding<Value>(for keyPath: WritableKeyPath<ProcessingMode, Value>) -> Binding<Value> {
        Binding(
            get: {
                self.settings.modes.first(where: { $0.id == self.editingModeID })?[keyPath: keyPath]
                    ?? self.activeMode[keyPath: keyPath]
            },
            set: { newValue in
                guard let index = self.settings.modes.firstIndex(where: { $0.id == self.editingModeID }) else {
                    return
                }
                self.settings.modes[index][keyPath: keyPath] = newValue
                self.requestAutoSave()
            }
        )
    }

    @discardableResult
    func addDictionaryEntry() -> String {
        let entry = DictionaryEntry()
        settings.dictionary.append(entry)
        requestAutoSave()
        return entry.id
    }

    func deleteDictionaryEntry(id: String) {
        settings.dictionary.removeAll { $0.id == id }
        requestAutoSave()
    }

    func refreshHistory() {
        do {
            history = try bridge.loadHistory()
            lastSeenHistoryRevision = runtime.historyRevision
        } catch {
            publish(error)
        }
    }

    func deleteHistoryEntry(id: String) {
        do {
            _ = try bridge.deleteHistoryEntry(id: id)
            history.removeAll { $0.id == id }
            lastSeenHistoryRevision = lastSeenHistoryRevision &+ 1
            onStateChanged?()
        } catch {
            publish(error)
        }
    }

    func clearHistory() {
        do {
            _ = try bridge.clearHistory()
            history.removeAll()
            lastSeenHistoryRevision = lastSeenHistoryRevision &+ 1
            onStateChanged?()
        } catch {
            publish(error)
        }
    }

    func copyHistoryEntry(id: String) {
        guard let entry = history.first(where: { $0.id == id }) else { return }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(entry.text, forType: .string)
    }

    func dictionaryBinding<Value>(entryID: String, for keyPath: WritableKeyPath<DictionaryEntry, Value>) -> Binding<Value> {
        Binding(
            get: {
                self.settings.dictionary.first(where: { $0.id == entryID })?[keyPath: keyPath]
                    ?? DictionaryEntry()[keyPath: keyPath]
            },
            set: { newValue in
                guard let index = self.settings.dictionary.firstIndex(where: { $0.id == entryID }) else {
                    return
                }
                self.settings.dictionary[index][keyPath: keyPath] = newValue
                self.requestAutoSave()
            }
        )
    }

    func modeChoiceBinding() -> Binding<PostProcessingChoice?> {
        Binding(
            get: {
                self.settings.modes.first(where: { $0.id == self.editingModeID })?.postProcessingChoice
            },
            set: { newValue in
                guard let index = self.settings.modes.firstIndex(where: { $0.id == self.editingModeID }) else {
                    return
                }
                self.settings.modes[index].postProcessingChoice = newValue
                self.requestAutoSave()
            }
        )
    }

    func languageBinding() -> Binding<String> {
        Binding(
            get: { self.selectedLanguageCode },
            set: { newValue in
                self.settings.transcriptionLanguage = newValue == "auto" ? "auto" : newValue
                self.requestAutoSave()
            }
        )
    }

    var postProcessingChoiceBinding: Binding<PostProcessingChoice> {
        Binding(
            get: {
                switch self.settings.activePostProcessingBackend {
                case .local:
                    if !self.settings.activeCustomLlmId.isEmpty,
                       let entry = self.settings.customLlmModels.first(where: { $0.id == self.settings.activeCustomLlmId }) {
                        return .localCustom(id: entry.id)
                    }
                    return .localPreset(self.settings.localLlm)
                case .ollama:
                    return .ollamaModel(self.settings.ollama.modelName)
                case .lmStudio:
                    return .lmStudioModel(self.settings.lmStudio.modelName)
                }
            },
            set: { newValue in
                switch newValue {
                case .localPreset(let preset):
                    self.settings.activePostProcessingBackend = .local
                    self.settings.activeCustomLlmId = ""
                    self.settings.localLlm = preset
                case .localCustom(let id):
                    self.settings.activePostProcessingBackend = .local
                    self.settings.activeCustomLlmId = id
                case .ollamaModel(let name):
                    self.settings.activePostProcessingBackend = .ollama
                    self.settings.activeCustomLlmId = ""
                    self.settings.ollama.modelName = name
                case .lmStudioModel(let name):
                    self.settings.activePostProcessingBackend = .lmStudio
                    self.settings.activeCustomLlmId = ""
                    self.settings.lmStudio.modelName = name
                }
                self.requestAutoSave()
            }
        )
    }

    func isWhisperPresetDownloaded(_ preset: ModelPreset) -> Bool {
        modelStatusList.first(where: { $0.backendModelName == preset.whisperModel })?.isDownloaded ?? false
    }

    func isLlmPresetDownloaded(_ preset: LlmPreset) -> Bool {
        llmStatusList.first(where: { $0.displayLabel == preset.displayName })?.isDownloaded ?? false
    }

    func isCustomLlmAvailable(_ entry: CustomLlmModel) -> Bool {
        switch entry.source {
        case .localPath:
            return true
        case .downloadUrl:
            return customLlmStatusList.first(where: { $0.id == entry.id })?.isDownloaded ?? false
        }
    }

    func isPostProcessingChoiceAvailable(_ choice: PostProcessingChoice) -> Bool {
        switch choice {
        case .localPreset(let preset):
            return isLlmPresetDownloaded(preset)
        case .localCustom(let id):
            guard let entry = settings.customLlmModels.first(where: { $0.id == id }) else {
                return false
            }
            return isCustomLlmAvailable(entry)
        case .ollamaModel(let name):
            return ollamaModels.contains(where: { $0.name == name })
        case .lmStudioModel(let name):
            return lmStudioModels.contains(where: { $0.name == name })
        }
    }

    var availableModelPresets: [ModelPreset] {
        var list = ModelPreset.allCases.filter { isWhisperPresetDownloaded($0) }
        if !list.contains(settings.localModel) {
            list.insert(settings.localModel, at: 0)
        }
        return list
    }

    func whisperPresetPickerLabel(_ preset: ModelPreset) -> String {
        isWhisperPresetDownloaded(preset)
            ? preset.displayName
            : "\(preset.displayName) (\(L("not loaded", locale: settings.effectiveLocale)))"
    }

    var availablePostProcessingChoices: [PostProcessingChoice] {
        var list: [PostProcessingChoice] = []
        list.append(contentsOf: LlmPreset.allCases
            .filter { isLlmPresetDownloaded($0) }
            .map { PostProcessingChoice.localPreset($0) })
        list.append(contentsOf: settings.customLlmModels
            .filter { isCustomLlmAvailable($0) }
            .map { PostProcessingChoice.localCustom(id: $0.id) })
        list.append(contentsOf: ollamaModels.map { PostProcessingChoice.ollamaModel($0.name) })
        list.append(contentsOf: lmStudioModels.map { PostProcessingChoice.lmStudioModel($0.name) })

        let current = postProcessingChoiceBinding.wrappedValue
        if !list.contains(where: { $0.id == current.id }) {
            list.insert(current, at: 0)
        }
        return list
    }

    func postProcessingChoiceLabel(_ choice: PostProcessingChoice) -> String {
        let locale = settings.effectiveLocale
        switch choice {
        case .localCustom(let id):
            if let entry = settings.customLlmModels.first(where: { $0.id == id }) {
                return "\(entry.name) (\(L("custom, local", locale: locale)))"
            }
            return choice.fallbackLabel(locale: locale)
        default:
            return choice.fallbackLabel(locale: locale)
        }
    }

    func postProcessingChoicePickerLabel(_ choice: PostProcessingChoice) -> String {
        let label = postProcessingChoiceLabel(choice)
        return isPostProcessingChoiceAvailable(choice)
            ? label
            : "\(label) (\(L("not loaded", locale: settings.effectiveLocale)))"
    }

    var postProcessingChoices: [PostProcessingChoice] {
        var choices: [PostProcessingChoice] = LlmPreset.allCases.map { .localPreset($0) }

        choices.append(
            contentsOf: settings.customLlmModels.map { .localCustom(id: $0.id) }
        )

        var ollamaNames = ollamaModels.map(\.name)
        let currentOllama = settings.ollama.modelName
        if !currentOllama.isEmpty && !ollamaNames.contains(currentOllama) {
            ollamaNames.insert(currentOllama, at: 0)
        }
        choices.append(contentsOf: ollamaNames.map { .ollamaModel($0) })

        var lmNames = lmStudioModels.map(\.name)
        let currentLmStudio = settings.lmStudio.modelName
        if !currentLmStudio.isEmpty && !lmNames.contains(currentLmStudio) {
            lmNames.insert(currentLmStudio, at: 0)
        }
        choices.append(contentsOf: lmNames.map { .lmStudioModel($0) })

        return choices
    }

    func addCustomLocalLlm(name: String, path: String) {
        let id = UUID().uuidString.lowercased()
        let entry = CustomLlmModel(
            id: id,
            name: name.trimmingCharacters(in: .whitespacesAndNewlines),
            source: .localPath(path: path)
        )
        settings.customLlmModels.append(entry)
        requestAutoSave()
    }

    @discardableResult
    func addCustomUrlLlm(name: String, url: String) -> String {
        let id = UUID().uuidString.lowercased()
        let trimmedUrl = url.trimmingCharacters(in: .whitespacesAndNewlines)
        let filename = URL(string: trimmedUrl)?.lastPathComponent ?? "\(id).gguf"
        let entry = CustomLlmModel(
            id: id,
            name: name.trimmingCharacters(in: .whitespacesAndNewlines),
            source: .downloadUrl(url: trimmedUrl, filename: filename)
        )
        settings.customLlmModels.append(entry)
        flushAutoSave()
        return id
    }

    func startCustomLlmDownload(id: String) {
        do {
            _ = try bridge.startCustomLlmDownload(id: id)
            bridgeError = nil
            poll()
        } catch {
            publish(error)
        }
    }

    func deleteCustomLlmFile(id: String) {
        do {
            _ = try bridge.deleteCustomLlmModel(id: id)
            bridgeError = nil
            poll()
        } catch {
            publish(error)
        }
    }

    func removeCustomLlm(id: String) {
        if let entry = settings.customLlmModels.first(where: { $0.id == id }) {
            if case .downloadUrl = entry.source {
                _ = try? bridge.deleteCustomLlmModel(id: id)
            }
        }
        settings.customLlmModels.removeAll(where: { $0.id == id })
        if settings.activeCustomLlmId == id {
            settings.activeCustomLlmId = ""
            settings.activePostProcessingBackend = .local
        }
        requestAutoSave()
    }

    func reloadAll() {
        do {
            settings = try bridge.loadSettings()
            persistedSettingsSnapshot = settings
            devices = try bridge.listInputDevices()
            modelStatus = try bridge.getModelStatus()
            modelStatusList = (try? bridge.getModelStatusList()) ?? []
            llmStatusList = (try? bridge.getLlmStatusList()) ?? []
            customLlmStatusList = (try? bridge.getCustomLlmStatusList()) ?? []
            diagnostics = try bridge.runPermissionDiagnostics()
            runtime = try bridge.getRuntimeStatus()
            lastSeenMicSwitchEventCount = runtime.micSwitchEventCount
            lastSeenDictationErrorCount = runtime.dictationErrorCount
            lastSeenDictationSuccessCount = runtime.dictationSuccessCount
            history = (try? bridge.loadHistory()) ?? []
            lastSeenHistoryRevision = runtime.historyRevision
            bridgeError = nil
            isCapturingHotkey = false
            hotkeyCapturePreview = ""
            hotkeyCaptureError = nil
            hotkeyBeforeCapture = settings.hotkey
            ensureSelectedMode()
            onStateChanged?()
        } catch {
            publish(error)
        }
    }

    func poll() {
        do {
            runtime = try bridge.getRuntimeStatus()
            modelStatus = try bridge.getModelStatus()
            if let list = try? bridge.getModelStatusList() {
                modelStatusList = list
            }
            if let list = try? bridge.getLlmStatusList() {
                llmStatusList = list
            }
            if let list = try? bridge.getCustomLlmStatusList() {
                customLlmStatusList = list
            }
            checkMicSwitchEvent()
            checkDictationErrorEvent()
            checkDictationSuccessEvent()
            if runtime.historyRevision != lastSeenHistoryRevision {
                history = (try? bridge.loadHistory()) ?? []
                lastSeenHistoryRevision = runtime.historyRevision
            }
            bridgeError = nil
            onStateChanged?()
        } catch {
            publish(error)
        }
    }

    private func checkDictationErrorEvent() {
        guard runtime.dictationErrorCount != lastSeenDictationErrorCount else { return }
        lastSeenDictationErrorCount = runtime.dictationErrorCount
        dictationErrorOccurredAt = Date()
    }

    private func checkDictationSuccessEvent() {
        guard runtime.dictationSuccessCount != lastSeenDictationSuccessCount else { return }
        lastSeenDictationSuccessCount = runtime.dictationSuccessCount
        dictationSuccessOccurredAt = Date()
    }

    private func checkMicSwitchEvent() {
        let current = runtime.micSwitchEventCount
        guard current != lastSeenMicSwitchEventCount else { return }
        let previous = lastSeenMicSwitchEventCount
        lastSeenMicSwitchEventCount = current
        if previous == 0 && runtime.lastMicSwitchMessage.isEmpty { return }
        guard settings.showMicSwitchNotifications else { return }
        let notification = MicSwitchNotification(
            message: runtime.lastMicSwitchMessage,
            activeDevice: runtime.activeInputDeviceName
        )
        devices = (try? bridge.listInputDevices()) ?? devices
        onMicSwitched?(notification)
    }

    func reregisterHotkey() {
        do {
            _ = try bridge.reregisterHotkey()
            runtime = try bridge.getRuntimeStatus()
            bridgeError = nil
            onStateChanged?()
        } catch {
            publish(error)
        }
    }

    func notifyDeviceListChanged() {
        do {
            _ = try bridge.notifyDeviceChange()
            let refreshedSettings = try bridge.loadSettings()
            settings = refreshedSettings
            persistedSettingsSnapshot = refreshedSettings
            devices = try bridge.listInputDevices()
            runtime = try bridge.getRuntimeStatus()
            checkMicSwitchEvent()
            bridgeError = nil
            onStateChanged?()
        } catch {
            publish(error)
        }
    }

    func refreshDevices() {
        do {
            devices = try bridge.listInputDevices()
            bridgeError = nil
        } catch {
            publish(error)
        }
    }

    func refreshDiagnostics() {
        do {
            var loaded = try bridge.runPermissionDiagnostics()
            loaded.items.append(contentsOf: permissionDiagnosticItems())
            diagnostics = loaded
            bridgeError = nil
        } catch {
            publish(error)
        }
    }

    /// Swift-side permission checks appended to the bridge diagnostics. The Rust
    /// bridge can't read macOS TCC state, so the actual microphone/accessibility
    /// authorization is evaluated here and surfaced as OK/error entries.
    private func permissionDiagnosticItems() -> [DiagnosticItemDTO] {
        let locale = settings.effectiveLocale

        let microphone: DiagnosticItemDTO
        if microphoneAuthorizationStatus == .authorized {
            microphone = DiagnosticItemDTO(
                title: L("Microphone permission", locale: locale),
                status: .ok,
                problem: L("Microphone access granted.", locale: locale),
                recommendation: L("No action needed.", locale: locale)
            )
        } else {
            microphone = DiagnosticItemDTO(
                title: L("Microphone permission", locale: locale),
                status: .error,
                problem: L("Microphone access is not granted yet.", locale: locale),
                recommendation: L("Enable Open Whisper under Microphone in System Settings → Privacy & Security.", locale: locale)
            )
        }

        let accessibility: DiagnosticItemDTO
        if accessibilityTrusted {
            accessibility = DiagnosticItemDTO(
                title: L("Accessibility permission", locale: locale),
                status: .ok,
                problem: L("Accessibility access granted.", locale: locale),
                recommendation: L("No action needed.", locale: locale)
            )
        } else {
            accessibility = DiagnosticItemDTO(
                title: L("Accessibility permission", locale: locale),
                status: .warning,
                problem: L("Accessibility access is not granted yet.", locale: locale),
                recommendation: L("Enable Open Whisper under Accessibility in System Settings → Privacy & Security so it can type into other apps.", locale: locale)
            )
        }

        return [microphone, accessibility]
    }

    @discardableResult
    func saveSettings() -> Bool {
        pendingAutoSaveTask?.cancel()
        pendingAutoSaveTask = nil
        do {
            try persistSettings()
            reloadAll()
            return true
        } catch let error as InlineHotkeyValidationError {
            failHotkeyCapture(error.message)
            return false
        } catch {
            publish(error)
            return false
        }
    }

    func requestAutoSave() {
        pendingAutoSaveTask?.cancel()
        pendingAutoSaveTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: AppModel.autoSaveDebounceNanoseconds)
            guard !Task.isCancelled else { return }
            await MainActor.run {
                self?.flushAutoSave()
            }
        }
    }

    func flushAutoSave() {
        pendingAutoSaveTask?.cancel()
        pendingAutoSaveTask = nil
        guard settings != persistedSettingsSnapshot else { return }
        do {
            try persistSettings()
            runtime = try bridge.getRuntimeStatus()
            onStateChanged?()
        } catch let error as InlineHotkeyValidationError {
            failHotkeyCapture(error.message)
        } catch {
            publish(error)
        }
    }

    private func persistSettings() throws {
        let normalizedHotkey = try prepareHotkeyForAssignment(
            settings.hotkey,
            allowNoOpHotkeys: [persistedSettingsSnapshot.hotkey, runtime.hotkeyRegistered ? runtime.hotkeyText : nil]
        )
        settings.hotkey = normalizedHotkey
        hotkeyCaptureError = nil
        bridgeError = nil
        _ = try bridge.saveSettings(settings)
        persistedSettingsSnapshot = settings
    }

    func completeOnboarding() -> Bool {
        settings.onboardingCompleted = true
        return saveSettings()
    }

    func reopenOnboarding() {
        onboardingStep = 0
    }

    func choosePreset(_ preset: ModelPreset) {
        settings.localModel = preset
        clearPinnedDefaultModelPath(&settings.localModelPath)
        requestAutoSave()
    }

    /// localModelPath is only meant for user-chosen custom model files. Older
    /// versions pinned a preset's default path here, which went stale after a
    /// preset switch — the bridge resolves the per-preset default itself when
    /// the path is empty.
    private func clearPinnedDefaultModelPath(_ path: inout String) {
        guard !path.isEmpty else { return }
        let filename = URL(fileURLWithPath: path).lastPathComponent
        if Set(ModelPreset.allCases.map(\.defaultFilename)).contains(filename) {
            path = ""
        }
    }

    func beginEditingMode(_ modeID: String) {
        editingModeID = modeID
    }

    func setActiveMode(_ modeID: String) {
        settings.activeModeId = modeID
        flushAutoSave()
    }

    func activateMode(_ modeID: String) {
        settings.activeModeId = modeID
        settings.postProcessingEnabled = true
        flushAutoSave()
    }

    func disablePostProcessing() {
        settings.postProcessingEnabled = false
        flushAutoSave()
    }

    func persistActiveModeImmediately(_ modeID: String) {
        do {
            var freshSettings = try bridge.loadSettings()
            if !freshSettings.modes.contains(where: { $0.id == modeID }) {
                return
            }
            freshSettings.activeModeId = modeID
            freshSettings.postProcessingEnabled = true
            _ = try bridge.saveSettings(freshSettings)
            reloadAll()
        } catch {
            publish(error)
        }
    }

    func persistPostProcessingEnabledImmediately(_ enabled: Bool) {
        do {
            var freshSettings = try bridge.loadSettings()
            freshSettings.postProcessingEnabled = enabled
            _ = try bridge.saveSettings(freshSettings)
            reloadAll()
        } catch {
            publish(error)
        }
    }

    func persistInputDeviceImmediately(_ name: String) {
        do {
            var freshSettings = try bridge.loadSettings()
            guard freshSettings.inputDeviceName != name else { return }
            freshSettings.inputDeviceName = name
            _ = try bridge.saveSettings(freshSettings)
            reloadAll()
        } catch {
            publish(error)
        }
    }

    func persistWhisperPresetImmediately(_ preset: ModelPreset) {
        do {
            var freshSettings = try bridge.loadSettings()
            freshSettings.localModel = preset
            clearPinnedDefaultModelPath(&freshSettings.localModelPath)
            _ = try bridge.saveSettings(freshSettings)
            reloadAll()
        } catch {
            publish(error)
        }
    }

    @discardableResult
    func createMode() -> String {
        let existingNames = Set(settings.modes.map(\.name))
        let baseName = L("New post-processing", locale: settings.effectiveLocale)
        var suffix = 1
        var candidate = baseName
        while existingNames.contains(candidate) {
            suffix += 1
            candidate = "\(baseName) \(suffix)"
        }

        let mode = ProcessingMode(
            id: UUID().uuidString.lowercased(),
            name: candidate,
            prompt: ""
        )
        settings.modes.append(mode)
        flushAutoSave()
        return mode.id
    }

    func deleteMode(_ modeID: String) {
        guard canDeleteModes,
              let index = settings.modes.firstIndex(where: { $0.id == modeID }) else {
            return
        }

        settings.modes.remove(at: index)
        if settings.activeModeId == modeID {
            settings.activeModeId = settings.modes.first?.id ?? ProcessingMode.cleanup.id
        }
        ensureSelectedMode()
        flushAutoSave()
    }

    func startHotkeyCapture() {
        hotkeyBeforeCapture = settings.hotkey
        hotkeyCapturePreview = settings.hotkey
        hotkeyCaptureError = nil
        isCapturingHotkey = true
    }

    func updateHotkeyCapturePreview(_ value: String) {
        hotkeyCapturePreview = value
        hotkeyCaptureError = nil
    }

    func commitCapturedHotkey(_ hotkey: String) {
        do {
            let normalized = try prepareHotkeyForAssignment(
                hotkey,
                allowNoOpHotkeys: [hotkeyBeforeCapture, runtime.hotkeyRegistered ? runtime.hotkeyText : nil]
            )
            settings.hotkey = normalized
            hotkeyCapturePreview = normalized
            hotkeyCaptureError = nil
            bridgeError = nil
            isCapturingHotkey = false
            flushAutoSave()
        } catch {
            failHotkeyCapture(error.localizedDescription)
        }
    }

    func cancelHotkeyCapture() {
        settings.hotkey = hotkeyBeforeCapture
        hotkeyCapturePreview = ""
        hotkeyCaptureError = nil
        isCapturingHotkey = false
    }

    func clearHotkeyCapture() {
        settings.hotkey = hotkeyBeforeCapture
        hotkeyCapturePreview = ""
        hotkeyCaptureError = L("Open Whisper needs a global hotkey. Empty input is not allowed.", locale: settings.effectiveLocale)
        isCapturingHotkey = false
    }

    func failHotkeyCapture(_ message: String) {
        hotkeyCaptureError = message
        bridgeError = nil
    }

    func startModelDownload() {
        startModelDownload(preset: settings.localModel)
    }

    func startModelDownload(preset: ModelPreset) {
        do {
            _ = try bridge.startModelDownload(preset: preset)
            bridgeError = nil
            poll()
        } catch {
            publish(error)
        }
    }

    func deleteModel() {
        deleteModel(preset: settings.localModel)
    }

    func deleteModel(preset: ModelPreset) {
        do {
            _ = try bridge.deleteModel(preset: preset)
            bridgeError = nil
            poll()
        } catch {
            publish(error)
        }
    }

    func startLlmDownload(preset: LlmPreset) {
        do {
            _ = try bridge.startLlmDownload(preset: preset)
            bridgeError = nil
            poll()
        } catch {
            publish(error)
        }
    }

    func deleteLlmModel(preset: LlmPreset) {
        do {
            _ = try bridge.deleteLlmModel(preset: preset)
            bridgeError = nil
            poll()
        } catch {
            publish(error)
        }
    }

    func refreshRemoteModels(backend: RemoteModelBackend) {
        do {
            let list = try bridge.listRemoteModels(backend: backend)
            switch backend {
            case .ollama:
                ollamaModels = list
                ollamaModelsError = nil
            case .lmStudio:
                lmStudioModels = list
                lmStudioModelsError = nil
            }
        } catch {
            let message = (error as? BridgeError)?.message ?? error.localizedDescription
            switch backend {
            case .ollama:
                ollamaModels = []
                ollamaModelsError = message
            case .lmStudio:
                lmStudioModels = []
                lmStudioModelsError = message
            }
        }
    }

    func toggleDictation() {
        do {
            if runtime.isRecording {
                _ = try bridge.stopDictation()
            } else {
                _ = try bridge.startDictation()
            }
            bridgeError = nil
            poll()
        } catch {
            publish(error)
        }
    }

    func cancelDictation() {
        do {
            _ = try bridge.cancelDictation()
            bridgeError = nil
            poll()
        } catch {
            publish(error)
        }
    }

    func openSystemSettings() {
        let candidates = [
            "/System/Applications/System Settings.app",
            "/System/Applications/System Preferences.app",
        ]

        for candidate in candidates where FileManager.default.fileExists(atPath: candidate) {
            NSWorkspace.shared.open(URL(fileURLWithPath: candidate))
            return
        }
    }

    /// Folder picker for the "save to disk" destination (recordings/transcripts).
    func chooseSaveDirectory() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = L("Choose", locale: settings.effectiveLocale)
        if !settings.saveDirectory.isEmpty {
            panel.directoryURL = URL(fileURLWithPath: settings.saveDirectory)
        }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        settings.saveDirectory = url.path
        requestAutoSave()
    }

    /// Opens the configured "save to disk" folder in Finder.
    func revealSaveDirectoryInFinder() {
        guard !settings.saveDirectory.isEmpty else { return }
        NSWorkspace.shared.open(URL(fileURLWithPath: settings.saveDirectory, isDirectory: true))
    }

    /// Current macOS authorization status for the microphone.
    var microphoneAuthorizationStatus: AVAuthorizationStatus {
        AVCaptureDevice.authorizationStatus(for: .audio)
    }

    /// Human-readable summary of the current microphone permission, localized.
    var microphonePermissionSummary: String {
        let locale = settings.effectiveLocale
        switch microphoneAuthorizationStatus {
        case .authorized:
            return L("Microphone access is granted.", locale: locale)
        case .denied:
            return L("Microphone access is denied. Enable it in System Settings.", locale: locale)
        case .restricted:
            return L("Microphone access is restricted by system policy.", locale: locale)
        case .notDetermined:
            return L("Microphone access has not been requested yet.", locale: locale)
        @unknown default:
            return L("Microphone access status is unknown.", locale: locale)
        }
    }

    /// Checks the microphone permission and routes the user to the right place to fix it.
    ///
    /// - `.notDetermined`: triggers the native permission prompt.
    /// - `.denied` / `.restricted`: opens the Microphone privacy pane directly.
    /// - `.authorized`: just refreshes the published status so the UI updates.
    func checkAndRequestMicrophoneAccess() {
        switch microphoneAuthorizationStatus {
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { [weak self] _ in
                Task { @MainActor in
                    self?.onStateChanged?()
                    self?.objectWillChange.send()
                }
            }
        case .denied, .restricted:
            openMicrophonePrivacySettings()
        case .authorized:
            objectWillChange.send()
        @unknown default:
            openMicrophonePrivacySettings()
        }
    }

    /// Opens the Microphone pane in System Settings (deep link), falling back to the app.
    func openMicrophonePrivacySettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"),
           NSWorkspace.shared.open(url) {
            return
        }
        openSystemSettings()
    }

    /// Whether the app is currently trusted for Accessibility (needed to insert text
    /// into other apps via simulated keystrokes).
    var accessibilityTrusted: Bool {
        AXIsProcessTrusted()
    }

    /// Human-readable summary of the current Accessibility permission, localized.
    var accessibilityPermissionSummary: String {
        let locale = settings.effectiveLocale
        return accessibilityTrusted
            ? L("Accessibility access is granted.", locale: locale)
            : L("Accessibility access is missing. Enable Open Whisper in System Settings.", locale: locale)
    }

    /// Checks the Accessibility permission and routes the user to fix it if needed.
    ///
    /// When not trusted, triggers the native "add to Accessibility" prompt and also
    /// opens the Accessibility privacy pane directly.
    func checkAndRequestAccessibilityAccess() {
        if accessibilityTrusted {
            objectWillChange.send()
            return
        }
        // Key value of `kAXTrustedCheckOptionPrompt`; referenced as a literal because the
        // global is not concurrency-safe under strict concurrency checking.
        let promptKey = "AXTrustedCheckOptionPrompt"
        _ = AXIsProcessTrustedWithOptions([promptKey: true] as CFDictionary)
        openAccessibilityPrivacySettings()
    }

    /// Opens the Accessibility pane in System Settings (deep link), falling back to the app.
    func openAccessibilityPrivacySettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"),
           NSWorkspace.shared.open(url) {
            return
        }
        openSystemSettings()
    }

    /// Removes the app's stale Accessibility TCC entry via `tccutil reset`, then opens
    /// the Accessibility pane so the user can re-add the app cleanly.
    ///
    /// A stale entry (left over from an older build/path) can list the app as enabled
    /// while macOS no longer honors it; removing it forces a fresh, working grant.
    func resetAccessibilityPermission() {
        guard let bundleID = Bundle.main.bundleIdentifier else {
            openAccessibilityPrivacySettings()
            return
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/tccutil")
        process.arguments = ["reset", "Accessibility", bundleID]
        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            publish(error)
        }
        objectWillChange.send()
        openAccessibilityPrivacySettings()
    }

    private func startPolling() {
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 0.35, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.poll()
            }
        }
    }

    private func publish(_ error: Error) {
        isCapturingHotkey = false
        bridgeError = error.localizedDescription
        onStateChanged?()
    }

    private func prepareHotkeyForAssignment(
        _ hotkey: String,
        allowNoOpHotkeys: [String?]
    ) throws -> String {
        let normalized: String
        do {
            normalized = try bridge.validateHotkey(hotkey)
        } catch {
            throw InlineHotkeyValidationError(message: error.localizedDescription)
        }

        do {
            try HotkeyAssignmentAdvisor.assertCanAssign(
                normalized,
                allowNoOpHotkeys: allowNoOpHotkeys.compactMap { $0 }
            )
        } catch {
            throw InlineHotkeyValidationError(message: error.localizedDescription)
        }

        return normalized
    }

    private func ensureSelectedMode() {
        if settings.modes.isEmpty {
            settings.modes = [.cleanup]
        }

        if !settings.modes.contains(where: { $0.id == settings.activeModeId }) {
            settings.activeModeId = settings.modes.first?.id ?? ProcessingMode.cleanup.id
        }

        if !settings.modes.contains(where: { $0.id == editingModeID }) {
            editingModeID = settings.activeModeId
        }
    }
}

private func isSingleKeyHotkey(_ hotkey: String) -> Bool {
    let normalized = hotkey.trimmingCharacters(in: .whitespacesAndNewlines)
    return !normalized.isEmpty && !normalized.contains("+")
}

private func hotkeyContainsFunctionKey(_ hotkey: String) -> Bool {
    let tokens = hotkey
        .split(separator: "+")
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
    for token in tokens {
        guard token.hasPrefix("f"),
              let number = Int(token.dropFirst()),
              (1...20).contains(number) else {
            continue
        }
        return true
    }
    return false
}

private struct InlineHotkeyValidationError: LocalizedError {
    let message: String

    var errorDescription: String? { message }
}

struct MicSwitchNotification {
    let message: String
    let activeDevice: String
}
