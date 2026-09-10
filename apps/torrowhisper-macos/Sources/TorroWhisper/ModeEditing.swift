import Foundation

extension PipelineStepConfig {
    var isAIAutocorrectionEnabled: Bool {
        enabled && config?["mode"] == "llm"
    }

    var unsupportedAutocorrectionMode: String? {
        guard let mode = config?["mode"], !["off", "llm"].contains(mode) else { return nil }
        return mode
    }

    mutating func setAIAutocorrectionEnabled(_ value: Bool) {
        enabled = value
        var configuration = config ?? [:]
        configuration["mode"] = value ? "llm" : "off"
        config = configuration
    }
}

extension ProcessingMode {
    /// Explicit pipelines replace the legacy dictionary flag at runtime.
    var effectiveDictionaryEnabled: Bool {
        pipeline.isEmpty ? dictionaryEnabled : pipeline.contains { $0.stageId == "dictionary" && $0.enabled }
    }

    mutating func setDictionaryEnabled(_ value: Bool) {
        dictionaryEnabled = value
        guard !pipeline.isEmpty else { return }
        let indices = pipeline.indices.filter { pipeline[$0].stageId == "dictionary" }
        if indices.isEmpty && value {
            pipeline.insert(PipelineStepConfig(stageId: "dictionary"), at: 0)
        } else {
            for index in indices { pipeline[index].enabled = value }
        }
    }
}

extension AppSettings {
    /// Persist the candidate before exposing it to the running app. Throwing
    /// leaves the caller's settings untouched, so the editor can retry or cancel.
    func committingModeDraft(
        _ draft: ProcessingMode,
        persist: (AppSettings) throws -> Void
    ) throws -> AppSettings {
        var mode = draft
        mode.name = mode.name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !mode.name.isEmpty else {
            throw NSError(domain: "TorroWhisper.ModeEditor", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: L("Enter a name.", locale: effectiveLocale)])
        }
        var candidate = self
        if let index = candidate.modes.firstIndex(where: { $0.id == mode.id }) {
            candidate.modes[index] = mode
        } else {
            candidate.modes.append(mode)
        }
        try persist(candidate)
        return candidate
    }
}
