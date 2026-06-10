import AppKit
import SwiftUI

struct SettingsView: View {
    @ObservedObject var model: AppModel
    let updaterController: UpdaterController
    let onReopenOnboarding: () -> Void
    @State private var selectedSection: SettingsSection? = .recording
    @State private var isEditingMode: Bool = false
    @State private var isManagingLanguageModels: Bool = false
    @State private var managerTab: LanguageModelsManagerTab = .transcription
    @State private var columnVisibility: NavigationSplitViewVisibility = .all
    @State private var isConfirmingHistoryClear: Bool = false
    @State private var isConfirmingAccessibilityReset: Bool = false
    @Environment(\.locale) private var locale

    var body: some View {
        NavigationSplitView(columnVisibility: $columnVisibility) {
            List(SettingsSection.allCases, selection: $selectedSection) { section in
                Label(section.title(locale: locale), systemImage: section.symbolName)
                    .tag(section)
            }
            .listStyle(.sidebar)
            .frame(minWidth: 240, idealWidth: 240)
            .navigationSplitViewColumnWidth(240)
            .toolbar(removing: .sidebarToggle)
        } detail: {
            Form {
                detailContent(for: detailSection)
            }
            .formStyle(.grouped)
            .navigationTitle(detailSection.title(locale: locale))
            .safeAreaInset(edge: .bottom) {
                bottomBar
            }
            .sheet(isPresented: $isEditingMode) {
                ModeEditorSheet(model: model) {
                    isEditingMode = false
                }
            }
            .sheet(isPresented: $isManagingLanguageModels) {
                LanguageModelsManagerSheet(
                    model: model,
                    selectedTab: $managerTab
                ) {
                    isManagingLanguageModels = false
                }
            }
            .alert(
                Text("Clear history?", bundle: .module),
                isPresented: $isConfirmingHistoryClear
            ) {
                Button(role: .destructive) {
                    model.clearHistory()
                } label: {
                    Text("Clear all", bundle: .module)
                }
                Button(role: .cancel) {} label: {
                    Text("Cancel", bundle: .module)
                }
            } message: {
                Text("This will permanently delete all entries.", bundle: .module)
            }
            .alert(
                Text("Reset accessibility permission?", bundle: .module),
                isPresented: $isConfirmingAccessibilityReset
            ) {
                Button(role: .destructive) {
                    model.resetAccessibilityPermission()
                } label: {
                    Text("Reset and reopen settings", bundle: .module)
                }
                Button(role: .cancel) {} label: {
                    Text("Cancel", bundle: .module)
                }
            } message: {
                Text("This removes Open Whisper from the Accessibility list so you can add it again. You will need to re-grant access afterwards.", bundle: .module)
            }
        }
        .navigationSplitViewStyle(.balanced)
        .frame(width: 820, height: 720)
    }

    private var detailSection: SettingsSection {
        selectedSection ?? .recording
    }

    @ViewBuilder
    private func detailContent(for section: SettingsSection) -> some View {
        switch section {
        case .recording:
            recordingContent
        case .modes:
            modesContent
        case .dictionary:
            dictionaryContent
        case .history:
            historyContent
        case .languageModels:
            languageModelsContent
        case .startup:
            startupContent
        case .updates:
            UpdatesSettingsView(updaterController: updaterController)
        case .diagnostics:
            diagnosticsContent
        case .help:
            helpContent
        }
    }

