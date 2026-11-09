import Foundation
import XCTest

@testable import TorroWhisper

final class RecordingLevelFeedTests: XCTestCase {
    @MainActor
    func testRepeatedStartPreservesTimerGenerationAndBars() {
        var reads = 0
        let feed = RecordingLevelFeed(
            readLevels: {
                reads += 1
                return RecordingLevelsDTO(levels: [0.25], recordingId: 7)
            }, now: { 1 }, log: { _ in })
        feed.start()
        let token = feed.generation
        feed.tick(generation: token)
        let bars = feed.bars
        feed.start()
        XCTAssertEqual(feed.generation, token)
        XCTAssertEqual(feed.bars, bars)
        XCTAssertEqual(reads, 1)
        feed.stop()
    }

    @MainActor
    func testQueuedTicksCannotReadOrPublishAfterStopOrRestart() {
        var reads = 0
        let feed = RecordingLevelFeed(
            readLevels: {
                reads += 1
                return RecordingLevelsDTO(levels: [0.5], recordingId: 9)
            }, now: { 1 }, log: { _ in })
        feed.start()
        let stale = feed.generation
        feed.stop()
        feed.tick(generation: stale)
        XCTAssertEqual(reads, 0)
        XCTAssertTrue(feed.bars.allSatisfy { $0 == 0 })
        feed.start()
        feed.tick(generation: stale)
        XCTAssertEqual(reads, 0)
        feed.tick(generation: feed.generation)
        XCTAssertEqual(reads, 1)
        XCTAssertEqual(feed.bars.last, 0.5)
        feed.stop()
    }

    @MainActor
    func testBridgeFailureKeepsBarsAndDiagnosticsContainOnlyTimings() {
        var fail = false
        var clock: TimeInterval = 0
        var logs: [String] = []
        let feed = RecordingLevelFeed(
            readLevels: {
                clock += 0.002
                if fail { throw BridgeError(message: "PRIVATE_SENTINEL") }
                return RecordingLevelsDTO(levels: [0.1234567], recordingId: 42)
            }, now: { clock }, log: { logs.append($0) })
        feed.start()
        feed.tick(generation: feed.generation)
        let bars = feed.bars
        fail = true
        clock = 0.2
        feed.tick(generation: feed.generation)
        XCTAssertEqual(feed.bars, bars)
        XCTAssertTrue(logs.isEmpty)
        feed.stop()
        XCTAssertEqual(logs.count, 1)
        XCTAssertTrue(logs[0].contains("recording=42"))
        XCTAssertTrue(logs[0].contains("main_cadence=[n=1"))
        XCTAssertTrue(logs[0].contains("ffi_decode=[n=2"))
        XCTAssertTrue(logs[0].contains("errors=1"))
        XCTAssertFalse(logs[0].contains("PRIVATE_SENTINEL"))
        XCTAssertFalse(logs[0].contains("0.1234567"))
    }

    @MainActor
    func testSummariesAreRateLimitedAndRecordingCorrelationResets() {
        var clock: TimeInterval = 0
        var id: UInt64 = 1
        var logs: [String] = []
        let feed = RecordingLevelFeed(
            readLevels: {
                RecordingLevelsDTO(levels: [], recordingId: id)
            }, now: { clock }, log: { logs.append($0) })
        feed.start()
        for tick in 0..<150 {
            clock = Double(tick) / 30
            feed.tick(generation: feed.generation)
        }
        XCTAssertTrue(logs.isEmpty)
        clock = 5
        feed.tick(generation: feed.generation)
        XCTAssertEqual(logs.count, 1)
        XCTAssertTrue(logs[0].contains("recording=1"))
        id = 2
        clock = 5.1
        feed.tick(generation: feed.generation)
        feed.stop()
        XCTAssertEqual(logs.count, 3)
        XCTAssertTrue(logs[2].contains("recording=2"))
        XCTAssertTrue(logs[2].contains("ffi_decode=[n=1"))
        XCTAssertTrue(logs[2].contains("main_cadence=[n=0"))
    }

    func testTimingAggregateAndOldLevelPayloadCompatibility() throws {
        var aggregate = FeedTimingAggregate()
        aggregate.record(0.01)
        aggregate.record(0.05)
        XCTAssertEqual(aggregate.count, 2)
        XCTAssertEqual(aggregate.total, 0.06, accuracy: 0.000001)
        XCTAssertEqual(aggregate.maximum, 0.05)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let legacy = try decoder.decode(RecordingLevelsDTO.self, from: Data(#"{"levels":[]}"#.utf8))
        XCTAssertNil(legacy.recordingId)
        let correlated = try decoder.decode(RecordingLevelsDTO.self, from: Data(#"{"levels":[],"recording_id":123}"#.utf8))
        XCTAssertEqual(correlated.recordingId, 123)
    }
}
