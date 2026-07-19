import SwiftUI

enum IndicatorPhase: Equatable {
    case recording
    case transcribing
    case postProcessing
    case modelNotReady(label: String, progress: Double?, isDownloading: Bool)
    /// Shown for a few seconds after a dictation failure (transcription
    /// error, worker crash, insertion failure) so the bubble doesn't just
    /// vanish without explanation.
    case error(message: String)
    /// Shown briefly (~1s) after a successful dictation so a fast completion
    /// reads as "finished" instead of the bubble silently disappearing.
    case done
}

@MainActor
final class RecordingLevelFeed: ObservableObject {
    static let barCount = 28
    static let pollingInterval: TimeInterval = 1.0 / 30.0
    static let levelGain: Float = 2.8
    static let noiseFloor: Float = 0.002

    @Published private(set) var bars: [Float] = Array(repeating: 0, count: RecordingLevelFeed.barCount)

    private let bridge = BridgeClient()
    private var timer: Timer?

    func start() {
        stop()
        let newTimer = Timer(timeInterval: Self.pollingInterval, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.tick()
            }
        }
        RunLoop.main.add(newTimer, forMode: .common)
        timer = newTimer
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        bars = Array(repeating: 0, count: Self.barCount)
    }

    private func tick() {
        guard let levels = try? bridge.getRecordingLevels().levels else {
            return
        }

        let slice = Array(levels.suffix(Self.barCount))
        if slice.count == Self.barCount {
            bars = slice
        } else {
            var padded = Array(repeating: Float(0), count: Self.barCount - slice.count)
            padded.append(contentsOf: slice)
            bars = padded
        }
    }
}

/// Polls the live transcript (#41) on its own cadence, decoupled from the
/// 0.35 s status poll (which must stay change-detected and idle-cheap) and
/// from the 30 Hz waveform feed. Publishes only when the Rust-side revision
/// advances, so SwiftUI re-renders track actual new text (~1.3×/s), not
/// poll ticks.
@MainActor
final class StreamingTranscriptFeed: ObservableObject {
    static let pollingInterval: TimeInterval = 0.2

    struct Snapshot: Equatable {
        var revision: UInt64 = 0
        var committed: String = ""
        var pending: String = ""
        var isFinal: Bool = false

        var isEmpty: Bool { committed.isEmpty && pending.isEmpty }
    }

    @Published private(set) var snapshot = Snapshot()

    private let bridge = BridgeClient()
    private var timer: Timer?

    /// Idempotent — runs on every state-change pass while the bubble shows an
    /// active phase. Unlike `RecordingLevelFeed.start()` it must not restart
    /// the timer or touch the snapshot: blanking visible text mid-recording
    /// would be a visible glitch (bars just refill on the next 33 ms tick,
    /// text would not).
    func start() {
        guard timer == nil else { return }
        let newTimer = Timer(timeInterval: Self.pollingInterval, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.tick()
            }
        }
        RunLoop.main.add(newTimer, forMode: .common)
        timer = newTimer
    }

    /// Stops polling but keeps the snapshot, so the done/error phases retain
    /// the last text.
    func stop() {
        timer?.invalidate()
        timer = nil
    }

    /// Session boundary: stop polling and clear the text.
    func reset() {
        stop()
        snapshot = Snapshot()
    }

    private func tick() {
        guard let dto = try? bridge.getStreamingTranscript() else { return }
        // Revision guard: ignore stale or out-of-order reads. The Rust side
        // never resets the counter, so this also holds across sessions.
        guard dto.revision > snapshot.revision else { return }
        snapshot = Snapshot(
            revision: dto.revision,
            committed: dto.committed,
            pending: dto.pending,
            isFinal: dto.isFinal
        )
    }
}

