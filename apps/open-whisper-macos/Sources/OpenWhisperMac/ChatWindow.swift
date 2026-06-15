import SwiftUI

/// Drives the chat window: polls the bridge chat state, manages the model
/// selection + conversation sessions, drives the recording waveform, and speaks
/// new assistant answers through the configured TTS backend.
@MainActor
final class ChatViewModel: ObservableObject {
    @Published var state: ChatStateDTO = .empty
    @Published var registry: [LlmRegistryEntryDTO] = []
    @Published var selectedModelStableId: String?

    /// Voice selection surfaced in the header (#28 AP5): the local Piper voices —
    /// the same curated set the settings expose — and the current pick. Changing
    /// it persists to `AppSettings.chat.tts` and reconfigures the live player.
    @Published var voiceOptions: [String] = []
    @Published var selectedVoice: String = ""
    /// True while a freshly picked voice's model is downloading (~110 MB on first
    /// use) — surfaced as a spinner so the header doesn't go silently dead.
    @Published var voiceDownloading = false

    /// Live mic levels for the in-window waveform (reuses the dictation feed).
    let levelFeed = RecordingLevelFeed()

    private let bridge = BridgeClient()
    private let tts = ChatTtsPlayer()
    private let thinking = ChatThinkingSound()
    private var timer: Timer?
    /// Streaming TTS bookkeeping: index of the assistant message currently being
    /// spoken, and how many characters of it have already been dispatched.
    private var ttsMsgIndex = -1
    private var spokenOffset = 0
    /// Set when the user stops speech (Escape) — suppresses further synthesis for
    /// the current answer even as it keeps streaming. Reset on the next answer.
    private var speechSuppressed = false
    private var loadedOnce = false
    private var lastActiveSessionId = ""

    /// Models offered for chat: exactly the app-wide enabled set the user curated
    /// in Settings → language models (an empty set means "show all", so a fresh
    /// install isn't empty). This mirrors the central curation 1:1 — no extra
    /// availability filtering, which previously dropped enabled-but-not-ready
    /// models and made the list diverge from what the user configured.
    var selectableModels: [LlmRegistryEntryDTO] {
        let enabled = registry.filter { $0.enabled }
        var pool = enabled.isEmpty ? registry : enabled
        // Always keep the model the picker currently points at (the active or
        // persisted-default model) present — otherwise a model running in the
        // background that isn't in the enabled set leaves the picker blank.
        if let selected = selectedModelStableId,
           !pool.contains(where: { $0.stableId == selected }),
           let active = registry.first(where: { $0.stableId == selected }) {
            pool.append(active)
        }
        return pool
    }

    /// Selectable plain language models (everything that is not a Hermes agent),
    /// for the first group of the header picker.
    var languageModelOptions: [LlmRegistryEntryDTO] {
        selectableModels.filter { $0.backendKind != .hermes }
    }

    /// Selectable Hermes agents, for the second group of the header picker.
    var agentOptions: [LlmRegistryEntryDTO] {
        selectableModels.filter { $0.backendKind == .hermes }
    }

    /// Persisted default model, used as the picker fallback for conversations
    /// that have not pinned their own model yet.
    private var defaultModelRef: LlmModelRefDTO?

