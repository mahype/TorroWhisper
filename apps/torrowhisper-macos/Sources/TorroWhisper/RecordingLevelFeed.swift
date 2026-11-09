import Combine
import Foundation

/// Fixed-size timing aggregates only: never retain levels or transcript data
/// in diagnostics. Clock/read/log injection keeps cadence tests deterministic.
struct FeedTimingAggregate {
    private(set) var count = 0
    private(set) var total: TimeInterval = 0
    private(set) var maximum: TimeInterval = 0

    mutating func record(_ seconds: TimeInterval) {
        let value = max(0, seconds)
        count += 1
        total += value
        maximum = max(maximum, value)
    }

    var summary: String {
        "n=\(count),mean_ms=\(total * 1000 / Double(max(1, count))),max_ms=\(maximum * 1000)"
    }
}

@MainActor
final class RecordingLevelFeed: ObservableObject {
    static let barCount = 28
    static let pollingInterval: TimeInterval = 1.0 / 30.0
    static let levelGain: Float = 2.8
    static let noiseFloor: Float = 0.002

    @Published private(set) var bars: [Float] = Array(repeating: 0, count: RecordingLevelFeed.barCount)
    private(set) var generation: UInt64 = 0
    private var timer: Timer?
    private let readLevels: () throws -> RecordingLevelsDTO
    private let now: () -> TimeInterval
    private let log: (String) -> Void
    private var recordingId: UInt64?
    private var lastTick: TimeInterval?
    private var lastReport: TimeInterval = 0
    private var cadence = FeedTimingAggregate()
    private var lateness = FeedTimingAggregate()
    private var bridgeTiming = FeedTimingAggregate()
    private var errors = 0

    init(
        readLevels: (() throws -> RecordingLevelsDTO)? = nil,
        now: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
        log: ((String) -> Void)? = nil
    ) {
        let bridge = BridgeClient()
        self.readLevels = readLevels ?? { try bridge.getRecordingLevels() }
        self.now = now
        self.log = log ?? { bridge.logMessage(level: "info", message: $0) }
    }

    func start() {
        guard timer == nil else { return }
        generation &+= 1
        let token = generation
        lastTick = nil
        lastReport = now()
        let newTimer = Timer(timeInterval: Self.pollingInterval, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.tick(generation: token) }
        }
        RunLoop.main.add(newTimer, forMode: .common)
        timer = newTimer
    }

    func stop() {
        guard timer != nil else { return }
        timer?.invalidate()
        timer = nil
        generation &+= 1  // Tasks already queued by the old timer are invalid.
        report()
        resetTiming()
        recordingId = nil
        lastTick = nil
        bars = Array(repeating: 0, count: Self.barCount)
    }

    // Internal so tests can deliver queued ticks without sleeping/run-loop races.
    func tick(generation token: UInt64) {
        guard timer != nil, token == generation else { return }
        let started = now()
        let previous = lastTick
        lastTick = started
        let result = Result { try readLevels() }
        let finished = now()
        if case .success(let dto) = result, dto.recordingId != recordingId {
            report()
            resetTiming()
            recordingId = dto.recordingId
            lastReport = started
        } else if let previous {
            let gap = max(0, started - previous)
            cadence.record(gap)
            lateness.record(max(0, gap - Self.pollingInterval))
        }
        // Includes synchronous FFI + JSON decode, not logging/publication.
        // Keep on MainActor: the Rust BridgeRuntime is thread-local.
        bridgeTiming.record(finished - started)
        switch result {
        case .success(let dto):
            let slice = Array(dto.levels.suffix(Self.barCount))
            bars = Array(repeating: 0, count: Self.barCount - slice.count) + slice
        case .failure:
            errors += 1  // Keep the latest valid bars; never log error payloads.
        }
        if finished - lastReport >= 5 {
            report()
            lastReport = finished
        }
    }

    private func report() {
        guard bridgeTiming.count > 0 else { return }
        log(
            "latency recording=\(recordingId ?? 0) waveform cumulative main_cadence=[\(cadence.summary)] main_lateness=[\(lateness.summary)] ffi_decode=[\(bridgeTiming.summary)] errors=\(errors)"
        )
    }

    private func resetTiming() {
        cadence = FeedTimingAggregate()
        lateness = FeedTimingAggregate()
        bridgeTiming = FeedTimingAggregate()
        errors = 0
    }
}
