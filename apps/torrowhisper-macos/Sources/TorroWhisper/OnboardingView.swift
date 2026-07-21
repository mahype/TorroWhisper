import SwiftUI

struct OnboardingView: View {
    @ObservedObject var model: AppModel
    let onFinish: () -> Void
    @Environment(\.locale) private var locale

    @State private var testBaselineSuccessCount: UInt64 = 0
    @State private var testBaselineErrorCount: UInt64 = 0
    @State private var recordingTestAttempted = false
    @State private var recordingTestPassed = false
    @State private var recordingTestError: String?
    @State private var automaticInsertionBeforeTest: Bool?
    @State private var testText = ""
    @FocusState private var testFieldFocused: Bool

    private static let transcriptionStep = 1
    private static let testStep = 3
    private static let lastStep = testStep

    var body: some View {
        HStack(spacing: 0) {
            StepRail(currentStep: model.onboardingStep)
                .frame(width: 200)

            VStack(spacing: 0) {
                Form {
                    currentStep
                }
                .formStyle(.grouped)
                .navigationTitle(stepTitle)

                footer
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(width: 760, height: 520)
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            model.applyOnboardingLanguageDefault()
            model.refreshDevices()
            if model.onboardingStep == Self.transcriptionStep {
                prepareDefaultTranscriptionModel()
            }
            if model.onboardingStep == Self.testStep {
                prepareRecordingTest()
            }
        }
        .onChange(of: model.onboardingStep) { _, newStep in
            if newStep == Self.transcriptionStep {
                prepareDefaultTranscriptionModel()
            }
            if newStep == Self.testStep {
                prepareRecordingTest()
            }
        }
        .onChange(of: model.settings.inputDeviceName) { _, _ in invalidateRecordingTest() }
        .onChange(of: model.settings.transcriptionLanguage) { _, _ in invalidateRecordingTest() }
        .onChange(of: model.settings.hotkey) { _, _ in invalidateRecordingTest() }
        .onChange(of: model.settings.triggerMode) { _, _ in invalidateRecordingTest() }
        .onChange(of: model.runtime.isRecording) { _, isRecording in
            if model.onboardingStep == Self.testStep, isRecording {
                recordingTestAttempted = true
                recordingTestError = nil
            }
        }
        .onChange(of: model.runtime.dictationSuccessCount) { _, count in
            guard model.onboardingStep == Self.testStep,
                  recordingTestAttempted,
                  count > testBaselineSuccessCount
            else { return }
            recordingTestPassed = true
            recordingTestError = nil
        }
        .onChange(of: model.runtime.dictationErrorCount) { _, count in
            guard model.onboardingStep == Self.testStep,
                  recordingTestAttempted,
                  count > testBaselineErrorCount
            else { return }
            recordingTestPassed = false
            recordingTestError = model.runtime.lastDictationError
        }
        .onChange(of: model.bridgeError) { _, error in
            guard model.onboardingStep == Self.testStep,
                  recordingTestAttempted,
                  let error,
                  !error.isEmpty
            else { return }
            recordingTestPassed = false
            recordingTestError = error
        }
        .onDisappear {
            if dictationBusy {
                model.cancelDictation()
            }
            restoreAutomaticInsertionAfterTest()
        }
    }

    private var stepTitle: String {
        switch model.onboardingStep {
        case 0: return L("Recording", locale: locale)
        case Self.transcriptionStep: return L("Transcription", locale: locale)
        case 2: return L("Start & behavior", locale: locale)
        default: return L("Ready", locale: locale)
        }
    }

    @ViewBuilder
    private var currentStep: some View {
        switch model.onboardingStep {
        case 0:
            recordingSetup
        case Self.transcriptionStep:
            transcriptionSetup
        case 2:
            behaviorSetup
        default:
            recordingTest
        }
    }

    private var recordingSetup: some View {
        Group {
            Section {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    if deviceNames.count > 1 {
                        Picker(selection: microphoneBinding) {
                            ForEach(deviceNames, id: \.self) { device in
                                Text(device).tag(device)
                            }
                        } label: {
                            Text("Microphone", bundle: .module)
                        }
                    } else {
                        LabeledContent {
                            Text(selectedMicrophoneName ?? L("No microphone found", locale: locale))
                                .foregroundStyle(deviceNames.isEmpty ? .secondary : .primary)
                        } label: {
                            Text("Microphone", bundle: .module)
                        }
                    }

                    Button {
                        model.refreshDevices()
                    } label: {
                        Label("Reload microphones", systemImage: "arrow.clockwise")
                    }
                    .controlSize(.small)
                }

                Picker(selection: model.languageBinding()) {
                    ForEach(model.availableLanguageOptions) { option in
                        Text(option.label(locale: locale)).tag(option.code)
                    }
                } label: {
                    Text("Default language", bundle: .module)
                }
            } header: {
                Text("Audio source", bundle: .module)
            } footer: {
                Text("NVIDIA Parakeet supports 25 European languages and detects the spoken language automatically. TorroWhisper selects your first supported macOS language by default.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Section {
                permissionRow(
                    granted: model.microphoneAuthorizationStatus == .authorized,
                    grantedKey: "Microphone access granted.",
                    pendingKey: "Microphone access is not granted yet.",
                    buttonKey: "Grant microphone access",
                    action: { model.checkAndRequestMicrophoneAccess() }
                )
            } header: {
                Text("Microphone permission", bundle: .module)
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
                Text("Trigger & hotkey", bundle: .module)
            }
        }
    }

    private var transcriptionSetup: some View {
        Section {
            LabeledContent {
                Text(model.selectedModelDisplayName)
            } label: {
                Text("Transcription model", bundle: .module)
            }

            if model.settings.transcriptionBackend == .parakeet {
                if model.parakeetStatus.isPreparing {
                    ProgressView()
                        .controlSize(.small)
                }
                LabeledContent {
                    Text(L(model.parakeetStatus.summary, locale: locale))
                } label: {
                    Text("Status", bundle: .module)
                }

                if let error = model.parakeetStatus.error {
                    Text(error)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .fixedSize(horizontal: false, vertical: true)
                    Button {
                        model.prepareParakeet()
                    } label: {
                        Text("Try again", bundle: .module)
                    }
                }
            } else if let status = currentWhisperStatus {
                if status.isDownloading, let basisPoints = status.progressBasisPoints {
                    ProgressView(value: Double(basisPoints) / 10_000.0)
                }
                Text(L(status.summary, locale: locale))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if !status.isDownloaded && !status.isDownloading {
                    Button {
                        model.startModelDownload()
                    } label: {
                        Text("Try again", bundle: .module)
                    }
                }
            }
        } header: {
            Text("Transcription", bundle: .module)
        } footer: {
            Text("Required. NVIDIA Parakeet is downloaded and prepared automatically when this page opens. Other transcription and post-processing models can be configured later in Settings.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var behaviorSetup: some View {
        Group {
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
                Text("Needed so TorroWhisper can type the transcribed text into other apps.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                Toggle(isOn: model.launchAtLoginBinding) {
                    Text("Launch at login", bundle: .module)
                }
            } header: {
                Text("System startup", bundle: .module)
            }
        }
    }

    private var recordingTest: some View {
        Section {
            VStack(spacing: 16) {
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 64, weight: .regular))
                    .symbolRenderingMode(.hierarchical)
                    .foregroundStyle(.green)
                    .accessibilityHidden(true)

                VStack(spacing: 4) {
                    Text("Everything is set up.", bundle: .module)
                        .font(.title2.weight(.semibold))
                    Text("You're ready to dictate. Try your shortcut once before you get started.", bundle: .module)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }

                VStack(spacing: 4) {
                    Text("Your shortcut", bundle: .module)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(hotkeyDisplayString(model.settings.hotkey))
                        .font(.title3.weight(.semibold).monospaced())
                        .textSelection(.enabled)
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text(testInstructionKey, bundle: .module)
                        .font(.callout)
                        .fixedSize(horizontal: false, vertical: true)

                    ZStack(alignment: .topLeading) {
                        if testText.isEmpty {
                            Text("Your dictated text will appear here.", bundle: .module)
                                .foregroundStyle(.tertiary)
                                .padding(.horizontal, 5)
                                .padding(.vertical, 8)
                                .allowsHitTesting(false)
                        }

                        TextEditor(text: $testText)
                            .focused($testFieldFocused)
                            .font(.body)
                            .scrollContentBackground(.hidden)
                            .padding(.horizontal, 1)
                    }
                    .frame(minHeight: 78, maxHeight: 100)
                    .padding(6)
                    .background(Color(nsColor: .textBackgroundColor))
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .overlay {
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .stroke(
                                testFieldFocused ? Color.torroAccent : Color(nsColor: .separatorColor),
                                lineWidth: testFieldFocused ? 2 : 1
                            )
                    }

                    recordingTestFeedback
                }
                .frame(maxWidth: 440, alignment: .leading)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 4)
        }
    }

