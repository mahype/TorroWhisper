import XCTest
@testable import TorroWhisper

final class ModeEditingTests: XCTestCase {
    func testFailedSaveDoesNotExposeDraftOrChangeActiveProfile() throws {
        let original = AppSettings.default
        var draft = try XCTUnwrap(original.modes.first)
        draft.name = "Changed"
        draft.prompt = "Changed prompt"
        draft.pipeline = [PipelineStepConfig(stageId: "auto_correct", config: ["mode": "llm"])]
        var current = original
        enum Failure: Error { case diskFull }
        XCTAssertThrowsError(try {
            current = try current.committingModeDraft(draft) { candidate in
                XCTAssertEqual(candidate.modes.first?.prompt, "Changed prompt")
                throw Failure.diskFull
            }
        }())
        XCTAssertEqual(current, original)
    }

    func testSavePreservesMetadataAndSelectionAndReplacesOnlyOneProfile() throws {
        var original = AppSettings.default
        let untouched = ProcessingMode(id: "untouched", name: "Other", prompt: "Other prompt")
        original.modes.append(untouched)
        original.activeModeId = untouched.id
        var draft = try XCTUnwrap(original.modes.first)
        draft.name = "  Revised  "
        draft.prompt = "First line\nSecond line"
        draft.dictionaryEnabled = false
        draft.pipeline = [PipelineStepConfig(stageId: "plugin", enabled: false, config: ["custom": "preserved"])]
        var writes = 0
        let saved = try original.committingModeDraft(draft) { candidate in
            writes += 1
            XCTAssertEqual(candidate.modes.first?.name, "Revised")
            // This is the complete value sent through the settings boundary.
            let roundTrip = try JSONDecoder().decode(AppSettings.self, from: JSONEncoder().encode(candidate))
            XCTAssertEqual(roundTrip, candidate)
        }
        XCTAssertEqual(writes, 1)
        XCTAssertEqual(saved.activeModeId, untouched.id)
        XCTAssertEqual(saved.modes.last, untouched)
        XCTAssertEqual(saved.modes.first?.pipeline, draft.pipeline)
        XCTAssertEqual(saved.modes.first?.prompt, draft.prompt)
        XCTAssertEqual(saved.modes.first?.kind, draft.kind)
        XCTAssertFalse(try XCTUnwrap(saved.modes.first).dictionaryEnabled)
    }

    func testNewDraftOnlyAppearsAfterSuccessfulCommit() throws {
        let original = AppSettings.default
        let draft = ProcessingMode(id: "new-id", name: "New profile", prompt: "")
        // Opening/editing/canceling owns a value, never adds it to the settings.
        XCTAssertFalse(original.modes.contains { $0.id == draft.id })
        let saved = try original.committingModeDraft(draft) { _ in }
        XCTAssertEqual(saved.modes.count, original.modes.count + 1)
        XCTAssertEqual(saved.activeModeId, original.activeModeId)
        let savedAgain = try saved.committingModeDraft(draft) { _ in }
        XCTAssertEqual(savedAgain.modes.count, saved.modes.count)
    }

    func testBlankNameNeverCallsPersistence() {
        let draft = ProcessingMode(id: "blank", name: " \n\t ", prompt: "")
        XCTAssertThrowsError(try AppSettings.default.committingModeDraft(draft) { _ in
            XCTFail("Invalid draft reached persistence")
        })
    }

    func testAutocorrectionCheckboxHandlesLegacyValuesAndPreservesExtraConfiguration() {
        for mode in ["off", "llm", "spell_check", "future"] {
            for enabled in [false, true] {
                var step = PipelineStepConfig(stageId: "auto_correct", enabled: enabled, config: ["mode": mode, "extra": "keep"])
                let original = step
                XCTAssertEqual(step.isAIAutocorrectionEnabled, enabled && mode == "llm")
                XCTAssertEqual(step.unsupportedAutocorrectionMode, ["off", "llm"].contains(mode) ? nil : mode)
                XCTAssertEqual(step, original, "Reading legacy state must not normalize it")
                step.setAIAutocorrectionEnabled(true)
                XCTAssertTrue(step.enabled)
                XCTAssertEqual(step.config, ["mode": "llm", "extra": "keep"])
                step.setAIAutocorrectionEnabled(false)
                XCTAssertFalse(step.enabled)
                XCTAssertEqual(step.config, ["mode": "off", "extra": "keep"])
            }
        }
    }

    func testDictionaryUsesExplicitPipelineAndKeepsBothControlsConsistent() {
        var mode = ProcessingMode(id: "test", name: "Test", prompt: "", dictionaryEnabled: true,
                                  pipeline: [PipelineStepConfig(stageId: "llm"), PipelineStepConfig(stageId: "dictionary", enabled: false)])
        XCTAssertFalse(mode.effectiveDictionaryEnabled)
        mode.setDictionaryEnabled(true)
        XCTAssertTrue(mode.effectiveDictionaryEnabled)
        XCTAssertEqual(mode.pipeline.map(\.stageId), ["llm", "dictionary"])
        mode.setDictionaryEnabled(false)
        XCTAssertFalse(mode.dictionaryEnabled)
        XCTAssertFalse(mode.effectiveDictionaryEnabled)
        mode.pipeline = [PipelineStepConfig(stageId: "llm")]
        mode.setDictionaryEnabled(true)
        XCTAssertEqual(mode.pipeline.map(\.stageId), ["dictionary", "llm"])
    }
}
