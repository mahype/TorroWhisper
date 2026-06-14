import SwiftUI

/// Drives the chat window: polls the bridge chat state, manages the model
/// selection + conversation sessions, drives the recording waveform, and speaks
/// new assistant answers through the configured TTS backend.
@MainActor
final class ChatViewModel: ObservableObject {
    @Published var state: ChatStateDTO = .empty
    @Published var registry: [LlmRegistryEntryDTO] = []
    @Published var selectedModelStableId: String?

    /// Live mic levels for the in-window waveform (reuses the dictation feed).
    let levelFeed = RecordingLevelFeed()

    private let bridge = BridgeClient()
    private let tts = ChatTtsPlayer()
    private var timer: Timer?
    private var spokenAssistantCount = 0
    private var loadedOnce = false
    private var lastActiveSessionId = ""

    /// Models worth offering for chat: anything Ready (local on disk or a cloud
    /// model with a key). Falls back to the full list if nothing is ready.
    var selectableModels: [LlmRegistryEntryDTO] {
        let ready = registry.filter { $0.availability == .ready }
        return ready.isEmpty ? registry : ready
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
        let chat = (try? bridge.loadSettings())?.chat ?? .default
        tts.configure(chat.tts)
        defaultModelRef = chat.defaultModelRef
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
        levelFeed.stop()
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
        // poll would tear down its 30 Hz timer and flat-line the bars.
        if phaseChanged {
            if fresh.phase == .listening {
                levelFeed.start()
            } else {
                levelFeed.stop()
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
            spokenAssistantCount = fresh.messages.filter { $0.role == .assistant }.count
            tts.stop()
        } else {
            speakNewAnswers()
        }
    }

    private func speakNewAnswers() {
        let answers = state.messages.filter { $0.role == .assistant }
        guard answers.count > spokenAssistantCount else { return }
        // Re-read the TTS config so a provider/voice change in Settings takes
        // effect without reopening the chat window.
        if let freshTts = (try? bridge.loadSettings())?.chat.tts {
            tts.configure(freshTts)
        }
        for answer in answers[spokenAssistantCount...] {
            tts.speak(answer.content)
        }
        spokenAssistantCount = answers.count
    }
}

struct ChatWindowView: View {
    @ObservedObject var chat: ChatViewModel
    @Environment(\.locale) private var locale

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
            .frame(maxWidth: 320)
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
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(12)
            }
            .onChange(of: chat.state.revision) { _, _ in
                withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
            }
        }
    }

    private var inputBar: some View {
        VStack(spacing: 6) {
            if let error = chat.state.error, !error.isEmpty {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack(spacing: 12) {
                if chat.state.phase == .listening {
                    ChatWaveformView(feed: chat.levelFeed)
                } else {
                    Text(phaseText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                Button {
                    chat.toggleListening()
                } label: {
                    Label {
                        Text(buttonLabel)
                    } icon: {
                        Image(systemName: buttonIcon)
                    }
                    .font(.title3)
                }
                .buttonStyle(.borderedProminent)
                .tint(buttonTint)
            }
        }
        .padding(12)
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

    private var phaseText: String {
        switch chat.state.phase {
        case .idle: return L("Ready", locale: locale)
        case .listening: return L("Listening…", locale: locale)
        case .transcribing: return L("Transcribing…", locale: locale)
        case .generating: return L("Thinking…", locale: locale)
        }
    }
}

/// Compact centered-bars waveform for the chat input bar, fed by the shared
/// recording-level feed. Mirrors the floating bubble's normalization.
private struct ChatWaveformView: View {
    @ObservedObject var feed: RecordingLevelFeed

    var body: some View {
        HStack(alignment: .center, spacing: 3) {
            ForEach(Array(feed.bars.enumerated()), id: \.offset) { _, level in
                Capsule()
                    .fill(Color.red.opacity(0.85))
                    .frame(width: 3, height: max(2, CGFloat(Self.normalized(level)) * 22))
            }
        }
        .frame(height: 22)
        .frame(maxWidth: .infinity, alignment: .leading)
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