    @ViewBuilder
    private var recordingContent: some View {
        Section {
            Picker(selection: model.binding(for: \.inputDeviceName)) {
                ForEach(deviceNames, id: \.self) { device in
                    Text(device).tag(device)
                }
            } label: {
                Text("Microphone", bundle: .module)
            }

            Picker(selection: model.languageBinding()) {
                ForEach(model.availableLanguageOptions) { option in
                    Text(option.label(locale: locale)).tag(option.code)
                }
            } label: {
                Text("Language", bundle: .module)
            }

            Button {
                model.refreshDevices()
            } label: {
                Text("Refresh devices", bundle: .module)
            }

            Toggle(isOn: model.binding(for: \.autoSwitchMicOnHotplug)) {
                Text("Switch microphone automatically when unplugged", bundle: .module)
            }
            Toggle(isOn: model.binding(for: \.showMicSwitchNotifications)) {
                Text("Show notification when microphone changes", bundle: .module)
            }
            .disabled(!model.settings.autoSwitchMicOnHotplug)
        } header: {
            Text("Audio source", bundle: .module)
        }

        Section {
            Picker(selection: model.binding(for: \.triggerMode)) {
                ForEach(TriggerMode.allCases) { mode in
                    Text(mode.label(locale: locale)).tag(mode)
                }
            } label: {
                Text("Mode", bundle: .module)
            }
            .pickerStyle(.segmented)
        } header: {
            Text("Trigger", bundle: .module)
        }

        Section {
            HotkeyRecorderField(
                title: model.hotkeyFieldTitle,
                currentHotkey: model.settings.hotkey,
                isCapturing: model.isCapturingHotkey,
                preview: model.hotkeyCapturePreview,
                errorText: model.hotkeyCaptureError,
                warningText: model.hotkeyRiskHint,
                warningDetails: model.hotkeyRiskHintDetails,
                onStartCapture: { model.startHotkeyCapture() },
                onCommit: { model.commitCapturedHotkey($0) },
                onCancel: { model.cancelHotkeyCapture() },
                onClear: { model.clearHotkeyCapture() },
                onPreview: { model.updateHotkeyCapturePreview($0) },
                onInvalid: { model.failHotkeyCapture($0) }
            )
        } header: {
            Text("Global hotkey", bundle: .module)
        }

        Section {
            Toggle(isOn: model.binding(for: \.insertTextAutomatically)) {
                Text("Insert text automatically", bundle: .module)
            }
            Toggle(isOn: model.binding(for: \.restoreClipboardAfterInsert)) {
                Text("Restore clipboard after inserting", bundle: .module)
            }
        } header: {
            Text("Text output", bundle: .module)
        }

        Section {
            Toggle(isOn: model.binding(for: \.showRecordingIndicator)) {
                Text("Show waveform window while recording", bundle: .module)
            }

            Picker(selection: model.binding(for: \.waveformStyle)) {
                ForEach(WaveformStyle.allCases) { style in
                    Text(style.label(locale: locale)).tag(style)
                }
            } label: {
                Text("Style", bundle: .module)
            }
            .disabled(!model.settings.showRecordingIndicator)

            Picker(selection: model.binding(for: \.waveformColor)) {
                ForEach(WaveformColor.allCases) { color in
                    Text(color.label(locale: locale))
                        .foregroundStyle(color.swiftUIColor)
                        .tag(color)
                }
            } label: {
                Text("Color", bundle: .module)
            }
            .disabled(!model.settings.showRecordingIndicator)

            Toggle(isOn: model.binding(for: \.largeRecordingIndicator)) {
                Text("Large view (easier to read)", bundle: .module)
            }
            .disabled(!model.settings.showRecordingIndicator)

            Toggle(isOn: model.binding(for: \.highContrastRecordingIndicator)) {
                Text("High contrast", bundle: .module)
            }
            .disabled(!model.settings.showRecordingIndicator)
        } header: {
            Text("Recording indicator", bundle: .module)
        }
    }

    @ViewBuilder
    private var modesContent: some View {
        Section {
            PostProcessingOffTile(
                isActive: !model.settings.postProcessingEnabled,
                onActivate: { model.disablePostProcessing() }
            )
            .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))

            ForEach(model.availableModes) { mode in
                ModeListTile(
                    mode: mode,
                    isActive: model.settings.postProcessingEnabled && model.settings.activeModeId == mode.id,
                    canDelete: model.canDeleteModes,
                    onActivate: { model.activateMode(mode.id) },
                    onEdit: {
                        model.beginEditingMode(mode.id)
                        isEditingMode = true
                    },
                    onDelete: { model.deleteMode(mode.id) }
                )
                .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
            }

