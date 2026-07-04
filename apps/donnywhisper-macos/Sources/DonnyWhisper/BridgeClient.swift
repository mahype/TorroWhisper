import Foundation
import DonnyWhisperBridgeFFI

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

    // MARK: Hermes Agents (#agent)

    /// Stores a Hermes agent's bearer token in the Keychain (keyed by agent id).
    func setHermesApiKey(id: String, key: String) throws -> String {
        try encodeAndCall(["id": id, "key": key], function: ow_set_hermes_api_key)
    }

    func deleteHermesApiKey(id: String) throws -> String {
        try encodeAndCall(["id": id], function: ow_delete_hermes_api_key)
    }

    /// Which configured Hermes agents currently have a token stored (booleans only).
    func getHermesApiKeyStatus() throws -> [HermesKeyStatusDTO] {
        try decodeResponse(from: ow_get_hermes_api_key_status())
    }

    /// Tests reachability + auth of a Hermes agent (GET /v1/models with its
    /// stored token). Blocking network call — invoke off the main thread.
    /// Returns a short status string; throws with the failure reason.
    func testHermesAgent(id: String, baseUrl: String) throws -> String {
        try encodeAndCall(["id": id, "base_url": baseUrl], function: ow_test_hermes_agent)
    }

    /// Available post-processing pipeline stages (built-in + plugin) for the
    /// per-mode pipeline editor.
    func listPipelineStages() throws -> [StageCatalogEntry] {
        try decodeResponse(from: ow_list_pipeline_stages())
    }

    // MARK: Plugins

    /// Catalog of available plugins (what exists). Enable state lives in
    /// AppSettings.plugins and is saved through the normal settings flow.
    func getPluginCatalog() throws -> [PluginDescriptorDTO] {
        try decodeResponse(from: ow_get_plugin_catalog())
    }

    // MARK: Chat plugin

    func chatStartListening() throws -> String {
        try decodeResponse(from: ow_chat_start_listening())
    }

    func chatStopListening() throws -> String {
        try decodeResponse(from: ow_chat_stop_listening())
    }

    func chatGetState() throws -> ChatStateDTO {
        try decodeResponse(from: ow_chat_get_state())
    }

    func chatReset() {
        let _: String? = try? decodeResponse(from: ow_chat_reset())
    }

    func chatSetModel(_ modelRef: LlmModelRefDTO?) {
        struct Payload: Encodable { var modelRef: LlmModelRefDTO? }
        let _: String? = try? encodeAndCall(Payload(modelRef: modelRef), function: ow_chat_set_model)
    }

    /// Submits a typed chat message (text input alongside voice).
    func chatSendText(_ text: String) {
        let _: String? = try? encodeAndCall(["text": text], function: ow_chat_send_text)
    }

    func chatNewSession() {
        let _: String? = try? decodeResponse(from: ow_chat_new_session())
    }

    func chatSwitchSession(id: String) {
        let _: String? = try? encodeAndCall(["id": id], function: ow_chat_switch_session)
    }

    func chatDeleteSession(id: String) {
        let _: String? = try? encodeAndCall(["id": id], function: ow_chat_delete_session)
    }

    /// Synthesizes chat TTS audio in Rust (reads the OpenAI key from the
    /// Keychain there) and returns the MP3 bytes. Safe to call off the main
    /// thread — it does not touch the bridge runtime.
    func chatTtsSynthesize(text: String, voice: String, rate: Float) throws -> Data {
        struct Request: Encodable {
            var text: String
            var voice: String
            var rate: Float
        }
        struct Response: Decodable { var audio: [UInt8] }
        let response: Response = try encodeAndCall(
            Request(text: text, voice: voice, rate: rate),
            function: ow_chat_tts_synthesize
        )
        return Data(response.audio)
    }

    // MARK: Local Piper TTS

    /// Curated downloadable Piper voice ids (`{lang}-{voice}-{quality}`).
    func ttsPiperVoices() throws -> [String] {
        try decodeResponse(from: ow_tts_piper_voices())
    }

    /// Whether a Piper voice (and the shared CLI) is downloaded and ready.
    func ttsLocalReady(voice: String) throws -> Bool {
        struct Response: Decodable { var ready: Bool }
        let response: Response = try encodeAndCall(["voice": voice], function: ow_tts_local_ready)
        return response.ready
    }

    /// Downloads + extracts the Piper CLI and the given voice if missing.
    /// Blocking (large download) — invoke off the main thread.
    func ttsLocalPrepare(voice: String) throws -> String {
        try encodeAndCall(["voice": voice], function: ow_tts_local_prepare)
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

    func validateHotkey(_ hotkey: String) throws -> String {
        try encodeAndCall(["hotkey": hotkey], function: ow_validate_hotkey)
    }

    func reregisterHotkey() throws -> String {
        try decodeResponse(from: ow_reregister_hotkey())
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

    /// Writes a line into the shared bridge log file. Levels: "error",
    /// "warn", "debug"; anything else logs at info.
    func logMessage(level: String, message: String) {
        let payload: [String: String] = ["level": level, "message": message]
        let _: String? = try? encodeAndCall(payload, function: ow_log_message)
    }

    /// Central plugin logging: routes a Swift-side plugin's log line through the
    /// shared `plugin:<id>` log (same place the Rust side logs to).
    func pluginLog(pluginId: String, level: String, message: String) {
        let payload: [String: String] = [
            "plugin_id": pluginId, "level": level, "message": message,
        ]
        let _: String? = try? encodeAndCall(payload, function: ow_plugin_log)
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
