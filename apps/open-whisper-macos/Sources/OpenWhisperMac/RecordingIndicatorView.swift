import SwiftUI

enum IndicatorPhase: Equatable {
    case recording
    case transcribing
    case postProcessing
    case modelNotReady(label: String, progress: Double?, isDownloading: Bool)
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
    @ObservedObject var feed: RecordingLevelFeed
    @Environment(\.locale) private var locale

    var body: some View {
        content
            .padding(10)
            .frame(width: 260, height: 98)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(Color.primary.opacity(0.08), lineWidth: 1)
            )
    }

    @ViewBuilder
    private var content: some View {
        switch phase {
        case .recording, .transcribing, .postProcessing:
            // Identical layout across every active phase so the box never jumps:
            // the dark waveform area stays up top (flat while not recording) and
            // the status line stays put — only the leading icon (stop button vs.
            // phase dot) and the hint text swap out.
            VStack(spacing: 8) {
                waveform
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(
                        Color.black.opacity(0.85),
                        in: RoundedRectangle(cornerRadius: 8, style: .continuous)
                    )
                infoRow
            }
        case let .modelNotReady(label, progress, isDownloading):
            modelNotReadyRow(label: label, progress: progress, isDownloading: isDownloading)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    /// Centered status line: a leading control — the stop button while
    /// recording (replacing the red dot), otherwise the blinking phase dot —
    /// next to the model name and a small phase hint underneath ("Stop:
    /// ⌃⇧Space" / "Transcription in progress…"), so every phase looks alike.
    private var infoRow: some View {
        HStack(spacing: 9) {
            leadingControl
            VStack(alignment: .leading, spacing: 1) {
                if !modelName.isEmpty {
                    Text(modelName)
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                if let modeName, !modeName.isEmpty {
                    Text(modeName)
                        .font(.system(size: 10))
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                if !phaseHint.isEmpty {
                    Text(phaseHint)
                        .font(.system(size: 9))
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }
            Spacer(minLength: 0)
        }
    }

    /// Fixed footprint shared by the stop button and the phase dot so the
    /// status text never shifts horizontally when the phase changes.
    private static let leadingControlSize: CGFloat = 24

    @ViewBuilder
    private var leadingControl: some View {
        if phase == .recording, let onStop {
            Button(action: onStop) {
                Image(systemName: "stop.fill")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.red)
                    .frame(width: Self.leadingControlSize, height: Self.leadingControlSize)
                    .background(Color.red.opacity(0.14), in: Circle())
            }
            .buttonStyle(.plain)
            .help(L("Stop dictation", locale: locale))
            .accessibilityLabel(L("Stop dictation", locale: locale))
        } else {
            statusDot
                .frame(width: Self.leadingControlSize, height: Self.leadingControlSize)
                .background(statusDotColor.opacity(0.15), in: Circle())
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
        case .modelNotReady:
            return ""
        }
    }

    private var statusDot: some View {
        TimelineView(.animation(minimumInterval: 0.05, paused: !isBlinkPhase)) { context in
            Circle()
                .fill(statusDotColor)
                .frame(width: 8, height: 8)
                .opacity(dotOpacity(at: context.date))
                .shadow(color: phase == .recording ? Color.red.opacity(0.6) : .clear, radius: 3)
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
        }
    }

    private var isBlinkPhase: Bool {
        switch phase {
        case .transcribing, .postProcessing: return true
        case .recording, .modelNotReady: return false
        }
    }

    @ViewBuilder
    private func modelNotReadyRow(label: String, progress: Double?, isDownloading: Bool) -> some View {
        HStack(alignment: .top, spacing: 10) {
            statusDot
                .padding(.top, 4)
            VStack(alignment: .leading, spacing: 4) {
                Text("Recording not possible", bundle: .module)
                    .font(.system(size: 13, weight: .medium))
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
        HStack(spacing: 3) {
            ForEach(Array(feed.bars.enumerated()), id: \.offset) { _, level in
                Capsule()
                    .fill(tint)
                    .frame(width: 4, height: barHeight(for: level))
                    .animation(.linear(duration: RecordingLevelFeed.pollingInterval), value: level)
            }
        }
        .frame(maxWidth: .infinity)
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

    private func barHeight(for level: Float) -> CGFloat {
        return max(2.0, normalizedLevel(level) * 48.0)
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
        case .accent: return .accentColor
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