struct RecordingIndicatorView: View {
    let phase: IndicatorPhase
    var style: WaveformStyle = .centeredBars
    var color: WaveformColor = .accent
    var modelName: String = ""
    var modeName: String? = nil
    /// Readable shortcut hint shown under the model label while recording, e.g.
    /// "Stop: ⌃⇧Space", so the user knows how to end the dictation without
    /// guessing (and without hitting Escape, which discards it).
    var stopHotkeyHint: String = ""
    /// Invoked by the small stop button in the bubble. Ends the dictation
    /// cleanly (keeps the transcript) — not a cancel.
    var onStop: (() -> Void)? = nil
    /// True while a cancelled dictation is still finishing transcription. Shows
    /// a "being cancelled" hint so the user knows work is still happening (and
    /// will be archived, not inserted).
    var isCancelling: Bool = false
    /// Larger bubble for low-vision users (accessibility). Scales every
    /// dimension and font; the window frame in AppDelegate scales to match.
    var isLarge: Bool = false
    /// Higher-contrast styling: bolder text, stronger text/background colors and
    /// a heavier border. Independent of `isLarge`.
    var highContrast: Bool = false
    /// Live transcription (#41): the dark box hosts a slim waveform strip plus
    /// the committed/pending transcript instead of the full-height waveform.
    /// When false the bubble renders exactly the pre-#41 layout.
    var showsLiveTranscript: Bool = false
    @ObservedObject var feed: RecordingLevelFeed
    @ObservedObject var transcriptFeed: StreamingTranscriptFeed
    @Environment(\.locale) private var locale

    /// Base bubble size at 1x. The window is sized to `baseSize * scale`.
    static let baseSize = CGSize(width: 260, height: 98)
    /// Wider, taller bubble hosting the live transcript (#41) — sized for
    /// comfortable reading (~5 lines at 13 pt).
    static let liveBaseSize = CGSize(width: 420, height: 180)
    /// Scale factor applied when the large (low-vision) view is enabled.
    static let largeScale: CGFloat = 1.7

    /// Single source of truth for the bubble/panel size. AppDelegate derives
    /// the window frame only from this — never from content or phase — so the
    /// panel size changes exclusively when the user flips a setting.
    static func windowSize(isLarge: Bool, live: Bool) -> CGSize {
        let base = live ? liveBaseSize : baseSize
        let scale = isLarge ? largeScale : 1.0
        return CGSize(width: base.width * scale, height: base.height * scale)
    }

    private var scale: CGFloat { isLarge ? Self.largeScale : 1.0 }
    private var leadingControlSize: CGFloat { 24 * scale }

    private func scaledFont(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        .system(size: size * scale, weight: highContrast ? boostedWeight(weight) : weight)
    }

    private func boostedWeight(_ weight: Font.Weight) -> Font.Weight {
        switch weight {
        case .medium: return .semibold
        case .semibold, .bold: return .bold
        default: return .semibold
        }
    }

