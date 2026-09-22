import XCTest
@testable import TorroWhisper

final class SmokeTests: XCTestCase {
    func testHarnessRuns() {
        XCTAssertEqual(1 + 1, 2)
    }

    func testHoldToDictateIsTheDefaultTriggerMode() {
        XCTAssertEqual(AppSettings.default.triggerMode, .pushToTalk)
    }

    func testRecordingLeavesSystemOutputUnchangedByDefault() {
        XCTAssertEqual(AppSettings.default.recordingOutputVolumePercent, 100)
    }

    func testRecordingOutputAttenuationIsRelativeAndClamped() {
        XCTAssertEqual(
            RecordingOutputVolumeController.attenuatedVolume(original: 0.8, percent: 25),
            0.2,
            accuracy: 0.0001
        )
        XCTAssertEqual(
            RecordingOutputVolumeController.attenuatedVolume(original: 0.8, percent: 250),
            0.8,
            accuracy: 0.0001
        )
    }
}
