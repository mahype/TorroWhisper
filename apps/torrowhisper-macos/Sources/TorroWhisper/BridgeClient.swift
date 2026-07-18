import Foundation
import TorroWhisperBridgeFFI

struct BridgeError: LocalizedError {
    let message: String

    var errorDescription: String? { message }
}

final class BridgeClient {
    private let decoder: JSONDecoder
    private let encoder: JSONEncoder

    init() {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        self.decoder = decoder

        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        self.encoder = encoder
    }

    func loadSettings() throws -> AppSettings {
        try decodeResponse(from: ow_load_settings())
    }

    func saveSettings(_ settings: AppSettings) throws -> String {
        try encodeAndCall(settings, function: ow_save_settings)
    }

    func listInputDevices() throws -> [DeviceDTO] {
        try decodeResponse(from: ow_list_input_devices())
    }

    func notifyDeviceChange() throws -> MicSwitchEventDTO? {
        try decodeResponse(from: ow_notify_device_change())
    }

    func getModelStatus() throws -> ModelStatusDTO {
        try decodeResponse(from: ow_get_model_status())
    }

    func getModelStatusList() throws -> [ModelStatusDTO] {
        try decodeResponse(from: ow_get_model_status_list())
    }

    func startModelDownload(preset: ModelPreset?) throws -> String {
        try encodeAndCall(["preset": preset?.rawValue], function: ow_start_model_download)
    }

    func deleteModel(preset: ModelPreset?) throws -> String {
        try encodeAndCall(["preset": preset?.rawValue], function: ow_delete_model)
    }

    func getLlmStatusList() throws -> [LlmModelStatusDTO] {
        try decodeResponse(from: ow_get_llm_status_list())
    }

    func startLlmDownload(preset: LlmPreset) throws -> String {
        try encodeAndCall(["preset": preset.rawValue], function: ow_start_llm_download)
    }

    func deleteLlmModel(preset: LlmPreset) throws -> String {
        try encodeAndCall(["preset": preset.rawValue], function: ow_delete_llm_model)
    }

    func getCustomLlmStatusList() throws -> [CustomLlmStatusDTO] {
        try decodeResponse(from: ow_get_custom_llm_status_list())
    }

    func startCustomLlmDownload(id: String) throws -> String {
        try encodeAndCall(["id": id], function: ow_start_custom_llm_download)
    }

    func deleteCustomLlmModel(id: String) throws -> String {
        try encodeAndCall(["id": id], function: ow_delete_custom_llm_model)
    }

    func listRemoteModels(backend: RemoteModelBackend) throws -> [RemoteModelDTO] {
        try encodeAndCall(["backend": backend.rawValue], function: ow_list_remote_models)
    }

    /// Unified local + cloud model registry. Remote Ollama/LM Studio models are
    /// fetched separately via `listRemoteModels` and merged by the caller.
    func getLlmRegistry() throws -> [LlmRegistryEntryDTO] {
        try decodeResponse(from: ow_get_llm_registry())
    }

    /// Stores a cloud-provider API key in the macOS Keychain.
    func setLlmApiKey(backend: LlmBackendKind, key: String) throws -> String {
        try encodeAndCall(["backend": backend.rawValue, "key": key], function: ow_set_llm_api_key)
    }

    func deleteLlmApiKey(backend: LlmBackendKind) throws -> String {
        try encodeAndCall(["backend": backend.rawValue], function: ow_delete_llm_api_key)
    }

    /// Which cloud backends currently have a key stored (booleans only).
    func getLlmApiKeyStatus() throws -> [ApiKeyStatusDTO] {
        try decodeResponse(from: ow_get_llm_api_key_status())
    }

    /// Available post-processing pipeline stages (built-in + plugin) for the
    /// per-mode pipeline editor.
    func listPipelineStages() throws -> [StageCatalogEntry] {
        try decodeResponse(from: ow_list_pipeline_stages())
    }

    func runPermissionDiagnostics() throws -> DiagnosticsDTO {
        try decodeResponse(from: ow_run_permission_diagnostics())
    }

    func startDictation() throws -> String {
        try decodeResponse(from: ow_start_dictation())
    }

    func stopDictation() throws -> String {
        try decodeResponse(from: ow_stop_dictation())
    }

    func cancelDictation() throws -> String {
        try decodeResponse(from: ow_cancel_dictation())
    }

