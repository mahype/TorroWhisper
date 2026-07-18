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

    /// The live-transcript endpoint (#41) must answer with an idle snapshot
    /// (revision 0, empty text) when no recording session ever ran.
    func testStreamingTranscriptIdleSnapshot() throws {
        struct TranscriptEnvelope: Decodable {
            struct Value: Decodable {
                let revision: UInt64
                let committed: String
                let pending: String
                let isFinal: Bool
            }
            let ok: Bool
            let value: Value?
        }

        guard let rawPointer = ow_get_streaming_transcript() else {
            XCTFail("Bridge returned nil pointer")
            throw BridgeTestError.nilResponse
        }
        defer { ow_string_free(rawPointer) }

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let data = Data(String(cString: rawPointer).utf8)
        let response = try decoder.decode(TranscriptEnvelope.self, from: data)

        XCTAssertTrue(response.ok)
        let value = try XCTUnwrap(response.value)
        XCTAssertEqual(value.revision, 0)
        XCTAssertEqual(value.committed, "")
        XCTAssertEqual(value.pending, "")
        XCTAssertFalse(value.isFinal)
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