    /// Model-name style: full primary in high-contrast mode, secondary otherwise.
    private var primaryTextStyle: AnyShapeStyle {
        highContrast ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary)
    }
    /// Mode-name / hint style: one step stronger in high-contrast mode.
    private var subtleTextStyle: AnyShapeStyle {
        highContrast ? AnyShapeStyle(.primary) : AnyShapeStyle(.tertiary)
    }

    var body: some View {
        let size = Self.windowSize(isLarge: isLarge, live: showsLiveTranscript)
        return content
            .padding(10 * scale)
            .frame(width: size.width, height: size.height)
            .background(
                highContrast ? AnyShapeStyle(.ultraThickMaterial) : AnyShapeStyle(.regularMaterial),
                in: RoundedRectangle(cornerRadius: 14 * scale, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 14 * scale, style: .continuous)
                    .strokeBorder(
                        highContrast ? Color.primary.opacity(0.35) : Color.primary.opacity(0.08),
                        lineWidth: (highContrast ? 1.5 : 1) * scale
                    )
            )
    }

    @ViewBuilder
    private var content: some View {
        switch phase {
        case .recording, .transcribing, .postProcessing, .error, .done:
            // Identical layout across every active phase (including the failure
            // and done states) so the box never jumps: the dark waveform area stays up top
            // (flat while not recording) and the status line stays put — only the
            // leading dot color and the title/hint text swap out.
            VStack(spacing: 8 * scale) {
                if showsLiveTranscript {
                    liveTranscriptBox
                } else {
                    waveform
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(
                            Color.black.opacity(highContrast ? 1.0 : 0.85),
                            in: RoundedRectangle(cornerRadius: 8 * scale, style: .continuous)
                        )
                }
                infoRow
            }
        case let .modelNotReady(label, progress, isDownloading):
            modelNotReadyRow(label: label, progress: progress, isDownloading: isDownloading)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    // MARK: Live transcript (#41)

    /// Live layout: the dark box is a pure reading surface — no waveform (the
    /// user chose text over waves; the pulsing red dot in the status row is
    /// the "recording" signal). Hidden from accessibility: the floating panel
    /// is outside the a11y tree anyway, and a text region changing every
    /// ~750 ms must never become a VoiceOver live region — the phase
    /// announcements in AppDelegate stay the only spoken output.
    private var liveTranscriptBox: some View {
        transcriptWindow
            .padding(.horizontal, 10 * scale)
            .padding(.vertical, 8 * scale)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(
                Color.black.opacity(highContrast ? 1.0 : 0.85),
                in: RoundedRectangle(cornerRadius: 8 * scale, style: .continuous)
            )
            .accessibilityHidden(true)
    }

    /// Bottom-anchored clipped window: the transcript takes its full ideal
    /// height and overflows the flexible frame upward, so the newest words
    /// are always visible — no scroll state, no per-update animation (which
    /// also satisfies Reduce Motion). The top fade signals "continues above".
    private var transcriptWindow: some View {
        transcriptContent
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
            .clipped()
            .mask(topFadeMask)
    }

    @ViewBuilder
    private var transcriptContent: some View {
        let snapshot = transcriptFeed.snapshot
        if snapshot.isEmpty {
            Text("Listening…", bundle: .module)
                .font(scaledFont(13).italic())
                .foregroundStyle(Color.white.opacity(0.35))
                // Keep the placeholder's layout identity stable; it simply
                // fades out in the post-stop phases when no text ever arrived.
                .opacity(phase == .recording && !isCancelling ? 1 : 0)
        } else {
            transcriptText(snapshot)
                .font(.system(size: 13 * scale, weight: highContrast ? .medium : .regular))
                .lineSpacing(2.5 * scale)
                // Load-bearing: report the full ideal height so the text
                // overflows the bottom-aligned window at the TOP. Without
                // this, Text truncates at the end and the window would show
                // the oldest words instead of the newest.
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    /// One flowing paragraph: committed bright, pending dimmed. A pending →
    /// committed promotion only changes the color attribute, never glyph
    /// positions — the shared prefix cannot flicker.
    private func transcriptText(_ snapshot: StreamingTranscriptFeed.Snapshot) -> Text {
        let committed = displayCommitted(snapshot.committed)
        let needsJoin = !committed.isEmpty
            && !snapshot.pending.isEmpty
            && !(committed.last?.isWhitespace ?? false)
        let committedColor = Color.white.opacity(highContrast ? 1.0 : 0.92)
        let pendingColor = Color.white.opacity(highContrast ? 0.7 : 0.5)
        return Text(committed + (needsJoin ? " " : ""))
            .foregroundStyle(committedColor)
            + Text(snapshot.pending)
            .foregroundStyle(pendingColor)
    }

    /// Caps the rendered text at a tail well beyond what three lines can show
    /// (~165 visible chars), so layout cost stays O(1) however long the
    /// dictation gets. Walks at most `maxChars` from the end (never the whole
    /// string) and snaps to a word boundary so the faded top line never
    /// starts mid-word.
    private func displayCommitted(_ committed: String) -> String {
        let maxChars = 600
        guard
            let cut = committed.index(
                committed.endIndex, offsetBy: -maxChars, limitedBy: committed.startIndex
            ),
            cut > committed.startIndex
        else {
            return committed
        }
        var tail = committed[cut...]
        if let firstSpace = tail.firstIndex(where: { $0.isWhitespace }) {
            tail = tail[tail.index(after: firstSpace)...]
        }
        return String(tail)
    }

    /// Clear→opaque vertical gradient over the window's top edge, so the
    /// clipped line above reads as intentional continuation instead of a cut.
    private var topFadeMask: some View {
        VStack(spacing: 0) {
            LinearGradient(
                gradient: Gradient(colors: [.clear, .black]),
                startPoint: .top,
                endPoint: .bottom
            )
            .frame(height: 12 * scale)
            Rectangle().fill(Color.black)
        }
    }

    /// The failure message when in the error phase, otherwise nil.
    private var errorMessage: String? {
        if case let .error(message) = phase { return message }
        return nil
    }

    private var isDonePhase: Bool { phase == .done }

    /// Title line in the status row: "Dictation failed" in the error phase,
    /// "Done" in the done phase, otherwise the model name.
    private var primaryLineText: String {
        if errorMessage != nil { return L("Dictation failed", locale: locale) }
        if isDonePhase { return L("Done", locale: locale) }
        return modelName
    }

    /// Detail line: the error message in the error phase, otherwise the phase hint.
    private var secondaryLineText: String {
        errorMessage ?? phaseHint
    }

    /// Centered status line: a leading control — the stop button while
    /// recording (replacing the red dot), otherwise the blinking phase dot —
    /// next to the model name and a small phase hint underneath ("Stop:
    /// ⌃⇧Space" / "Transcription in progress…"), so every phase looks alike.
    private var infoRow: some View {
        HStack(spacing: 9 * scale) {
            leadingControl
            VStack(alignment: .leading, spacing: 1 * scale) {
                if !primaryLineText.isEmpty {
                    Text(primaryLineText)
                        .font(scaledFont(11, weight: .medium))
                        .foregroundStyle(primaryTextStyle)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                if errorMessage == nil, !isDonePhase, let modeName, !modeName.isEmpty {
                    Text(modeName)
                        .font(scaledFont(10))
                        .foregroundStyle(subtleTextStyle)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                if !secondaryLineText.isEmpty {
                    Text(secondaryLineText)
                        .font(scaledFont(9))
                        .foregroundStyle(subtleTextStyle)
                        .lineLimit(2)
                        .truncationMode(.tail)
                }
            }
            Spacer(minLength: 0)
        }
    }

    @ViewBuilder
    private var leadingControl: some View {
        if phase == .recording, let onStop {
            Button(action: onStop) {
                Image(systemName: "stop.fill")
                    .font(.system(size: 11 * scale, weight: .bold))
                    .foregroundStyle(.red)
                    .frame(width: leadingControlSize, height: leadingControlSize)
                    .background(Color.red.opacity(highContrast ? 0.24 : 0.14), in: Circle())
            }
            .buttonStyle(.plain)
            .help(L("Stop dictation", locale: locale))
            .accessibilityLabel(L("Stop dictation", locale: locale))
        } else {
            statusDot
                .frame(width: leadingControlSize, height: leadingControlSize)
                .background(statusDotColor.opacity(highContrast ? 0.25 : 0.15), in: Circle())
        }
    }

    private var phaseHint: String {
        switch phase {
        case .recording:
            return stopHotkeyHint
        case .transcribing:
            return isCancelling
                ? L("Cancelling — saving to history…", locale: locale)
                : L("Transcription in progress", locale: locale)
        case .postProcessing:
            return isCancelling
                ? L("Cancelling — saving to history…", locale: locale)
                : L("Post-processing in progress", locale: locale)
        case .modelNotReady, .error, .done:
            return ""
        }
    }

    private var statusDot: some View {
        TimelineView(.animation(minimumInterval: 0.05, paused: !isBlinkPhase)) { context in
            Circle()
                .fill(statusDotColor)
                .frame(width: TorroMetrics.statusDot * scale, height: TorroMetrics.statusDot * scale)
                .opacity(dotOpacity(at: context.date))
                .shadow(color: phase == .recording ? Color.red.opacity(0.6) : .clear, radius: 3 * scale)
        }
        // A dot never explains itself (design guide §Statuspunkt): it always
        // carries a tooltip and an accessibility label.
        .help(statusDotLabel)
        .accessibilityLabel(Text("Status", bundle: .module))
        .accessibilityValue(statusDotLabel)
    }

    /// What the dot's color means, in words.
    private var statusDotLabel: String {
        if isCancelling {
            return L("Cancelling — saving to history…", locale: locale)
        }
        switch phase {
        case .recording: return L("Recording", locale: locale)
        case .transcribing: return L("Transcribing…", locale: locale)
        case .postProcessing: return L("Post-processing in progress", locale: locale)
        case .modelNotReady: return L("Loading speech model…", locale: locale)
        case .error: return L("Error", locale: locale)
        case .done: return L("Done", locale: locale)
        }
    }

    private func dotOpacity(at date: Date) -> Double {
        guard isBlinkPhase else { return 1.0 }
        let slot = UInt64(date.timeIntervalSince1970 * 16.0)
        var h = slot &* 0x9E3779B97F4A7C15
        h ^= h >> 30
        h &*= 0xBF58476D1CE4E5B9
        h ^= h >> 27
        return (h & 0b11) == 0 ? 0.0 : 1.0
    }

    private var statusDotColor: Color {
        if isCancelling {
            return .orange
        }
        switch phase {
        case .recording: return .red
        case .transcribing: return .yellow
        case .postProcessing: return .yellow
        case .modelNotReady: return .orange
        case .error: return .red
        case .done: return .green
        }
    }

    private var isBlinkPhase: Bool {
        switch phase {
        case .transcribing, .postProcessing: return true
        case .recording, .modelNotReady, .error, .done: return false
        }
    }

    @ViewBuilder
    private func modelNotReadyRow(label: String, progress: Double?, isDownloading: Bool) -> some View {
        HStack(alignment: .top, spacing: 10 * scale) {
            statusDot
                .padding(.top, 4 * scale)
            VStack(alignment: .leading, spacing: 4 * scale) {
                Text("Recording not possible", bundle: .module)
                    .font(scaledFont(13, weight: .medium))
                    .foregroundStyle(.primary)
                if let progress, isDownloading {
                    let percent = Int((progress * 100.0).rounded())
                    Text(String(format: L("Model loading: %@ (%d%%)", locale: locale), label, percent))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    ProgressView(value: progress)
                        .accessibilityLabel(String(format: L("Model loading: %@", locale: locale), label))
                        .accessibilityValue("\(percent)%")
                } else if isDownloading {
                    Text(String(format: L("Model loading: %@", locale: locale), label))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    ProgressView()
                        .progressViewStyle(.linear)
                } else {
                    Text(String(format: L("Model %@ is missing. Please download it in Settings.", locale: locale), label))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private var waveform: some View {
        switch style {
        case .centeredBars:
            centeredBars
        case .line:
            lineWave
        case .envelope:
            envelopeWave
        }
    }

    private var tint: Color { color.swiftUIColor }

    private var centeredBars: some View {
        // Size the bars from the space the layout actually grants (like the
        // line/envelope styles do via GeometryReader). A fixed maximum bar
        // height can exceed the waveform slot — `.frame(maxHeight: .infinity)`
        // never shrinks below its child, so loud bars used to push the dark
        // box (and with it the whole bubble layout) taller in sync with the
        // audio level.
        GeometryReader { geo in
            HStack(spacing: 3 * scale) {
                ForEach(Array(feed.bars.enumerated()), id: \.offset) { _, level in
                    Capsule()
                        .fill(tint)
                        .frame(width: 4 * scale, height: barHeight(for: level, available: geo.size.height))
                        .animation(.linear(duration: RecordingLevelFeed.pollingInterval), value: level)
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
        }
    }

    private var lineWave: some View {
        GeometryReader { geo in
            ZStack(alignment: .center) {
                Rectangle()
                    .fill(tint.opacity(0.18))
                    .frame(height: 1)

                envelopePath(in: geo.size, direction: .up)
                    .stroke(tint,
                            style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
                envelopePath(in: geo.size, direction: .down)
                    .stroke(tint.opacity(0.85),
                            style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .animation(.linear(duration: RecordingLevelFeed.pollingInterval), value: feed.bars)
            .drawingGroup()
        }
    }

    private var envelopeWave: some View {
        GeometryReader { geo in
            filledEnvelopePath(in: geo.size)
                .fill(tint)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .animation(.linear(duration: RecordingLevelFeed.pollingInterval), value: feed.bars)
                .drawingGroup()
        }
    }

    private enum EnvelopeDirection { case up, down }

    private func envelopePath(in size: CGSize, direction: EnvelopeDirection) -> Path {
        let width = size.width
        let height = size.height
        let mid = height / 2
        let bars = feed.bars
        let count = bars.count
        guard count > 0 else { return Path() }
        let step = count > 1 ? width / CGFloat(count - 1) : 0

        return Path { path in
            path.move(to: CGPoint(x: 0, y: mid))
            for (i, level) in bars.enumerated() {
                let amp = normalizedLevel(level) * mid
                let x = CGFloat(i) * step
                let y: CGFloat = direction == .up ? (mid - amp) : (mid + amp)
                path.addLine(to: CGPoint(x: x, y: y))
            }
            path.addLine(to: CGPoint(x: width, y: mid))
        }
    }

    private func filledEnvelopePath(in size: CGSize) -> Path {
        let width = size.width
        let height = size.height
        let mid = height / 2
        let bars = feed.bars
        let count = bars.count
        guard count > 0 else { return Path() }
        let step = count > 1 ? width / CGFloat(count - 1) : 0

        return Path { path in
            path.move(to: CGPoint(x: 0, y: mid))
            for (i, level) in bars.enumerated() {
                let amp = normalizedLevel(level) * mid
                path.addLine(to: CGPoint(x: CGFloat(i) * step, y: mid - amp))
            }
            path.addLine(to: CGPoint(x: width, y: mid))
            for (i, level) in bars.enumerated().reversed() {
                let amp = normalizedLevel(level) * mid
                path.addLine(to: CGPoint(x: CGFloat(i) * step, y: mid + amp))
            }
            path.closeSubpath()
        }
    }

    private func barHeight(for level: Float, available: CGFloat) -> CGFloat {
        max(2.0 * scale, min(normalizedLevel(level) * available, available))
    }

    private func normalizedLevel(_ level: Float) -> CGFloat {
        guard level.isFinite else { return 0 }
        let cleaned = max(0.0, level - RecordingLevelFeed.noiseFloor)
        let curved = sqrt(cleaned) * RecordingLevelFeed.levelGain
        return min(1.0, max(0.0, CGFloat(curved)))
    }
}

extension WaveformColor {
    var swiftUIColor: Color {
        switch self {
        case .accent: return .torroAccent
        case .blue: return .blue
        case .green: return .green
        case .teal: return .teal
        case .orange: return .orange
        case .red: return .red
        case .pink: return .pink
        case .purple: return .purple
        }
    }
}
