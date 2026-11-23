import Foundation
import XCTest

@testable import TorroWhisper

private final class Calls: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [String] = []
    func add(_ command: String, _ event: String) {
        lock.lock()
        items.append("\(event):\(command)")
        lock.unlock()
    }
    var all: [String] {
        lock.lock()
        defer { lock.unlock() }
        return items
    }
}

final class RecordingCommandRunnerTests: XCTestCase {
    @MainActor
    private func makeRunner(_ calls: Calls) -> RecordingCommandRunner {
        RecordingCommandRunner(execute: { calls.add($0, $1) }, log: { _ in })
    }

    @MainActor
    func testStartAndStopRunOncePerRecordingEvenWithRepeatedUpdates() {
        let calls = Calls()
        let runner = makeRunner(calls)
        runner.update(isRecording: false, startCommand: "on", stopCommand: "off")
        runner.update(isRecording: true, startCommand: "on", stopCommand: "off")
        runner.update(isRecording: true, startCommand: "on", stopCommand: "off")
        runner.update(isRecording: false, startCommand: "on", stopCommand: "off")
        runner.update(isRecording: false, startCommand: "on", stopCommand: "off")
        XCTAssertEqual(calls.all, ["recording_started:on", "recording_stopped:off"])
    }

    @MainActor
    func testEmptyAndBlankCommandsAreSkipped() {
        let calls = Calls()
        let runner = makeRunner(calls)
        runner.update(isRecording: true, startCommand: "", stopCommand: "  \n")
        runner.update(isRecording: false, startCommand: "", stopCommand: "  \n")
        XCTAssertTrue(calls.all.isEmpty)
    }

    @MainActor
    func testStopCommandIsTheOneThatWasActiveWhenRecordingStarted() {
        let calls = Calls()
        let runner = makeRunner(calls)
        runner.update(isRecording: true, startCommand: "on", stopCommand: "off-old")
        runner.update(isRecording: true, startCommand: "on", stopCommand: "off-new")
        runner.update(isRecording: false, startCommand: "on", stopCommand: "off-new")
        XCTAssertEqual(calls.all, ["recording_started:on", "recording_stopped:off-old"])
    }

    @MainActor
    func testFinishClosesAnOpenRecordingExactlyOnce() {
        let calls = Calls()
        let runner = makeRunner(calls)
        runner.finish()
        XCTAssertTrue(calls.all.isEmpty)
        runner.update(isRecording: true, startCommand: "on", stopCommand: "off")
        runner.finish()
        runner.finish()
        runner.update(isRecording: false, startCommand: "on", stopCommand: "off")
        XCTAssertEqual(calls.all, ["recording_started:on", "recording_stopped:off"])
    }

    @MainActor
    func testShellExecutorPassesEventAndStopsHangingCommands() throws {
        let marker = FileManager.default.temporaryDirectory
            .appendingPathComponent("torrowhisper-cmd-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: marker) }
        let runner = RecordingCommandRunner(log: { _ in })
        runner.update(
            isRecording: true,
            startCommand: "printf %s \"$TORROWHISPER_EVENT\" > '\(marker.path)'",
            stopCommand: "")
        let deadline = Date().addingTimeInterval(5)
        while !FileManager.default.fileExists(atPath: marker.path), Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        XCTAssertEqual(try String(contentsOf: marker, encoding: .utf8), "recording_started")
    }
}
