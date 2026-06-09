import SwiftUI

struct OnboardingView: View {
    @ObservedObject var model: AppModel
    let onFinish: () -> Void
    @Environment(\.locale) private var locale

    var body: some View {
        HStack(spacing: 0) {
            StepRail(currentStep: model.onboardingStep)
                .frame(width: 200)

            VStack(spacing: 0) {
                Form {
                    currentStep
                }
                .formStyle(.grouped)
                .scrollDisabled(true)
                .navigationTitle(stepTitle)

                footer
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(width: 760, height: 520)
        .background(Color(nsColor: .windowBackgroundColor))
        .onChange(of: model.onboardingStep) {
            if model.onboardingStep == 2 {
                triggerModelDownloadsIfNeeded()
            }
        }
        .onChange(of: model.settings.localModel) {
            if model.onboardingStep == 2 {
                triggerModelDownloadsIfNeeded()
            }
        }
        .onChange(of: model.settings.localLlm) {
            if model.onboardingStep == 2 {
                triggerModelDownloadsIfNeeded()
            }
        }
    }

    private var stepTitle: String {
        switch model.onboardingStep {
        case 0: return L("Welcome", locale: locale)
        case 1: return L("Audio & hotkey", locale: locale)
        case 2: return L("Language models", locale: locale)
        case 3: return L("Start & behavior", locale: locale)
        default: return L("Diagnostics", locale: locale)
        }
    }

    @ViewBuilder
    private var currentStep: some View {
        switch model.onboardingStep {
        case 0:
            Section {
                Text("Tray-first, local, and built for everyday use. Default is local Whisper; Ollama and LM Studio stay optional.", bundle: .module)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } header: {
                Text("Open Whisper", bundle: .module)
            }

            Section {
                LabeledContent {
                    Text(model.settings.inputDeviceName)
                } label: {
                    Text("Microphone", bundle: .module)
                }
                LabeledContent {
                    Text(hotkeyDisplayString(model.settings.hotkey))
                } label: {
                    Text("Hotkey", bundle: .module)
                }
                LabeledContent {
                    Text(model.selectedModelDisplayName)
                } label: {
                    Text("Model", bundle: .module)
                }
                LabeledContent {
                    Text(model.settings.startupBehavior.label(locale: locale))
                } label: {
                    Text("System startup", bundle: .module)
                }
            } header: {
                Text("Current selection", bundle: .module)
            }
        case 1:
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
                    Text("Reload microphones", bundle: .module)
                }
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
        case 2:
            Section {
                Picker(selection: model.binding(for: \.localModel)) {
                    ForEach(ModelPreset.allCases) { preset in
                        Text(preset.displayName).tag(preset)
                    }
                } label: {
                    Text("Whisper model", bundle: .module)
                }

                Text("\(model.settings.localModel.description(locale: locale)) (\(model.settings.localModel.downloadSizeText))")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if let status = currentWhisperStatus {
                    if status.isDownloading, let basisPoints = status.progressBasisPoints {
                        ProgressView(value: Double(basisPoints) / 10_000.0)
                    }
                    LabeledContent {
                        Text(status.summary)
                    } label: {
                        Text("Status", bundle: .module)
                    }
                }
            } header: {
                Text("Transcription", bundle: .module)
            }

            Section {
                Picker(selection: model.binding(for: \.localLlm)) {
                    ForEach(LlmPreset.allCases) { preset in
                        Text(preset.displayName).tag(preset)
                    }
                } label: {
                    Text("Language model", bundle: .module)
                }

                Text("\(model.settings.localLlm.description(locale: locale)) (\(model.settings.localLlm.approxSizeLabel))")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if let status = currentLlmStatus {
                    if status.isDownloading, let basisPoints = status.progressBasisPoints {
                        ProgressView(value: Double(basisPoints) / 10_000.0)
                    }
                    LabeledContent {
                        Text(status.summary)
                    } label: {
                        Text("Status", bundle: .module)
                    }
                }
            } header: {
                Text("Post-processing", bundle: .module)
            }

            Section {
                Text("Both models download now and must finish before you can continue. You can add or switch to other models later in Settings under 'Language models'.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        case 3:
            Section {
                Picker(selection: model.binding(for: \.startupBehavior)) {
                    ForEach(StartupBehavior.allCases) { behavior in
                        Text(behavior.label(locale: locale)).tag(behavior)
                    }
                } label: {
                    Text("System startup", bundle: .module)
                }
            } header: {
                Text("System startup", bundle: .module)
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
                Toggle(isOn: model.binding(for: \.vadEnabled)) {
                    Text("Enable silence stop", bundle: .module)
                }
            } header: {
                Text("Dictation stop", bundle: .module)
            }
        default:
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
    }

    private var footer: some View {
        HStack {
            Button {
                model.onboardingStep = max(0, model.onboardingStep - 1)
            } label: {
                Text("Back", bundle: .module)
            }
            .disabled(model.onboardingStep == 0)

            Spacer()

            if model.onboardingStep == 4 {
                Button {
                    if model.completeOnboarding() {
                        onFinish()
                    }
                } label: {
                    Text("Finish", bundle: .module)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
            } else {
                Button {
                    model.onboardingStep = min(4, model.onboardingStep + 1)
                } label: {
                    Text("Next", bundle: .module)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(model.onboardingStep == 2 && !modelsReady)
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
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

    private var currentWhisperStatus: ModelStatusDTO? {
        if model.modelStatusList.isEmpty {
            return model.modelStatus
        }
        return model.modelStatusList.first { $0.backendModelName == model.settings.localModel.whisperModel }
    }

    private var currentLlmStatus: LlmModelStatusDTO? {
        model.llmStatusList.first { $0.displayLabel == model.settings.localLlm.displayName }
    }

    private var modelsReady: Bool {
        (currentWhisperStatus?.isDownloaded ?? false) && (currentLlmStatus?.isDownloaded ?? false)
    }

    private func triggerModelDownloadsIfNeeded() {
        let whisperDownloaded = currentWhisperStatus?.isDownloaded ?? false
        let whisperDownloading = currentWhisperStatus?.isDownloading ?? false
        if !whisperDownloaded && !whisperDownloading {
            model.startModelDownload(preset: model.settings.localModel)
        }

        let llmDownloaded = currentLlmStatus?.isDownloaded ?? false
        let llmDownloading = currentLlmStatus?.isDownloading ?? false
        if !llmDownloaded && !llmDownloading {
            model.startLlmDownload(preset: model.settings.localLlm)
        }
    }
}