    func getRuntimeStatus() throws -> RuntimeStatusDTO {
        try decodeResponse(from: ow_get_runtime_status())
    }

    func getRecordingLevels() throws -> RecordingLevelsDTO {
        try decodeResponse(from: ow_get_recording_levels())
    }

    func getStreamingTranscript() throws -> StreamingTranscriptDTO {
        try decodeResponse(from: ow_get_streaming_transcript())
    }

    /// Latency breakdown of the most recent dictation (#43).
    func getLastTiming() throws -> StageTimingDTO {
        try decodeResponse(from: ow_get_last_timing())
    }

    /// Runs the whisper model & thread benchmark over the embedded reference
    /// clip (#43). Long-running — call off the main thread. `threadCounts`
    /// empty = the default 1/2/4/6/8 sweep on the active model.
    func runWhisperBenchmark(threadCounts: [UInt32] = []) throws -> BenchmarkReportDTO {
        try encodeAndCall(["thread_counts": threadCounts], function: ow_run_whisper_benchmark)
    }

    func validateHotkey(_ hotkey: String) throws -> String {
        try encodeAndCall(["hotkey": hotkey], function: ow_validate_hotkey)
    }

    func reregisterHotkey() throws -> String {
        try decodeResponse(from: ow_reregister_hotkey())
    }

    func suspendHotkey() throws -> String {
        try decodeResponse(from: ow_suspend_hotkey())
    }

    func loadHistory() throws -> [HistoryEntry] {
        try decodeResponse(from: ow_load_history())
    }

    func deleteHistoryEntry(id: String) throws -> String {
        try encodeAndCall(["id": id], function: ow_delete_history_entry)
    }

    func clearHistory() throws -> String {
        try decodeResponse(from: ow_clear_history())
    }

    func getLogPath() throws -> String {
        try decodeResponse(from: ow_get_log_path())
    }

    /// Writes a diagnostics snapshot (settings, hotkey, model inventories)
    /// into the shared log file. Returns a localized confirmation message.
    func writeDiagnosticsLog() throws -> String {
        try decodeResponse(from: ow_write_diagnostics_log())
    }

    /// Records the start of an app session in the bridge. Returns true when
    /// the previous session ended abnormally (crash, abort, kill) — the
    /// bridge detects this via a session marker file and logs it.
    @discardableResult
    func sessionStarted() -> Bool {
        struct Response: Decodable { var previousEndedAbnormally: Bool }
        let response: Response? = try? decodeResponse(from: ow_session_started())
        return response?.previousEndedAbnormally ?? false
    }

    /// Records a clean shutdown so the next launch does not report an
    /// abnormal end. Call from `applicationWillTerminate`.
    func sessionEndedCleanly() {
        let _: String? = try? decodeResponse(from: ow_session_ended_cleanly())
    }

    /// Writes a line into the shared bridge log file. Levels: "error",
    /// "warn", "debug"; anything else logs at info.
    func logMessage(level: String, message: String) {
        let payload: [String: String] = ["level": level, "message": message]
        let _: String? = try? encodeAndCall(payload, function: ow_log_message)
    }

    private func encodeAndCall<Input: Encodable, Output: Decodable>(
        _ input: Input,
        function: (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?
    ) throws -> Output {
        let payload = try encoder.encode(input)
        guard let json = String(data: payload, encoding: .utf8) else {
            throw BridgeError(message: "JSON-Payload konnte nicht erzeugt werden.")
        }

        return try json.withCString { pointer in
            try decodeResponse(from: function(pointer))
        }
    }

    private func decodeResponse<T: Decodable>(from rawPointer: UnsafeMutablePointer<CChar>?) throws -> T {
        guard let rawPointer else {
            throw BridgeError(message: "Bridge hat keinen Rueckgabewert geliefert.")
        }
        defer { ow_string_free(rawPointer) }

        let json = String(cString: rawPointer)
        guard let data = json.data(using: .utf8) else {
            throw BridgeError(message: "Bridge lieferte kein gueltiges UTF-8.")
        }

        let envelope = try decoder.decode(Envelope<T>.self, from: data)
        if envelope.ok, let value = envelope.value {
            return value
        }

        throw BridgeError(message: envelope.error ?? "Unbekannter Bridge-Fehler.")
    }
}

private struct Envelope<Value: Decodable>: Decodable {
    let ok: Bool
    let value: Value?
    let error: String?
}