    func start() {
        registry = (try? bridge.getLlmRegistry()) ?? []
        // Pull the persisted chat config (default model + TTS) fresh each time
        // the window opens, so edits in Settings → Plugins → Chat take effect.
        let settings = try? bridge.loadSettings()
        let chat = settings?.chat ?? .default
        // Speech output (TTS) now lives top-level in AppSettings (#28 AP1), no
        // longer inside the chat plugin config.
        let speech = settings?.speechOutput ?? ChatSettingsDTO.default.tts
        tts.configure(speech)
        defaultModelRef = chat.defaultModelRef
        selectedVoice = speech.piperVoice.isEmpty ? "de_DE-thorsten-high" : speech.piperVoice
        reloadVoiceOptions()
        let poll = Timer(timeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.tick() }
        }
        RunLoop.main.add(poll, forMode: .common)
        timer = poll
        tick()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        tts.stop()
        thinking.stop()
        levelFeed.stop()
    }

    /// Stops speech output now (Escape) and silences the rest of the current
    /// answer even while it keeps streaming. The next answer speaks normally.
    func cancelSpeech() {
        tts.stop()
        thinking.stop()
        speechSuppressed = true
    }

    func toggleListening() {
        switch state.phase {
        case .listening:
            _ = try? bridge.chatStopListening()
        case .idle:
            tts.stop()
            do {
                _ = try bridge.chatStartListening()
            } catch {
                // e.g. the chat plugin was disabled while the window stayed open.
                state.error = error.localizedDescription
            }
        case .transcribing, .generating:
            break
        }
    }

    /// Sends a typed message as a chat turn (same flow as a voice transcript).
    func sendText(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        tts.stop()
        bridge.chatSendText(trimmed)
        tick()
    }

    // MARK: Sessions

    func newSession() {
        tts.stop()
        bridge.chatNewSession()
        tick()
    }

    func switchSession(_ id: String) {
        guard id != state.activeSessionId else { return }
        tts.stop()
        bridge.chatSwitchSession(id: id)
        tick()
    }

    func deleteSession(_ id: String) {
        tts.stop()
        bridge.chatDeleteSession(id: id)
        tick()
    }

    func selectModel(_ stableId: String?) {
        selectedModelStableId = stableId
        let ref = registry.first(where: { $0.stableId == stableId })?.modelRef
        bridge.chatSetModel(ref)
    }

    // MARK: Voice (#28 AP5)

    /// Persists the picked local Piper voice into `AppSettings.chat.tts` and
    /// reconfigures the live player so the next spoken answer uses it. Also pins
    /// the provider to Piper: the chat then speaks with the same fast neural voice
    /// the settings configure (System is only an automatic fallback, and OpenAI
    /// TTS was removed), so picking a voice here also normalizes a stale provider.
    func selectVoice(_ id: String) {
        selectedVoice = id
        guard var settings = try? bridge.loadSettings() else { return }
        settings.speechOutput.piperVoice = id
        settings.speechOutput.provider = .piper
        _ = try? bridge.saveSettings(settings)
        tts.configure(settings.speechOutput)
        prepareVoiceIfNeeded(id)
    }

    /// Downloads the picked voice's model in the background when it isn't on disk
    /// yet, so the first spoken answer doesn't block silently on a ~110 MB fetch
    /// inside `piper_speech`. The spinner clears only if this voice is still the
    /// selection, so switching again mid-download doesn't hide the new download.
    private func prepareVoiceIfNeeded(_ id: String) {
        if (try? bridge.ttsLocalReady(voice: id)) == true {
            voiceDownloading = false
            return
        }
        voiceDownloading = true
        Task { [weak self] in
            _ = await Task.detached { try? BridgeClient().ttsLocalPrepare(voice: id) }.value
            guard let self else { return }
            if self.selectedVoice == id { self.voiceDownloading = false }
        }
    }

    /// Human-readable label for a Piper voice id within its language section:
    /// `de_DE-thorsten-high` → `Thorsten — high`.
    func voiceLabel(_ id: String) -> String {
        let parts = id.split(separator: "-")
        guard parts.count >= 2 else { return id }
        let name = parts[1].replacingOccurrences(of: "_", with: " ").capitalized
        let quality = parts.count > 2 ? String(parts[2]) : ""
        return quality.isEmpty ? name : "\(name) — \(quality)"
    }

    /// Voices grouped by language (`de_DE`, `en_US`, …) in first-seen order, for
    /// the header picker's sections.
    var voiceGroups: [VoiceGroup] {
        var order: [String] = []
        var byLang: [String: [String]] = [:]
        for id in voiceOptions {
            let lang = String(id.split(separator: "-").first ?? Substring(id))
            if byLang[lang] == nil { order.append(lang) }
            byLang[lang, default: []].append(id)
        }
        return order.map { VoiceGroup(id: $0, label: Self.languageLabel($0), ids: byLang[$0] ?? []) }
    }

    /// Friendly name for a Piper language code (`de_DE` → "German (Germany)" in
    /// the current locale), falling back to the raw code.
    private static func languageLabel(_ code: String) -> String {
        Locale.current.localizedString(forIdentifier: code) ?? code
    }

    /// Loads the local Piper voices to offer — the same curated set the settings
    /// expose. The current pick is always kept present even if the list can't be
    /// fetched.
    private func reloadVoiceOptions() {
        voiceOptions = (try? bridge.ttsPiperVoices()) ?? []
        if !selectedVoice.isEmpty, !voiceOptions.contains(selectedVoice) {
            voiceOptions.insert(selectedVoice, at: 0)
        }
    }

    /// Re-syncs the header picker to the active conversation's model when the
    /// user switches conversations. Each conversation remembers its own pick
    /// (#agent); falls back to the persisted default, then the first ready
    /// model. Only updates local UI state — it must not re-persist (that would
    /// overwrite the conversation's stored pick with the fallback).
    private func syncSelection(to activeRef: LlmModelRefDTO?) {
        let resolved = activeRef ?? defaultModelRef
        if let resolved,
           let match = registry.first(where: { $0.modelRef == resolved }) {
            selectedModelStableId = match.stableId
        } else {
            selectedModelStableId = selectableModels.first?.stableId
        }
    }

    private func tick() {
        guard let fresh = try? bridge.chatGetState() else { return }
        // Phase changes without a revision bump (listening → transcribing →
        // generating → idle), so watch both. Avoids re-rendering 10×/sec when
        // nothing changed.
        let phaseChanged = !loadedOnce || fresh.phase != state.phase
        let changed = phaseChanged || fresh.revision != state.revision
        guard changed else { return }

        // Drive the waveform only on the listening edge — calling start() every
        // poll would tear down its 30 Hz timer and flat-line the bars. The
        // "thinking" cue runs for the whole generating phase to mask the wait.
        if phaseChanged {
            switch fresh.phase {
            case .listening:
                levelFeed.start()
                thinking.stop()
            case .generating:
                levelFeed.stop()
                thinking.start()
            case .transcribing, .idle:
                levelFeed.stop()
                thinking.stop()
            }
        }
        loadedOnce = true

        let sessionChanged = fresh.activeSessionId != lastActiveSessionId
        lastActiveSessionId = fresh.activeSessionId
        state = fresh
        if sessionChanged {
            // Switched to / loaded a different conversation — reflect its pinned
            // model/agent and don't re-speak its existing answers.
            syncSelection(to: fresh.activeModelRef)
            markAllSpoken()
            tts.stop()
        } else {
            pumpStreamingTts()
        }
    }

    /// Speaks the active conversation's latest assistant answer as it streams in:
    /// complete sentences are spoken while generation continues; the remainder is
    /// flushed once it finishes. Successive answers and successive sentences are
    /// queued in order by the player.
    private func pumpStreamingTts() {
        guard let lastIndex = state.messages.indices.last,
              state.messages[lastIndex].role == .assistant else {
            return
        }
        // A new assistant message → start fresh + re-read TTS config so a
        // voice/speed change in Settings takes effect without reopening.
        if lastIndex != ttsMsgIndex {
            ttsMsgIndex = lastIndex
            spokenOffset = 0
            speechSuppressed = false
            if let freshTts = (try? bridge.loadSettings())?.speechOutput {
                tts.configure(freshTts)
            }
        }

        let chars = Array(state.messages[lastIndex].content)
        // Text started arriving → drop the "thinking" cue.
        if !chars.isEmpty { thinking.stop() }
        // User stopped this answer's speech — stay silent until the next one.
        if speechSuppressed { return }
        if spokenOffset > chars.count { spokenOffset = chars.count }
        let pending = String(chars[spokenOffset...])

        if state.phase == .generating {
            // Speak only up to the last completed sentence; keep the tail buffered.
            guard let upto = Self.lastSentenceBoundary(in: pending) else { return }
            let ready = String(pending.prefix(upto))
            let chunk = ready.trimmingCharacters(in: .whitespacesAndNewlines)
            if !chunk.isEmpty { tts.speak(chunk) }
            spokenOffset += ready.count
        } else {
            // Generation finished — flush whatever is left.
            let chunk = pending.trimmingCharacters(in: .whitespacesAndNewlines)
            if !chunk.isEmpty { tts.speak(chunk) }
            spokenOffset = chars.count
        }
    }

    /// Marks the active conversation's current text as already spoken (used on
    /// session switch so loading a conversation doesn't re-read its answers).
    private func markAllSpoken() {
        if let lastIndex = state.messages.indices.last,
           state.messages[lastIndex].role == .assistant {
            ttsMsgIndex = lastIndex
            spokenOffset = state.messages[lastIndex].content.count
        } else {
            ttsMsgIndex = -1
            spokenOffset = 0
        }
    }

    /// Character offset just past the last sentence-ending punctuation, or `nil`
    /// if none yet — so only complete sentences are spoken during streaming.
    private static func lastSentenceBoundary(in text: String) -> Int? {
        let terminators: Set<Character> = [".", "!", "?", ":", ";", "…", "\n"]
        var boundary: Int? = nil
        for (i, ch) in text.enumerated() where terminators.contains(ch) {
            boundary = i + 1
        }
        return boundary
    }
}

