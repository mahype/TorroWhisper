import SwiftUI

/// Drives the chat window: polls the bridge chat state, manages the model
/// selection, and speaks new assistant answers through the configured TTS
/// backend (system or OpenAI).
@MainActor
final class ChatViewModel: ObservableObject {
    @Published var state: ChatStateDTO = .empty
    @Published var registry: [LlmRegistryEntryDTO] = []
    @Published var selectedModelStableId: String?

    private let bridge = BridgeClient()
    private let tts = ChatTtsPlayer()
    private var timer: Timer?
    private var spokenAssistantCount = 0
    private var loadedOnce = false

    /// Models worth offering for chat: anything Ready (local on disk or a cloud
    /// model with a key). Falls back to the full list if nothing is ready.
    var selectableModels: [LlmRegistryEntryDTO] {
        let ready = registry.filter { $0.availability == .ready }
        return ready.isEmpty ? registry : ready
    }

    func start() {
        registry = (try? bridge.getLlmRegistry()) ?? []
        // Pull the persisted chat config (default model + TTS) fresh each time
        // the window opens, so edits in Settings → Plugins → Chat take effect.
        let chat = (try? bridge.loadSettings())?.chat ?? .default
        tts.configure(chat.tts)
        seedModelSelection(defaultRef: chat.defaultModelRef)
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

    func newConversation() {
        tts.stop()
        bridge.chatReset()
        spokenAssistantCount = 0
        tick()
    }

    func selectModel(_ stableId: String?) {
        selectedModelStableId = stableId
        let ref = registry.first(where: { $0.stableId == stableId })?.modelRef
        bridge.chatSetModel(ref)
    }

    /// Seeds the in-window picker. Prefers the persisted default model; falls
    /// back to the first ready model. The pick is a session override only.
    private func seedModelSelection(defaultRef: LlmModelRefDTO?) {
        guard selectedModelStableId == nil else { return }
        if let defaultRef,
           let match = registry.first(where: { $0.modelRef == defaultRef }) {
            selectModel(match.stableId)
        } else if let first = selectableModels.first {
            selectModel(first.stableId)
        }
    }

    private func tick() {
        guard let fresh = try? bridge.chatGetState() else { return }
        // Phase changes without a revision bump (listening → transcribing →
        // generating → idle), so watch both. Avoids re-rendering 10×/sec when
        // nothing changed.
        let changed = !loadedOnce || fresh.revision != state.revision || fresh.phase != state.phase
        guard changed else { return }
        loadedOnce = true
        state = fresh
        speakNewAnswers()
    }

    private func speakNewAnswers() {
        let answers = state.messages.filter { $0.role == .assistant }
        guard answers.count > spokenAssistantCount else { return }
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
        VStack(spacing: 0) {
            header
            Divider()
            transcript
            Divider()
            inputBar
        }
        .frame(minWidth: 440, idealWidth: 480, minHeight: 480, idealHeight: 560)
        .onAppear { chat.start() }
        .onDisappear { chat.stop() }
    }

    private var header: some View {
        HStack(spacing: 12) {
            Picker(selection: Binding(
                get: { chat.selectedModelStableId },
                set: { chat.selectModel($0) }
            )) {
                ForEach(chat.selectableModels) { entry in
                    Text(entry.displayName).tag(String?.some(entry.stableId))
                }
            } label: {
                Text("Model", bundle: .module)
            }
            .frame(maxWidth: 280)

            Spacer()

            Button {
                chat.newConversation()
            } label: {
                Label {
                    Text("New", bundle: .module)
                } icon: {
                    Image(systemName: "square.and.pencil")
                }
            }
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
                Text(phaseText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button {
                    chat.toggleListening()
                } label: {
                    Label {
                        Text(chat.state.phase == .listening
                            ? L("Stop", locale: locale)
                            : L("Speak", locale: locale))
                    } icon: {
                        Image(systemName: chat.state.phase == .listening
                            ? "stop.circle.fill"
                            : "mic.circle.fill")
                    }
                    .font(.title3)
                }
                .buttonStyle(.borderedProminent)
                .disabled(chat.state.phase == .transcribing || chat.state.phase == .generating)
            }
        }
        .padding(12)
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
