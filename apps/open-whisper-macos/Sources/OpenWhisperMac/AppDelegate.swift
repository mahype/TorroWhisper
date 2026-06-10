import AppKit
import Carbon.HIToolbox
import SwiftUI

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate, NSWindowDelegate {
    let model = AppModel()
    let updaterController = UpdaterController()

    private var statusItem: NSStatusItem!
    private let statusMenu = NSMenu()
    private var dictationItem: NSMenuItem!
    private var settingsItem: NSMenuItem!
    private var modeSwitchItem: NSMenuItem!
    private var modelSwitchItem: NSMenuItem!
    private var micSwitchItem: NSMenuItem!
    private var recentHistoryItem: NSMenuItem!
    private var statusItemLine: NSMenuItem!
    private var quitItem: NSMenuItem!
    private var checkForUpdatesItem: NSMenuItem!
    private var feedbackItem: NSMenuItem!
    private var settingsWindow: NSWindow?
    private var onboardingWindow: NSWindow?
    private var feedbackWindow: NSWindow?
    private var recordingIndicatorWindow: NSWindow?
    private var micSwitchToastWindow: NSPanel?
    private var micSwitchToastDismissTask: Task<Void, Never>?
    private var lastAnnouncedPhaseKey: String?
    private let audioDeviceMonitor = AudioDeviceMonitor()
    private let keyboardHardwareMonitor = KeyboardHardwareMonitor()
    private let recordingLevelFeed = RecordingLevelFeed()
    private let modeMenu = NSMenu()
    private let modelMenu = NSMenu()
    private let micMenu = NSMenu()
    private let recentHistoryMenu = NSMenu()
    private var escapeHotKeyRef: EventHotKeyRef?
    private var escapeEventHandler: EventHandlerRef?
    private var escapeLocalMonitor: Any?
    private static let escapeKeyCode: UInt16 = 53
    // "OWES" — Open Whisper Escape
    private static let escapeHotKeySignature: OSType = 0x4F57_4553
    private static let escapeHotKeyID: UInt32 = 1

    private var currentLocale: Locale { model.settings.effectiveLocale }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.imagePosition = .imageOnly
        statusItem.button?.toolTip = "Open Whisper"

        dictationItem = NSMenuItem(title: "", action: #selector(toggleDictation), keyEquivalent: "")
        settingsItem = NSMenuItem(title: "", action: #selector(showSettings), keyEquivalent: ",")
        modeSwitchItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        modeSwitchItem.submenu = modeMenu
        modelSwitchItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        modelSwitchItem.submenu = modelMenu
        micSwitchItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        micSwitchItem.submenu = micMenu
        recentHistoryItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        recentHistoryItem.submenu = recentHistoryMenu
        statusItemLine = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        statusItemLine.isEnabled = false
        quitItem = NSMenuItem(title: "", action: #selector(quitApp), keyEquivalent: "q")
        checkForUpdatesItem = NSMenuItem(
            title: "",
            action: #selector(checkForUpdates),
            keyEquivalent: ""
        )
        feedbackItem = NSMenuItem(
            title: "",
            action: #selector(showFeedback),
            keyEquivalent: ""
        )

        statusMenu.delegate = self
        statusMenu.items = [
            dictationItem,
            .separator(),
            settingsItem,
            .separator(),
            micSwitchItem,
            modeSwitchItem,
            modelSwitchItem,
            recentHistoryItem,
            statusItemLine,
            .separator(),
            feedbackItem,
            checkForUpdatesItem,
            .separator(),
            quitItem,
        ]
        statusItem.menu = statusMenu

        model.onStateChanged = { [weak self] in
            self?.refreshMenuState()
        }
        model.onMicSwitched = { [weak self] notification in
            self?.showMicSwitchToast(notification)
        }
        refreshMenuState()

        audioDeviceMonitor.onDevicesChanged = { [weak self] in
            self?.model.notifyDeviceListChanged()
        }
        audioDeviceMonitor.start()

        keyboardHardwareMonitor.onKeyboardChanged = { [weak self] in
            self?.model.reregisterHotkey()
        }
        keyboardHardwareMonitor.start()

        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.model.refreshDiagnostics()
            if !self.model.runtime.onboardingCompleted
                && !self.model.settings.onboardingCompleted {
                self.showOnboarding(nil)
            }
        }
    }

    func menuWillOpen(_ menu: NSMenu) {
        refreshMenuState()
    }

    @objc private func toggleDictation() {
        model.toggleDictation()
    }

    @objc private func showSettings(_ sender: Any?) {
        let window = settingsWindow ?? makeWindow(
            title: L("Open Whisper Settings", locale: currentLocale),
            size: NSSize(width: 820, height: 720),
            rootView: SettingsView(
                model: model,
                updaterController: updaterController,
                onReopenOnboarding: { [weak self] in
                    self?.showOnboarding(nil)
                }
            )
        )
        if settingsWindow == nil {
            window.delegate = self
        }
        settingsWindow = window
        show(window)
    }

    func windowWillClose(_ notification: Notification) {
        guard let window = notification.object as? NSWindow, window === settingsWindow else {
            return
        }
        model.flushAutoSave()
    }

    @objc private func showFeedback(_ sender: Any?) {
        let window = feedbackWindow ?? makeWindow(
            title: L("Send feedback", locale: currentLocale),
            size: NSSize(width: 460, height: 320),
            rootView: FeedbackView()
        )
        feedbackWindow = window
        show(window)
    }

    @objc private func showOnboarding(_ sender: Any?) {
        model.reopenOnboarding()
        let window = onboardingWindow ?? makeWindow(
            title: L("Open Whisper Setup", locale: currentLocale),
            size: NSSize(width: 760, height: 520),
            rootView: OnboardingView(model: model) { [weak self] in
                self?.onboardingWindow?.orderOut(nil)
            }
        )
        onboardingWindow = window
        show(window)
    }

    @objc private func quitApp() {
        NSApp.terminate(nil)
    }

    @objc private func checkForUpdates() {
        updaterController.checkForUpdates()
    }

    @objc private func selectMode(_ sender: NSMenuItem) {
        guard let modeID = sender.representedObject as? String else {
            return
        }
        model.persistActiveModeImmediately(modeID)
    }

    @objc private func disablePostProcessing(_ sender: Any?) {
        model.persistPostProcessingEnabledImmediately(false)
    }

    @objc private func selectWhisperPreset(_ sender: NSMenuItem) {
        guard
            let raw = sender.representedObject as? String,
            let preset = ModelPreset(rawValue: raw)
        else { return }
        model.persistWhisperPresetImmediately(preset)
    }

    @objc private func selectInputDevice(_ sender: NSMenuItem) {
        guard let name = sender.representedObject as? String else { return }
        model.persistInputDeviceImmediately(name)
    }

    private func refreshMenuState() {
        let runtime = model.runtime
        let locale = currentLocale
        let dictationLabel = runtime.isRecording
            ? L("Stop dictation", locale: locale)
            : L("Start dictation", locale: locale)
        let hotkeySuffix = runtime.hotkeyText.trimmingCharacters(in: .whitespaces)
        dictationItem.title = hotkeySuffix.isEmpty
            ? dictationLabel
            : "\(dictationLabel) — \(hotkeySuffix)"
        settingsItem.title = L("Settings…", locale: locale)
        modeSwitchItem.title = L("Post-processing", locale: locale)
        modelSwitchItem.title = L("Transcription model", locale: locale)
        micSwitchItem.title = L("Microphone", locale: locale)
        recentHistoryItem.title = L("Recent dictations", locale: locale)
        quitItem.title = L("Quit", locale: locale)
        checkForUpdatesItem.title = L("Check for updates…", locale: locale)
        feedbackItem.title = L("Send feedback…", locale: locale)
        rebuildModeMenu()
        rebuildModelMenu()
        rebuildMicMenu()
        rebuildRecentHistoryMenu()
        statusItemLine.title = model.bridgeError ?? runtime.lastStatus
        statusItem.button?.image = statusImage(recording: runtime.isRecording)
        statusItem.button?.toolTip = buildStatusTooltip(runtime: runtime)
        updateRecordingIndicatorVisibility()
        refreshWindowTitles()
        announceRuntimeTransition()
    }

    /// Speaks dictation phase changes to VoiceOver. The floating indicator panel
    /// is not in the accessibility tree, so screen-reader users would otherwise
    /// get no feedback that recording started, transcription finished, etc.
    private func announceRuntimeTransition() {
        let runtime = model.runtime
        let locale = currentLocale
        let key: String
        var announcement: String?

        if runtime.dictationBlockedByMissingModel {
            key = "blocked"
            announcement = L("Recording not possible. The model is missing.", locale: locale)
        } else if runtime.isRecording {
            key = "recording"
            announcement = L("Recording started.", locale: locale)
        } else if runtime.isTranscribing {
            key = "transcribing"
            announcement = L("Transcribing…", locale: locale)
        } else if runtime.isPostProcessing {
            key = "postProcessing"
            announcement = L("Post-processing…", locale: locale)
        } else {
            key = "idle"
            if let last = lastAnnouncedPhaseKey, last != "idle", last != "blocked" {
                let status = runtime.lastStatus.trimmingCharacters(in: .whitespaces)
                announcement = status.isEmpty ? L("Dictation finished.", locale: locale) : status
            }
        }

        guard key != lastAnnouncedPhaseKey else { return }
        lastAnnouncedPhaseKey = key
        if let announcement, !announcement.isEmpty {
            postAccessibilityAnnouncement(announcement)
        }
    }

    private func postAccessibilityAnnouncement(
        _ message: String,
        priority: NSAccessibilityPriorityLevel = .high
    ) {
        NSAccessibility.post(
            element: NSApp as Any,
            notification: .announcementRequested,
            userInfo: [
                .announcement: message,
                .priority: priority.rawValue,
            ]
        )
    }

    private func buildStatusTooltip(runtime: RuntimeStatusDTO) -> String {
        let base = model.bridgeError ?? runtime.lastStatus
        let mic = runtime.activeInputDeviceName
        guard !mic.isEmpty else { return base }
        return "\(base)\nMicrophone: \(mic)"
    }

    private func showMicSwitchToast(_ notification: MicSwitchNotification) {
        let message = notification.message.isEmpty
            ? "Microphone changed to '\(notification.activeDevice)'."
            : notification.message
        let window = micSwitchToastWindow ?? makeMicSwitchToastWindow(message: message)
        if let hosting = window.contentViewController as? NSHostingController<MicSwitchToastView> {
            hosting.rootView = MicSwitchToastView(message: message)
        }
        micSwitchToastWindow = window
        postAccessibilityAnnouncement(message)
        positionMicSwitchToastWindow(window)
        window.alphaValue = 0
        window.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.18
            window.animator().alphaValue = 1
        }

        micSwitchToastDismissTask?.cancel()
        micSwitchToastDismissTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 2_800_000_000)
            guard !Task.isCancelled else { return }
            await MainActor.run {
                guard let window = self?.micSwitchToastWindow else { return }
                NSAnimationContext.runAnimationGroup { context in
                    context.duration = 0.25
                    window.animator().alphaValue = 0
                } completionHandler: {
                    Task { @MainActor in
                        window.orderOut(nil)
                    }
                }
            }
        }
    }

    private func makeMicSwitchToastWindow(message: String) -> NSPanel {
        let size = NSSize(width: 340, height: 60)
        let panel = NSPanel(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isFloatingPanel = true
        panel.becomesKeyOnlyIfNeeded = true
        panel.level = .statusBar
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = false
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .fullScreenAuxiliary]
        panel.isReleasedWhenClosed = false

        let hosting = NSHostingController(rootView: MicSwitchToastView(message: message))
        hosting.view.frame = NSRect(origin: .zero, size: size)
        panel.contentViewController = hosting
        return panel
    }

    private func positionMicSwitchToastWindow(_ window: NSWindow) {
        guard let screenFrame = NSScreen.main?.visibleFrame else { return }
        let margin: CGFloat = 16
        let size = window.frame.size
        let topPadding: CGFloat = recordingIndicatorWindow?.isVisible == true ? 120 : 0
        let origin = NSPoint(
            x: screenFrame.midX - size.width / 2,
            y: screenFrame.maxY - size.height - margin - topPadding
        )
        window.setFrameOrigin(origin)
    }

    private func refreshWindowTitles() {
        let locale = currentLocale
        settingsWindow?.title = L("Open Whisper Settings", locale: locale)
        onboardingWindow?.title = L("Open Whisper Setup", locale: locale)
        feedbackWindow?.title = L("Send feedback", locale: locale)
    }

    private func updateRecordingIndicatorVisibility() {
        let runtime = model.runtime
        let phase: IndicatorPhase? = {
            if runtime.dictationBlockedByMissingModel {
                let progress = runtime.blockedModelProgressBasisPoints.map { Double($0) / 10_000.0 }
                return .modelNotReady(
                    label: runtime.blockedModelLabel,
                    progress: progress,
                    isDownloading: runtime.blockedModelIsDownloading
                )
            }
            if runtime.isRecording { return .recording }
            if runtime.isTranscribing { return .transcribing }
            if runtime.isPostProcessing { return .postProcessing }
            return nil
        }()

        guard model.settings.showRecordingIndicator, let phase else {
            recordingLevelFeed.stop()
            recordingIndicatorWindow?.orderOut(nil)
            removeEscapeMonitor()
            return
        }

        installEscapeMonitor()

        if phase == .recording {
            recordingLevelFeed.start()
        } else {
            recordingLevelFeed.stop()
        }

        let style = model.settings.waveformStyle
        let color = model.settings.waveformColor
        let modelSuffix: String? = {
            switch phase {
            case .recording, .transcribing:
                return model.selectedModelDisplayName
            case .postProcessing:
                return model.selectedPostProcessingDisplayName
            case .modelNotReady:
                return nil
            }
        }()
        let modelName = modelSuffix ?? ""
        let modeName: String? = model.settings.postProcessingEnabled ? model.activeModeName : nil
        let stopHotkeyHint: String = {
            guard phase == .recording else { return "" }
            // Compact symbol form (⌥⇧S) instead of "Option+Shift+S" to keep the
            // hint short across languages.
            let shortcut = hotkeyDisplayString(model.settings.hotkey)
                .replacingOccurrences(of: " ", with: "")
            let stop = String(format: L("Stop: %@", locale: currentLocale), shortcut)
            let cancel = L("Cancel: Esc", locale: currentLocale)
            return "\(stop) · \(cancel)"
        }()
        let onStop: () -> Void = { [weak self] in self?.model.toggleDictation() }
        let isCancelling = model.runtime.isCancelling
        let window = recordingIndicatorWindow ?? makeRecordingIndicatorWindow(phase: phase, style: style, color: color, modelName: modelName, modeName: modeName, stopHotkeyHint: stopHotkeyHint, isCancelling: isCancelling, onStop: onStop)
        recordingIndicatorWindow = window
        // The bubble normally ignores mouse events so it never steals clicks
        // from whatever is underneath. While recording we accept them so the
        // small stop button is clickable.
        window.ignoresMouseEvents = (phase != .recording)
        updateIndicatorPhase(window: window, phase: phase, style: style, color: color, modelName: modelName, modeName: modeName, stopHotkeyHint: stopHotkeyHint, isCancelling: isCancelling, onStop: onStop)
        positionRecordingIndicatorWindow(window)
        window.orderFrontRegardless()
    }

    private func updateIndicatorPhase(window: NSWindow, phase: IndicatorPhase, style: WaveformStyle, color: WaveformColor, modelName: String, modeName: String?, stopHotkeyHint: String, isCancelling: Bool, onStop: @escaping () -> Void) {
        guard let hosting = window.contentViewController as? NSHostingController<LocalizedRoot<RecordingIndicatorView>> else {
            return
        }
        let inner = hosting.rootView.content()
        if inner.phase != phase
            || inner.style != style
            || inner.color != color
            || inner.modelName != modelName
            || inner.modeName != modeName
            || inner.stopHotkeyHint != stopHotkeyHint
            || inner.isCancelling != isCancelling {
            hosting.rootView = LocalizedRoot(model: model) {
                RecordingIndicatorView(
                    phase: phase,
                    style: style,
                    color: color,
                    modelName: modelName,
                    modeName: modeName,
                    stopHotkeyHint: stopHotkeyHint,
                    onStop: onStop,
                    isCancelling: isCancelling,
                    feed: self.recordingLevelFeed
                )
            }
        }
    }

    private func makeRecordingIndicatorWindow(phase: IndicatorPhase, style: WaveformStyle, color: WaveformColor, modelName: String, modeName: String?, stopHotkeyHint: String, isCancelling: Bool, onStop: @escaping () -> Void) -> NSWindow {
        let size = NSSize(width: 260, height: 98)
        let panel = NSPanel(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isFloatingPanel = true
        panel.becomesKeyOnlyIfNeeded = true
        panel.level = .floating
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .fullScreenAuxiliary]
        panel.isReleasedWhenClosed = false

        let hosting = NSHostingController(
            rootView: LocalizedRoot(model: model) {
                RecordingIndicatorView(
                    phase: phase,
                    style: style,
                    color: color,
                    modelName: modelName,
                    modeName: modeName,
                    stopHotkeyHint: stopHotkeyHint,
                    onStop: onStop,
                    isCancelling: isCancelling,
                    feed: self.recordingLevelFeed
                )
            }
        )
        hosting.view.frame = NSRect(origin: .zero, size: size)
        panel.contentViewController = hosting
        return panel
    }

    private func positionRecordingIndicatorWindow(_ window: NSWindow) {
        guard let screenFrame = NSScreen.main?.visibleFrame else { return }
        let margin: CGFloat = 16
        let size = window.frame.size
        let origin = NSPoint(
            x: screenFrame.midX - size.width / 2,
            y: screenFrame.maxY - size.height - margin
        )
        window.setFrameOrigin(origin)
    }

    private func rebuildModeMenu() {
        modeMenu.removeAllItems()

        let postProcessingEnabled = model.persistedPostProcessingEnabled

        let offItem = NSMenuItem(
            title: L("Off", locale: currentLocale),
            action: #selector(disablePostProcessing(_:)),
            keyEquivalent: ""
        )
        offItem.target = self
        offItem.state = postProcessingEnabled ? .off : .on
        modeMenu.addItem(offItem)

        modeMenu.addItem(.separator())

        for mode in model.persistedModes {
            let item = NSMenuItem(
                title: mode.name,
                action: #selector(selectMode(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.representedObject = mode.id
            item.state = (postProcessingEnabled && model.persistedActiveModeID == mode.id) ? .on : .off
            modeMenu.addItem(item)
        }
    }

    private func rebuildModelMenu() {
        modelMenu.removeAllItems()
        let activePreset = model.settings.localModel
        for preset in model.availableModelPresets {
            let item = NSMenuItem(
                title: preset.displayName,
                action: #selector(selectWhisperPreset(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.representedObject = preset.rawValue
            item.state = (preset == activePreset) ? .on : .off
            modelMenu.addItem(item)
        }
    }

    private func rebuildMicMenu() {
        micMenu.removeAllItems()
        let activeName = model.runtime.activeInputDeviceName.isEmpty
            ? model.settings.inputDeviceName
            : model.runtime.activeInputDeviceName
        let locale = currentLocale

        var names = model.devices.map(\.name)
        if names.isEmpty {
            names = [model.settings.inputDeviceName]
        }
        if !activeName.isEmpty && !names.contains(activeName) {
            names.insert(activeName, at: 0)
        }

        for name in names {
            let label = name == "System Default"
                ? L("System default", locale: locale)
                : name
            let item = NSMenuItem(
                title: label,
                action: #selector(selectInputDevice(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.representedObject = name
            item.state = (name == activeName) ? .on : .off
            micMenu.addItem(item)
        }
    }

    private static let recentHistoryTrayPreviewLimit = 40
    private static let recentHistoryTrayCount = 5

    private func rebuildRecentHistoryMenu() {
        recentHistoryMenu.removeAllItems()
        let locale = currentLocale

        let entries = Array(model.history.prefix(Self.recentHistoryTrayCount))
        if entries.isEmpty {
            let item = NSMenuItem(
                title: L("No entries", locale: locale),
                action: nil,
                keyEquivalent: ""
            )
            item.isEnabled = false
            recentHistoryMenu.addItem(item)
            return
        }

        for entry in entries {
            let preview = trayPreview(for: entry.text)
            let title = entry.wasCancelled
                ? "⚠︎ \(preview)"
                : preview
            let item = NSMenuItem(
                title: title,
                action: #selector(copyHistoryEntryFromTray(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.representedObject = entry.id
            recentHistoryMenu.addItem(item)
        }
    }

    private func trayPreview(for text: String) -> String {
        let collapsed = text
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespaces)
        if collapsed.count <= Self.recentHistoryTrayPreviewLimit {
            return collapsed
        }
        let cutoff = collapsed.index(
            collapsed.startIndex,
            offsetBy: Self.recentHistoryTrayPreviewLimit
        )
        return String(collapsed[..<cutoff]) + "…"
    }

    @objc private func copyHistoryEntryFromTray(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String else { return }
        model.copyHistoryEntry(id: id)
    }

    private func show(_ window: NSWindow) {
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    private func makeWindow<Content: View>(title: String, size: NSSize, rootView: Content) -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = title
        window.center()
        window.isReleasedWhenClosed = false
        window.contentViewController = NSHostingController(
            rootView: LocalizedRoot(model: model) { rootView }
        )
        return window
    }

    private func statusImage(recording: Bool) -> NSImage? {
        let symbolName = recording ? "megaphone.fill" : "megaphone"
        let image = NSImage(systemSymbolName: symbolName, accessibilityDescription: "Open Whisper")
        image?.isTemplate = true
        return image
    }

    private func installEscapeMonitor() {
        installEscapeCarbonHotKey()

        if escapeLocalMonitor == nil {
            escapeLocalMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard event.keyCode == AppDelegate.escapeKeyCode else { return event }
                self?.model.cancelDictation()
                return nil
            }
        }
    }

    private func removeEscapeMonitor() {
        removeEscapeCarbonHotKey()

        if let monitor = escapeLocalMonitor {
            NSEvent.removeMonitor(monitor)
            escapeLocalMonitor = nil
        }
    }

    private func installEscapeCarbonHotKey() {
        guard escapeHotKeyRef == nil else { return }

        if escapeEventHandler == nil {
            var spec = EventTypeSpec(
                eventClass: OSType(kEventClassKeyboard),
                eventKind: UInt32(kEventHotKeyPressed)
            )
            let selfPtr = Unmanaged.passUnretained(self).toOpaque()
            let installStatus = InstallEventHandler(
                GetApplicationEventTarget(),
                AppDelegate.escapeHotKeyHandler,
                1,
                &spec,
                selfPtr,
                &escapeEventHandler
            )
            if installStatus != noErr {
                NSLog("Open Whisper: failed to install Escape event handler (OSStatus %d)", Int(installStatus))
                escapeEventHandler = nil
                return
            }
        }

        let hotKeyID = EventHotKeyID(
            signature: AppDelegate.escapeHotKeySignature,
            id: AppDelegate.escapeHotKeyID
        )
        let registerStatus = RegisterEventHotKey(
            UInt32(AppDelegate.escapeKeyCode),
            0,
            hotKeyID,
            GetApplicationEventTarget(),
            0,
            &escapeHotKeyRef
        )

        if registerStatus == noErr { return }

        escapeHotKeyRef = nil
        if registerStatus == OSStatus(eventHotKeyExistsErr) { return }
        NSLog("Open Whisper: failed to register Escape hotkey (OSStatus %d)", Int(registerStatus))
    }

    private func removeEscapeCarbonHotKey() {
        if let ref = escapeHotKeyRef {
            UnregisterEventHotKey(ref)
            escapeHotKeyRef = nil
        }
        if let handler = escapeEventHandler {
            RemoveEventHandler(handler)
            escapeEventHandler = nil
        }
    }

    private static let escapeHotKeyHandler: EventHandlerUPP = { _, event, userData in
        guard let event, let userData else { return OSStatus(eventNotHandledErr) }

        var hotKeyID = EventHotKeyID()
        let status = GetEventParameter(
            event,
            EventParamName(kEventParamDirectObject),
            EventParamType(typeEventHotKeyID),
            nil,
            MemoryLayout<EventHotKeyID>.size,
            nil,
            &hotKeyID
        )

        guard status == noErr,
              hotKeyID.signature == AppDelegate.escapeHotKeySignature,
              hotKeyID.id == AppDelegate.escapeHotKeyID
        else {
            return OSStatus(eventNotHandledErr)
        }

        let delegate = Unmanaged<AppDelegate>.fromOpaque(userData).takeUnretainedValue()
        DispatchQueue.main.async {
            delegate.model.cancelDictation()
        }
        return noErr
    }
}