            HStack(spacing: 10) {
                Button {
                    let newID = model.createMode()
                    model.beginEditingMode(newID)
                    isEditingMode = true
                } label: {
                    Text("New post-processing", bundle: .module)
                }
                Spacer()
            }
        } header: {
            Text("Post-processing", bundle: .module)
        }
    }

    @ViewBuilder
    private var dictionaryContent: some View {
        Section {
            if model.settings.dictionary.isEmpty {
                Text("No entries yet. Add a word that Whisper transcribes incorrectly and the replacement that should be used instead.", bundle: .module)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                ForEach(model.settings.dictionary) { entry in
                    DictionaryEntryRow(
                        patternBinding: model.dictionaryBinding(entryID: entry.id, for: \.pattern),
                        replacementBinding: model.dictionaryBinding(entryID: entry.id, for: \.replacement),
                        caseSensitiveBinding: model.dictionaryBinding(entryID: entry.id, for: \.caseSensitive),
                        wholeWordBinding: model.dictionaryBinding(entryID: entry.id, for: \.wholeWord),
                        onDelete: { model.deleteDictionaryEntry(id: entry.id) }
                    )
                    .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
                }
            }

            HStack(spacing: 10) {
                Button {
                    model.addDictionaryEntry()
                } label: {
                    Text("Add entry", bundle: .module)
                }
                Spacer()
            }
        } header: {
            Text("Word replacements", bundle: .module)
        } footer: {
            Text("Replacements run on the raw transcript before any post-processing. Each replacement can be applied case-sensitively or only to whole words.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    @ViewBuilder
    private var historyContent: some View {
        Section {
            Toggle(isOn: model.binding(for: \.historyEnabled)) {
                Text("Record history", bundle: .module)
            }
            Stepper(
                value: model.binding(for: \.historyMaxEntries),
                in: historyMaxEntriesMin...historyMaxEntriesLimit,
                step: 10
            ) {
                HStack {
                    Text("Maximum history entries", bundle: .module)
                    Spacer()
                    Text("\(model.settings.historyMaxEntries)")
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }
        } header: {
            Text("Settings", bundle: .module)
        } footer: {
            Text("History records the final transcript of each dictation, including ones cancelled with Escape. When the cap is reached, the oldest entry is dropped.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }

        Section {
            if model.history.isEmpty {
                Text("No history yet.", bundle: .module)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                ForEach(model.history) { entry in
                    HistoryEntryRow(
                        entry: entry,
                        onDelete: { model.deleteHistoryEntry(id: entry.id) },
                        onCopy: { model.copyHistoryEntry(id: entry.id) }
                    )
                    .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
                }
            }

            HStack {
                Spacer()
                Button(role: .destructive) {
                    isConfirmingHistoryClear = true
                } label: {
                    Text("Clear all", bundle: .module)
                }
                .disabled(model.history.isEmpty)
            }
        } header: {
            Text("Recent dictations", bundle: .module)
        }
    }

    @ViewBuilder
    private var languageModelsContent: some View {
        Section {
            Picker(selection: model.binding(for: \.localModel)) {
                ForEach(model.availableModelPresets) { preset in
                    Text(model.whisperPresetPickerLabel(preset)).tag(preset)
                }
            } label: {
                Text("Model", bundle: .module)
            }

            Text(model.selectedTranscriptionSummaryText)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            if let progress = model.modelDownloadProgress {
                ProgressView(value: progress)
            }

            Button {
                managerTab = .transcription
                isManagingLanguageModels = true
            } label: {
                Text("Manage language models…", bundle: .module)
            }
        } header: {
            Text("Transcription", bundle: .module)
        }

        Section {
            Picker(selection: model.postProcessingChoiceBinding) {
                ForEach(model.availablePostProcessingChoices) { choice in
                    Text(model.postProcessingChoicePickerLabel(choice)).tag(choice)
                }
            } label: {
                Text("Model", bundle: .module)
            }

            Text(model.postProcessingSummaryText)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            Button {
                managerTab = .postProcessing
                isManagingLanguageModels = true
            } label: {
                Text("Manage language models…", bundle: .module)
            }
        } header: {
            Text("Post-processing", bundle: .module)
        }
    }

    @ViewBuilder
    private var startupContent: some View {
        Section {
            Toggle(isOn: model.launchAtLoginBinding) {
                Text("Launch at login", bundle: .module)
            }
        } header: {
            Text("System startup", bundle: .module)
        }

        Section {
            Picker(selection: model.binding(for: \.uiLanguage)) {
                ForEach(UiLanguage.allCases) { option in
                    Text(option.displayLabel).tag(option)
                }
            } label: {
                Text("App language", bundle: .module)
            }
        } header: {
            Text("Language", bundle: .module)
        } footer: {
            VStack(alignment: .leading, spacing: 4) {
                Text("“System” follows your macOS language setting.", bundle: .module)
                Text("Changes take effect after restarting Open Whisper.", bundle: .module)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }

        Section {
            Toggle(isOn: model.binding(for: \.vadEnabled)) {
                Text("Voice Activity Detection", bundle: .module)
            }

            LabeledContent {
                HStack(spacing: 10) {
                    Slider(
                        value: Binding(
                            get: { Double(model.settings.vadSilenceMs) },
                            set: {
                                model.settings.vadSilenceMs = UInt32($0.rounded())
                                model.requestAutoSave()
                            }
                        ),
                        in: 300...2_500,
                        step: 50
                    )
                    .frame(width: 200)
                    Text("\(model.settings.vadSilenceMs) ms")
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                        .frame(width: 70, alignment: .trailing)
                }
            } label: {
                Text("Silence stop", bundle: .module)
            }
        } header: {
            Text("Dictation stop", bundle: .module)
        }

        Section {
            LabeledContent {
                Text(model.runtime.startupSummary)
            } label: {
                Text("System startup", bundle: .module)
            }
            LabeledContent {
                Text(model.runtime.hotkeyText)
            } label: {
                Text("Hotkey", bundle: .module)
            }
            LabeledContent {
                Text(model.activeModeName)
            } label: {
                Text("Post-processing", bundle: .module)
            }
        } header: {
            Text("Currently registered", bundle: .module)
        }
    }

    @ViewBuilder
    private var diagnosticsContent: some View {
        Section {
            Text(model.diagnostics.summary)
                .font(.subheadline)
                .foregroundStyle(.secondary)

            HStack(spacing: 10) {
                Button {
                    model.refreshDiagnostics()
                } label: {
                    Text("Refresh", bundle: .module)
                }
                Button {
                    model.openSystemSettings()
                } label: {
                    Text("Open System Settings", bundle: .module)
                }
            }
        } header: {
            Text("Overview", bundle: .module)
        }

        Section {
            ForEach(model.diagnostics.items) { item in
                DiagnosticDisclosureCard(item: item)
                    .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
            }
        } header: {
            Text("Details", bundle: .module)
        }
    }

    @ViewBuilder
    private var helpContent: some View {
        Section {
            LabeledContent {
                Text(appVersionString)
            } label: {
                Text("Version", bundle: .module)
            }
            LabeledContent {
                Text(bundleIdentifierString)
            } label: {
                Text("Bundle", bundle: .module)
            }

            Button {
                openReleaseNotes()
            } label: {
                Text("Open release notes on GitHub", bundle: .module)
            }
            .disabled(!canOpenReleaseNotes)
        } header: {
            Text("About Open Whisper", bundle: .module)
        }

        Section {
            Text(model.microphonePermissionSummary)
                .font(.callout)
                .foregroundStyle(.secondary)

            Button {
                model.checkAndRequestMicrophoneAccess()
            } label: {
                Text("Check microphone access", bundle: .module)
            }
        } header: {
            Text("Microphone permission", bundle: .module)
        }

        Section {
            Text(model.accessibilityPermissionSummary)
                .font(.callout)
                .foregroundStyle(.secondary)

            Button {
                model.checkAndRequestAccessibilityAccess()
            } label: {
                Text("Check accessibility access", bundle: .module)
            }
            Button(role: .destructive) {
                isConfirmingAccessibilityReset = true
            } label: {
                Text("Reset accessibility permission", bundle: .module)
            }
        } header: {
            Text("Accessibility permission", bundle: .module)
        } footer: {
            Text("If text insertion stops working even though Open Whisper is listed under Accessibility, reset the permission and add the app again.", bundle: .module)
        }

        Section {
            Text("You can restart the setup assistant anytime to reconfigure microphone, hotkey, and language models.", bundle: .module)
                .font(.callout)
                .foregroundStyle(.secondary)

            Button {
                onReopenOnboarding()
            } label: {
                Text("Restart onboarding", bundle: .module)
            }
        } header: {
            Text("Setup", bundle: .module)
        }

        Section {
            Text("Open Whisper writes events and errors to a log file. Attach it when reporting problems.", bundle: .module)
                .font(.callout)
                .foregroundStyle(.secondary)

            Button {
                revealLogFileInFinder()
            } label: {
                Text("Show log file in Finder", bundle: .module)
            }
            Button {
                copyRecentLogToClipboard()
            } label: {
                Text("Copy recent log to clipboard", bundle: .module)
            }
        } header: {
            Text("Diagnostics", bundle: .module)
        }
    }

    private func revealLogFileInFinder() {
        guard let path = try? BridgeClient().getLogPath() else { return }
        let url = URL(fileURLWithPath: path)
        if FileManager.default.fileExists(atPath: path) {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        } else {
            NSWorkspace.shared.activateFileViewerSelecting([url.deletingLastPathComponent()])
        }
    }

    /// Copies the last 500 log lines so they can be pasted into a bug report.
    private func copyRecentLogToClipboard() {
        guard let path = try? BridgeClient().getLogPath(),
              let content = try? String(contentsOfFile: path, encoding: .utf8)
        else { return }
        let tail = content
            .split(separator: "\n", omittingEmptySubsequences: false)
            .suffix(500)
            .joined(separator: "\n")
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(tail, forType: .string)
    }

    private var appVersionString: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "—"
    }

    private var bundleIdentifierString: String {
        Bundle.main.bundleIdentifier ?? "—"
    }

    private var canOpenReleaseNotes: Bool {
        appVersionString != "—" && appVersionString != "0.0.0"
    }

    private func openReleaseNotes() {
        guard canOpenReleaseNotes,
              let url = URL(string: "https://github.com/mahype/open-whisper/releases/tag/v\(appVersionString)")
        else { return }
        NSWorkspace.shared.open(url)
    }

    private var bottomBar: some View {
        HStack(spacing: 12) {
            HStack(spacing: 6) {
                Circle()
                    .fill(runtimeAccent)
                    .frame(width: 8, height: 8)
                Text(model.bridgeError ?? runtimeLabel)
                    .font(.callout)
                    .foregroundStyle(model.bridgeError == nil ? Color.primary : Color.red)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }

            Spacer()

            Button {
                model.toggleDictation()
            } label: {
                Text(model.runtime.isRecording ? "Stop" : "Start dictation", bundle: .module)
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 10)
        .background(.regularMaterial)
    }

    private var deviceNames: [String] {
        var names = model.devices.map(\.name)
        if names.isEmpty {
            return [model.settings.inputDeviceName]
        }
        let saved = model.settings.inputDeviceName
        if !saved.isEmpty && !names.contains(saved) {
            names.insert(saved, at: 0)
        }
        return names
    }

    private var runtimeLabel: String {
        if model.runtime.isRecording {
            return L("Recording active", locale: locale)
        }
        if model.runtime.isPostProcessing {
            return L("Post-processing in progress", locale: locale)
        }
        if model.runtime.isTranscribing {
            return L("Transcription in progress", locale: locale)
        }
        return model.runtime.lastStatus.isEmpty ? L("Ready", locale: locale) : model.runtime.lastStatus
    }

    private var runtimeAccent: Color {
        if model.bridgeError != nil {
            return .red
        }
        if model.runtime.isRecording {
            return .red
        }
        if model.runtime.isPostProcessing {
            return .purple
        }
        if model.runtime.isTranscribing {
            return .orange
        }
        return .green
    }
}
