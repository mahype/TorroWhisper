import SwiftUI

struct OnboardingView: View {
    @ObservedObject var model: AppModel
    let onFinish: () -> Void
    @Environment(\.locale) private var locale

    @State private var isManagingLanguageModels = false
    @State private var managerTab: LanguageModelsManagerTab = .postProcessing

    private static let lastStep = 5

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
        .sheet(isPresented: $isManagingLanguageModels) {
            LanguageModelsManagerSheet(model: model, selectedTab: $managerTab) {
                isManagingLanguageModels = false
            }
        }
    }

    private var stepTitle: String {
        switch model.onboardingStep {
        case 0: return L("Welcome", locale: locale)
        case 1: return L("Audio & hotkey", locale: locale)
        case 2: return L("Permissions", locale: locale)
        case 3: return L("Language models", locale: locale)
        case 4: return L("Start & behavior", locale: locale)
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
                permissionRow(
                    granted: model.microphoneAuthorizationStatus == .authorized,
                    grantedKey: "Microphone access granted.",
                    pendingKey: "Microphone access is not granted yet.",
                    buttonKey: "Grant microphone access",
                    action: { model.checkAndRequestMicrophoneAccess() }
                )
            } header: {
                Text("Microphone", bundle: .module)
            } footer: {
                Text("Required to record your dictation.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                permissionRow(
                    granted: model.accessibilityTrusted,
                    grantedKey: "Accessibility access granted.",
                    pendingKey: "Accessibility access is not granted yet.",
                    buttonKey: "Grant accessibility access",
                    action: { model.checkAndRequestAccessibilityAccess() }
                )
            } header: {
                Text("Accessibility", bundle: .module)
            } footer: {
                Text("Needed so Open Whisper can type the transcribed text into other apps.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                Button {
                    model.refreshDiagnostics()
                } label: {
                    Text("Check again", bundle: .module)
                }
            } footer: {
                Text("After granting a permission in System Settings, tap 'Check again' to refresh the status here.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        case 3:
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

                whisperDownloadControl
            } header: {
                Text("Transcription", bundle: .module)
            } footer: {
                Text("Required. Download the selected transcription model to continue.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                if hasPostProcessingModel {
                    Label {
                        Text("A post-processing model is installed.", bundle: .module)
                    } icon: {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundStyle(.green)
                    }
                } else {
                    Text("No post-processing model installed yet.", bundle: .module)
                        .font(.callout.weight(.medium))
                    Button {
                        managerTab = .postProcessing
                        isManagingLanguageModels = true
                    } label: {
                        Label {
                            Text("Choose a post-processing model", bundle: .module)
                        } icon: {
                            Image(systemName: "arrow.down.circle")
                        }
                    }
                }
            } header: {
                Text("Post-processing", bundle: .module)
            } footer: {
                Text("Optional — only needed if you want post-processing. It runs your dictation through a language model to clean it up: punctuation, capitalization, and filler-word removal. Choose a model now if you want it, or just continue and add one later in Settings.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        case 4:
            Section {
                Toggle(isOn: model.launchAtLoginBinding) {
                    Text("Launch at login", bundle: .module)
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

            if model.onboardingStep == Self.lastStep {
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
                    model.onboardingStep = min(Self.lastStep, model.onboardingStep + 1)
                } label: {
                    Text("Next", bundle: .module)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(model.onboardingStep == 3 && !whisperReady)
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .background(.regularMaterial)
    }

    /// One permission entry: a green "granted" confirmation once the access is
    /// in place, otherwise a short note plus the button that requests it (or
    /// deep-links into System Settings when already denied).
    @ViewBuilder
    private func permissionRow(
        granted: Bool,
        grantedKey: LocalizedStringKey,
        pendingKey: LocalizedStringKey,
        buttonKey: LocalizedStringKey,
        action: @escaping () -> Void
    ) -> some View {
        if granted {
            Label {
                Text(grantedKey, bundle: .module)
            } icon: {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            }
        } else {
            Text(pendingKey, bundle: .module)
                .font(.callout)
                .foregroundStyle(.secondary)
            Button(action: action) {
                Text(buttonKey, bundle: .module)
            }
        }
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

    /// Whether any post-processing (LLM) model is already downloaded. Drives the
    /// optional hint on the model step — no LLM is offered for download in
    /// onboarding anymore; the user is pointed at the language-models manager
    /// instead.
    private var hasPostProcessingModel: Bool {
        model.llmStatusList.contains { $0.isDownloaded }
    }

    /// Gating for the model step: only the transcription model is mandatory.
    /// Post-processing stays optional, so it never blocks the wizard.
    private var whisperReady: Bool {
        currentWhisperStatus?.isDownloaded ?? false
    }

    /// Manual download control for the transcription model. Nothing downloads
    /// automatically — the user must click, and cannot continue until the
    /// selected model is downloaded.
    @ViewBuilder
    private var whisperDownloadControl: some View {
        let status = currentWhisperStatus
        if status?.isDownloaded ?? false {
            Label {
                Text("Downloaded", bundle: .module)
            } icon: {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            }
        } else {
            Button {
                model.startModelDownload(preset: model.settings.localModel)
            } label: {
                Text((status?.isDownloading ?? false) ? "Loading…" : "Download", bundle: .module)
            }
            .disabled(status?.isDownloading ?? false)
        }
    }
}