    @ViewBuilder
    private var recordingTestFeedback: some View {
        if recordingTestPassed {
            Label {
                Text("It works — your dictation was inserted successfully.", bundle: .module)
            } icon: {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            }
        } else if model.runtime.isRecording {
            Label {
                Text("Speak now…", bundle: .module)
            } icon: {
                Image(systemName: "waveform.circle.fill")
                    .foregroundStyle(.green)
            }
        } else if model.runtime.isTranscribing || model.runtime.isPostProcessing {
            HStack(spacing: 8) {
                ProgressView()
                    .controlSize(.small)
                Text("Transcribing test recording…", bundle: .module)
                    .foregroundStyle(.secondary)
            }
        } else if let recordingTestError, !recordingTestError.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Label {
                    Text("Recording test failed.", bundle: .module)
                } icon: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.red)
                }
                Text(recordingTestError)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        } else {
            Text("Complete this short test to finish setup.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var testInstructionKey: LocalizedStringKey {
        switch model.settings.triggerMode {
        case .toggle:
            return "Click in the field, press the shortcut, dictate a short sentence, then press it again."
        case .pushToTalk:
            return "Click in the field, hold the shortcut while dictating a short sentence, then release it."
        }
    }

    private var footer: some View {
        HStack(spacing: 12) {
            if let problem = footerProblem {
                Label {
                    Text(problem, bundle: .module)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } icon: {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(.orange)
                }
                .lineLimit(2)
            }

            Spacer()

            Button {
                if model.onboardingStep == Self.testStep {
                    restoreAutomaticInsertionAfterTest()
                }
                model.onboardingStep = max(0, model.onboardingStep - 1)
            } label: {
                Text("Back", bundle: .module)
            }
            .disabled(model.onboardingStep == 0 || dictationBusy)

            if model.onboardingStep == Self.lastStep {
                Button {
                    restoreAutomaticInsertionAfterTest()
                    if model.completeOnboarding() {
                        onFinish()
                    }
                } label: {
                    Text("Get started", bundle: .module)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(
                    !recordingTestPassed
                        || !model.requiredOnboardingPermissionsGranted
                        || dictationBusy
                )
            } else {
                Button {
                    advanceToNextStep()
                } label: {
                    Text("Next", bundle: .module)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(!currentStepComplete || dictationBusy)
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .background(.regularMaterial)
    }

    private var currentStepComplete: Bool {
        switch model.onboardingStep {
        case 0: return recordingConfigurationComplete
        case Self.transcriptionStep:
            return transcriptionReady && model.microphoneAuthorizationStatus == .authorized
        case 2:
            return model.requiredOnboardingPermissionsGranted
        default:
            return recordingTestPassed && model.requiredOnboardingPermissionsGranted
        }
    }

    private var recordingConfigurationComplete: Bool {
        !deviceNames.isEmpty
            && !model.settings.inputDeviceName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && TranscriptionLanguageOption.option(for: model.settings.transcriptionLanguage) != nil
            && !model.settings.hotkey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !model.isCapturingHotkey
            && model.hotkeyCaptureError == nil
            && model.microphoneAuthorizationStatus == .authorized
    }

    private var footerProblem: LocalizedStringKey? {
        switch model.onboardingStep {
        case 0:
            if deviceNames.isEmpty { return "Connect or reload a microphone to continue." }
            if model.microphoneAuthorizationStatus != .authorized { return "Grant microphone access to continue." }
            if model.settings.hotkey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                || model.isCapturingHotkey
                || model.hotkeyCaptureError != nil {
                return "Set a valid global hotkey to continue."
            }
            return nil
        case Self.transcriptionStep:
            if model.microphoneAuthorizationStatus != .authorized {
                return "Grant microphone access to continue."
            }
            return transcriptionReady ? nil : "Wait until the transcription model is ready."
        case 2:
            if model.microphoneAuthorizationStatus != .authorized {
                return "Grant microphone access to continue."
            }
            return model.accessibilityTrusted ? nil : "Grant accessibility access to continue."
        default:
            if model.microphoneAuthorizationStatus != .authorized {
                return "Grant microphone access to continue."
            }
            if !model.accessibilityTrusted {
                return "Grant accessibility access to continue."
            }
            return recordingTestPassed ? nil : "Complete a successful recording test to finish setup."
        }
    }

    private var dictationBusy: Bool {
        model.runtime.isRecording || model.runtime.isTranscribing || model.runtime.isPostProcessing
    }

    private func advanceToNextStep() {
        guard currentStepComplete, model.saveSettings() else { return }
        model.onboardingStep = min(Self.lastStep, model.onboardingStep + 1)
    }

    private func prepareDefaultTranscriptionModel() {
        model.prepareDefaultTranscriptionModelForOnboarding()
    }

    private func prepareRecordingTest() {
        if automaticInsertionBeforeTest == nil {
            automaticInsertionBeforeTest = model.settings.insertTextAutomatically
            if !model.settings.insertTextAutomatically {
                model.settings.insertTextAutomatically = true
                _ = model.saveSettings()
            }
        }
        testBaselineSuccessCount = model.runtime.dictationSuccessCount
        testBaselineErrorCount = model.runtime.dictationErrorCount
        recordingTestAttempted = false
        recordingTestPassed = false
        recordingTestError = nil
        testText = ""
        testFieldFocused = false
    }

    private func invalidateRecordingTest() {
        guard recordingTestAttempted || recordingTestPassed else { return }
        prepareRecordingTest()
    }

    /// The final test temporarily enables insertion so the user can see the
    /// result in the test field. Their prior Settings choice is restored when
    /// leaving setup.
    private func restoreAutomaticInsertionAfterTest() {
        guard let previous = automaticInsertionBeforeTest else { return }
        automaticInsertionBeforeTest = nil
        guard model.settings.insertTextAutomatically != previous else { return }
        model.settings.insertTextAutomatically = previous
        _ = model.saveSettings()
    }

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
        model.devices.map(\.name)
    }

    private var selectedMicrophoneName: String? {
        if deviceNames.contains(model.settings.inputDeviceName) {
            return model.settings.inputDeviceName
        }
        return model.devices.first(where: \.isSelected)?.name ?? deviceNames.first
    }

    private var microphoneBinding: Binding<String> {
        Binding(
            get: { selectedMicrophoneName ?? "" },
            set: { newValue in
                model.settings.inputDeviceName = newValue
                model.requestAutoSave()
            }
        )
    }

    private var currentWhisperStatus: ModelStatusDTO? {
        if model.modelStatusList.isEmpty {
            return model.modelStatus
        }
        return model.modelStatusList.first {
            $0.backendModelName == model.settings.localModel.whisperModel
        }
    }

    private var transcriptionReady: Bool {
        switch model.settings.transcriptionBackend {
        case .parakeet: return model.parakeetStatus.isReady
        case .whisper: return currentWhisperStatus?.isDownloaded == true
        }
    }
}