/// A language group of Piper voices for the header picker's sections.
struct VoiceGroup: Identifiable {
    let id: String
    let label: String
    let ids: [String]
}

struct ChatWindowView: View {
    @ObservedObject var chat: ChatViewModel
    @Environment(\.locale) private var locale
    @State private var draft = ""

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            VStack(spacing: 0) {
                header
                Divider()
                transcript
                Divider()
                inputBar
            }
        }
        .navigationSplitViewStyle(.balanced)
        .frame(minWidth: 680, idealWidth: 760, minHeight: 480, idealHeight: 580)
        .onAppear { chat.start() }
        .onDisappear { chat.stop() }
        // Escape stops the spoken answer.
        .onExitCommand { chat.cancelSpeech() }
    }

    // MARK: Sidebar (conversation history)

    private var sidebar: some View {
        List(selection: Binding(
            get: { chat.state.activeSessionId },
            set: { if let id = $0 { chat.switchSession(id) } }
        )) {
            ForEach(chat.state.sessions) { session in
                VStack(alignment: .leading, spacing: 2) {
                    Text(sessionTitle(session))
                        .lineLimit(1)
                    Text(sessionSubtitle(session))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .tag(session.id)
                .contextMenu {
                    Button(role: .destructive) {
                        chat.deleteSession(session.id)
                    } label: {
                        Text("Delete", bundle: .module)
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .frame(minWidth: 200, idealWidth: 220)
        .navigationSplitViewColumnWidth(220)
        .safeAreaInset(edge: .top) {
            Button {
                chat.newSession()
            } label: {
                Label {
                    Text("New chat", bundle: .module)
                } icon: {
                    Image(systemName: "square.and.pencil")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
            .padding(8)
        }
    }

    private func sessionTitle(_ session: ChatSessionDTO) -> String {
        session.title.isEmpty ? L("New chat", locale: locale) : session.title
    }

    private func sessionSubtitle(_ session: ChatSessionDTO) -> String {
        session.messageCount == 0
            ? L("Empty", locale: locale)
            : "\(session.messageCount) " + L("messages", locale: locale)
    }

    // MARK: Detail

    private var header: some View {
        HStack(spacing: 12) {
            Picker(selection: Binding(
                get: { chat.selectedModelStableId },
                set: { chat.selectModel($0) }
            )) {
                Section {
                    ForEach(chat.languageModelOptions) { entry in
                        Text(entry.displayName).tag(String?.some(entry.stableId))
                    }
                } header: {
                    Text("Language models", bundle: .module)
                }
                if !chat.agentOptions.isEmpty {
                    Section {
                        ForEach(chat.agentOptions) { entry in
                            Text(entry.displayName).tag(String?.some(entry.stableId))
                        }
                    } header: {
                        Text("Hermes agents", bundle: .module)
                    }
                }
            } label: {
                Text("Model", bundle: .module)
            }
            .frame(maxWidth: 300)

            Picker(selection: Binding(
                get: { chat.selectedVoice },
                set: { chat.selectVoice($0) }
            )) {
                ForEach(chat.voiceGroups) { group in
                    Section(group.label) {
                        ForEach(group.ids, id: \.self) { id in
                            Text(chat.voiceLabel(id)).tag(id)
                        }
                    }
                }
            } label: {
                Text("Voice", bundle: .module)
            }
            .frame(maxWidth: 240)

            if chat.voiceDownloading {
                ProgressView()
                    .controlSize(.small)
                Text("Downloading voice…", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize()
            }

            Spacer()
        }
        .padding(12)
    }

    private var transcript: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10) {
                    ForEach(Array(chat.state.messages.enumerated()), id: \.offset) { _, message in
                        ChatBubble(message: message)
                    }
                    // Recording shows as a waveform bubble on the user's (right)
                    // side; the assistant "typing" appears on the left until its
                    // streamed text starts.
                    if chat.state.phase == .listening {
                        RecordingBubble(feed: chat.levelFeed)
                    }
                    if showsTypingBubble {
                        TypingBubble()
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(12)
            }
            .onChange(of: chat.state.revision) { _, _ in
                withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
            }
            .onChange(of: chat.state.phase) { _, _ in
                withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
            }
        }
    }

    /// Show the "typing" indicator while generating, until the assistant's
    /// streamed text starts (after which its growing bubble stands in for it).
    private var showsTypingBubble: Bool {
        chat.state.phase == .generating && chat.state.messages.last?.role != .assistant
    }

    private var inputBar: some View {
        VStack(spacing: 6) {
            if let error = chat.state.error, !error.isEmpty {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack(spacing: 10) {
                TextField(L("Message…", locale: locale), text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { sendDraft() }
                    .disabled(inputBusy)
                if !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    Button {
                        sendDraft()
                    } label: {
                        Image(systemName: "arrow.up.circle.fill").font(.title2)
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.tint)
                    .disabled(inputBusy)
                    .help(L("Send", locale: locale))
                }
                Button {
                    chat.toggleListening()
                } label: {
                    Image(systemName: buttonIcon).font(.title2)
                }
                .buttonStyle(.borderedProminent)
                .tint(buttonTint)
                .help(buttonLabel)
            }
        }
        .padding(12)
    }

    /// Typed input is only accepted when not mid-transcription/generation.
    private var inputBusy: Bool {
        chat.state.phase == .transcribing || chat.state.phase == .generating
    }

    private func sendDraft() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !inputBusy else { return }
        chat.sendText(text)
        draft = ""
    }

    private var buttonLabel: String {
        switch chat.state.phase {
        case .listening: return L("Stop", locale: locale)
        case .transcribing: return L("Transcribing…", locale: locale)
        case .generating: return L("Thinking…", locale: locale)
        case .idle: return L("Speak", locale: locale)
        }
    }

    private var buttonIcon: String {
        switch chat.state.phase {
        case .listening: return "stop.circle.fill"
        case .transcribing, .generating: return "ellipsis.circle.fill"
        case .idle: return "mic.circle.fill"
        }
    }

    /// Matches the floating dictation bubble: red while recording, yellow while
    /// transcribing/answering, accent when idle.
    private var buttonTint: Color {
        switch chat.state.phase {
        case .listening: return .red
        case .transcribing, .generating: return .yellow
        case .idle: return .accentColor
        }
    }

}

/// Recording indicator shown as a bubble on the user's (right) side of the
/// transcript while listening — like a chat client's "typing" indicator.
private struct RecordingBubble: View {
    @ObservedObject var feed: RecordingLevelFeed

    var body: some View {
        HStack {
            Spacer(minLength: 40)
            ChatWaveformView(feed: feed)
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(
                    Color.accentColor.opacity(0.18),
                    in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                )
        }
    }
}

/// Assistant "typing" indicator: three softly pulsing dots on the left, shown
/// while the answer is being generated but hasn't started streaming text yet.
private struct TypingBubble: View {
    var body: some View {
        HStack {
            TimelineView(.animation) { context in
                let t = context.date.timeIntervalSinceReferenceDate
                HStack(spacing: 5) {
                    ForEach(0..<3, id: \.self) { i in
                        Circle()
                            .fill(.secondary)
                            .frame(width: 7, height: 7)
                            .opacity(0.3 + 0.7 * Self.pulse(t, i))
                    }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 12)
                .background(.quaternary, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            Spacer(minLength: 40)
        }
    }

    private static func pulse(_ t: TimeInterval, _ i: Int) -> Double {
        (sin(t * 3.0 - Double(i) * 0.6) + 1) / 2
    }
}

/// Compact centered-bars waveform fed by the shared recording-level feed.
/// Mirrors the floating bubble's normalization.
private struct ChatWaveformView: View {
    @ObservedObject var feed: RecordingLevelFeed

    var body: some View {
        HStack(alignment: .center, spacing: 3) {
            ForEach(Array(feed.bars.enumerated()), id: \.offset) { _, level in
                Capsule()
                    .fill(Color.accentColor)
                    .frame(width: 3, height: max(2, CGFloat(Self.normalized(level)) * 22))
            }
        }
        .frame(height: 22)
        .animation(.linear(duration: RecordingLevelFeed.pollingInterval), value: feed.bars)
    }

    private static func normalized(_ level: Float) -> Float {
        let cleaned = max(0, level - RecordingLevelFeed.noiseFloor)
        let curved = cleaned.squareRoot() * RecordingLevelFeed.levelGain
        return min(1, max(0, curved))
    }
}

private struct ChatBubble: View {
    let message: ChatMessageDTO

    private var isUser: Bool { message.role == .user }

    var body: some View {
        HStack {
            if isUser { Spacer(minLength: 40) }
            Text(message.content)
                .textSelection(.enabled)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(
                    isUser ? AnyShapeStyle(Color.accentColor.opacity(0.18))
                        : AnyShapeStyle(.quaternary),
                    in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                )
                .frame(maxWidth: .infinity, alignment: isUser ? .trailing : .leading)
            if !isUser { Spacer(minLength: 40) }
        }
    }
}
