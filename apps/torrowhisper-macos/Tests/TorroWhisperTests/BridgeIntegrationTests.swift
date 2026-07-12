import XCTest
import TorroWhisperBridgeFFI

private struct Envelope: Decodable {
    let ok: Bool
    let value: String?
    let error: String?
}

final class BridgeIntegrationTests: XCTestCase {
    func testValidateHotkeyAcceptsValidCombo() throws {
        let response = try callBridge(json: #"{"hotkey":"Cmd+Shift+R"}"#)
        XCTAssertTrue(response.ok, "expected ok=true, got error: \(response.error ?? "nil")")
        XCTAssertNotNil(response.value)
    }

    func testValidateHotkeyRejectsModifierOnlyCombo() throws {
        let response = try callBridge(json: #"{"hotkey":"Ctrl+Shift"}"#)
        XCTAssertFalse(response.ok)
        XCTAssertNotNil(response.error)
    }

    private func callBridge(json: String) throws -> Envelope {
        let rawPointer = json.withCString { pointer in
            ow_validate_hotkey(pointer)
        }
        guard let rawPointer else {
            XCTFail("Bridge returned nil pointer")
            throw BridgeTestError.nilResponse
        }
        defer { ow_string_free(rawPointer) }

        let data = Data(String(cString: rawPointer).utf8)
        return try JSONDecoder().decode(Envelope.self, from: data)
    }
}

private enum BridgeTestError: Error {
    case nilResponse
}
