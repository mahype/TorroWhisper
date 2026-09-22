import XCTest
@testable import TorroWhisper

final class SmokeTests: XCTestCase {
    func testHarnessRuns() {
        XCTAssertEqual(1 + 1, 2)
    }

    func testHoldToDictateIsTheDefaultTriggerMode() {
        XCTAssertEqual(AppSettings.default.triggerMode, .pushToTalk)
    }
}
