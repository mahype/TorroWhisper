import Foundation

enum StartupBehavior: String, Codable, CaseIterable, Identifiable {
    case askOnFirstLaunch = "ask_on_first_launch"
    case launchAtLogin = "launch_at_login"
    case manualLaunch = "manual_launch"

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .askOnFirstLaunch:
            return L("Ask on first launch", locale: locale)
        case .launchAtLogin:
            return L("Launch at login", locale: locale)
        case .manualLaunch:
            return L("Launch manually only", locale: locale)
        }
    }
}

enum TriggerMode: String, Codable, CaseIterable, Identifiable {
    case pushToTalk = "push_to_talk"
    case toggle

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .pushToTalk:
            return "Push-to-talk"
        case .toggle:
            return "Toggle"
        }
    }
}

enum WaveformStyle: String, CaseIterable, Identifiable {
    case centeredBars = "centered_bars"
    case line
    case envelope

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .centeredBars:
            return L("Centered bars", locale: locale)
        case .line:
            return L("Line", locale: locale)
        case .envelope:
            return L("Envelope", locale: locale)
        }
    }
}

extension WaveformStyle: Codable {
    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        let raw = try container.decode(String.self)
        self = WaveformStyle(rawValue: raw) ?? .centeredBars
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

enum WaveformColor: String, CaseIterable, Identifiable {
    case accent
    case blue
    case green
    case teal
    case orange
    case red
    case pink
    case purple

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .accent: return L("System accent", locale: locale)
        case .blue: return L("Blue", locale: locale)
        case .green: return L("Green", locale: locale)
        case .teal: return L("Teal", locale: locale)
        case .orange: return L("Orange", locale: locale)
        case .red: return L("Red", locale: locale)
        case .pink: return L("Pink", locale: locale)
        case .purple: return L("Purple", locale: locale)
        }
    }
}

extension WaveformColor: Codable {
    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        let raw = try container.decode(String.self)
        self = WaveformColor(rawValue: raw) ?? .accent
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

enum ModelPreset: String, Codable, CaseIterable, Identifiable {
    case tiny
    case light
    case standard
    case quality
    case largeV3TurboQ5_0 = "large_v3_turbo_q5_0"
    case largeV3Turbo = "large_v3_turbo"
    case largeV3 = "large_v3"

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .tiny:
            return L("Tiny", locale: locale)
        case .light:
            return L("Small", locale: locale)
        case .standard:
            return L("Medium", locale: locale)
        case .largeV3TurboQ5_0:
            return L("Turbo", locale: locale)
        case .quality:
            return L("Large", locale: locale)
        case .largeV3Turbo:
            return L("Turbo+", locale: locale)
        case .largeV3:
            return L("Maximum", locale: locale)
        }
    }

    var displayName: String {
        switch self {
        case .tiny:
            return "Whisper Tiny (78 MB)"
        case .light:
            return "Whisper Base (148 MB)"
        case .standard:
            return "Whisper Small (488 MB)"
        case .quality:
            return "Whisper Medium (1,5 GB)"
        case .largeV3TurboQ5_0:
            return "Whisper Large v3 Turbo Q5_0 (574 MB)"
        case .largeV3Turbo:
            return "Whisper Large v3 Turbo (1,6 GB)"
        case .largeV3:
            return "Whisper Large v3 (3,1 GB)"
        }
    }

    var whisperModel: String {
        switch self {
        case .tiny:
            return "tiny"
        case .light:
            return "base"
        case .standard:
            return "small"
        case .largeV3TurboQ5_0:
            return "large-v3-turbo-q5_0"
        case .quality:
            return "medium"
        case .largeV3Turbo:
            return "large-v3-turbo"
        case .largeV3:
            return "large-v3"
        }
    }

    var defaultFilename: String {
        switch self {
        case .tiny:
            return "ggml-tiny.bin"
        case .light:
            return "ggml-base.bin"
        case .standard:
            return "ggml-small.bin"
        case .largeV3TurboQ5_0:
            return "ggml-large-v3-turbo-q5_0.bin"
        case .quality:
            return "ggml-medium.bin"
        case .largeV3Turbo:
            return "ggml-large-v3-turbo.bin"
        case .largeV3:
            return "ggml-large-v3.bin"
        }
    }

    func description(locale: Locale) -> String {
        switch self {
        case .tiny:
            return L("Tiny model for very weak machines with minimal latency.", locale: locale)
        case .light:
            return L("Small local model for quick response on weaker machines.", locale: locale)
        case .standard:
            return L("Solid default for daily use and accuracy.", locale: locale)
        case .largeV3TurboQ5_0:
            return L("Quantized Turbo variant: large-v3 quality at a compact size.", locale: locale)
        case .quality:
            return L("Larger model with higher accuracy and more CPU/RAM demand.", locale: locale)
        case .largeV3Turbo:
            return L("Fast Large-v3 Turbo with high accuracy — great balance for recent Macs.", locale: locale)
        case .largeV3:
            return L("Maximum accuracy. Large download and high RAM demand.", locale: locale)
        }
    }

    var downloadSizeBytes: UInt64 {
        switch self {
        case .tiny:
            return 77_691_713
        case .light:
            return 147_951_465
        case .standard:
            return 487_601_967
        case .largeV3TurboQ5_0:
            return 574_041_195
        case .quality:
            return 1_533_763_059
        case .largeV3Turbo:
            return 1_624_555_275
        case .largeV3:
            return 3_095_033_483
        }
    }

    var downloadSizeText: String {
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        formatter.allowedUnits = [.useMB, .useGB]
        formatter.includesUnit = true
        formatter.isAdaptive = true
        return formatter.string(fromByteCount: Int64(downloadSizeBytes))
    }
}

enum LlmPreset: String, Codable, CaseIterable, Identifiable {
    case small
    case medium
    case large

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .small: return L("Small", locale: locale)
        case .medium: return L("Medium", locale: locale)
        case .large: return L("Large", locale: locale)
        }
    }

    /// Token used in `LlmModelRef::stable_id` (`local_preset:<token>`). Rust derives
    /// it from `LlmPreset::label()`, which is capitalized — MUST match exactly.
    var stableToken: String {
        switch self {
        case .small: return "Small"
        case .medium: return "Medium"
        case .large: return "Large"
        }
    }

    var displayName: String {
        switch self {
        case .small: return "Gemma 4 E2B (3.5 GB)"
        case .medium: return "Gemma 4 E4B (5.4 GB)"
        case .large: return "Gemma 4 26B (17 GB)"
        }
    }

    func description(locale: Locale) -> String {
        switch self {
        case .small:
            return L("Small language model (Gemma 4 E2B). Fast and lean, runs on 8 GB of RAM.", locale: locale)
        case .medium:
            return L("Mid-size language model (Gemma 4 E4B) — solid default for 16 GB of RAM or more.", locale: locale)
        case .large:
            return L("Large language model (Gemma 4 26B A4B, Mixture-of-Experts) with best quality — needs 32 GB of RAM or more.", locale: locale)
        }
    }

    var approxSizeLabel: String {
        switch self {
        case .small: return "ca. 3.5 GB"
        case .medium: return "ca. 5.4 GB"
        case .large: return "ca. 17 GB"
        }
    }

    var downloadSizeBytes: UInt64 {
        switch self {
        case .small: return 3_462_677_760
        case .medium: return 5_405_167_904
        case .large: return 17_035_037_632
        }
    }
}

enum ProviderKind: String, Codable, CaseIterable, Identifiable {
    case localWhisper = "local_whisper"
    case ollama
    case lmStudio = "lm_studio"

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .localWhisper:
            return "Local Whisper"
        case .ollama:
            return "Ollama"
        case .lmStudio:
            return "LM Studio"
        }
    }
}

enum PostProcessingBackend: String, Codable, CaseIterable, Identifiable {
    case local
    case ollama
    case lmStudio = "lm_studio"

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .local:
            return L("Local model", locale: locale)
        case .ollama:
            return "Ollama"
        case .lmStudio:
            return "LM Studio"
        }
    }
}

enum RemoteModelBackend: String, Codable, CaseIterable, Identifiable {
    case ollama
    case lmStudio = "lm_studio"

    var id: String { rawValue }

    func label(locale: Locale) -> String {
        switch self {
        case .ollama: return "Ollama"
        case .lmStudio: return "LM Studio"
        }
    }
}

struct RemoteModelDTO: Codable, Identifiable, Hashable {
    var backend: RemoteModelBackend
    var name: String
    var summary: String

    var id: String { "\(backend.rawValue).\(name)" }
}

enum DiagnosticStatus: String, Codable {
    case ok
    case info
    case warning
    case error

    func label(locale: Locale) -> String {
        switch self {
        case .ok:
            return "OK"
        case .info:
            return L("Note", locale: locale)
        case .warning:
            return L("Warning", locale: locale)
        case .error:
            return L("Error", locale: locale)
        }
    }
}

struct ExternalProviderSettings: Codable, Equatable {
    var endpoint: String
    var modelName: String
}

enum CustomLlmSource: Codable, Equatable, Hashable {
    case localPath(path: String)
    case downloadUrl(url: String, filename: String)

    private enum CodingKeys: String, CodingKey {
        case kind
        case path
        case url
        case filename
    }

    private enum Kind: String, Codable {
        case localPath = "local_path"
        case downloadUrl = "download_url"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let kind = try container.decode(Kind.self, forKey: .kind)
        switch kind {
        case .localPath:
            let path = try container.decode(String.self, forKey: .path)
            self = .localPath(path: path)
        case .downloadUrl:
            let url = try container.decode(String.self, forKey: .url)
            let filename = try container.decode(String.self, forKey: .filename)
            self = .downloadUrl(url: url, filename: filename)
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .localPath(let path):
            try container.encode(Kind.localPath, forKey: .kind)
            try container.encode(path, forKey: .path)
        case .downloadUrl(let url, let filename):
            try container.encode(Kind.downloadUrl, forKey: .kind)
            try container.encode(url, forKey: .url)
            try container.encode(filename, forKey: .filename)
        }
    }

    var summaryText: String {
        switch self {
        case .localPath(let path):
            return path
        case .downloadUrl(let url, _):
            return url
        }
    }
}

struct CustomLlmModel: Codable, Identifiable, Hashable, Equatable {
    var id: String
    var name: String
    var source: CustomLlmSource
}

enum PostProcessingChoice: Codable, Hashable, Identifiable {
    case localPreset(LlmPreset)
    case localCustom(id: String)
    case ollamaModel(String)
    case lmStudioModel(String)

    var id: String {
        switch self {
        case .localPreset(let preset):
            return "local.\(preset.rawValue)"
        case .localCustom(let id):
            return "custom.\(id)"
        case .ollamaModel(let name):
            return "ollama.\(name)"
        case .lmStudioModel(let name):
            return "lmStudio.\(name)"
        }
    }

    /// Cross-language model identity — MUST match `LlmModelRefDTO.stableId` (and
    /// Rust `LlmModelRef::stable_id`) for the same model, so it can be looked up
    /// in `AppSettings.enabledModelIds` (the app-wide enabled set).
    var stableId: String {
        switch self {
        case .localPreset(let preset): return "local_preset:\(preset.stableToken)"
        case .localCustom(let id): return "local_custom:\(id)"
        case .ollamaModel(let name): return "ollama:\(name)"
        case .lmStudioModel(let name): return "lmstudio:\(name)"
        }
    }

    func fallbackLabel(locale: Locale) -> String {
        switch self {
        case .localPreset(let preset):
            return "\(preset.displayName) (\(L("local", locale: locale)))"
        case .localCustom:
            return L("Custom language model (local)", locale: locale)
        case .ollamaModel(let name):
            return name.isEmpty
                ? "Ollama (\(L("no model", locale: locale)))"
                : "Ollama · \(name)"
        case .lmStudioModel(let name):
            return name.isEmpty
                ? "LM Studio (\(L("no model", locale: locale)))"
                : "LM Studio · \(name)"
        }
    }

    private enum CodingKeys: String, CodingKey {
        case kind
        case preset
        case id
        case modelName
    }

    private enum Kind: String, Codable {
        case localPreset = "local_preset"
        case localCustom = "local_custom"
        case ollama
        case lmStudio = "lm_studio"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let kind = try container.decode(Kind.self, forKey: .kind)
        switch kind {
        case .localPreset:
            let preset = try container.decode(LlmPreset.self, forKey: .preset)
            self = .localPreset(preset)
        case .localCustom:
            let id = try container.decode(String.self, forKey: .id)
            self = .localCustom(id: id)
        case .ollama:
            let name = try container.decode(String.self, forKey: .modelName)
            self = .ollamaModel(name)
        case .lmStudio:
            let name = try container.decode(String.self, forKey: .modelName)
            self = .lmStudioModel(name)
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .localPreset(let preset):
            try container.encode(Kind.localPreset, forKey: .kind)
            try container.encode(preset, forKey: .preset)
        case .localCustom(let id):
            try container.encode(Kind.localCustom, forKey: .kind)
            try container.encode(id, forKey: .id)
        case .ollamaModel(let name):
            try container.encode(Kind.ollama, forKey: .kind)
            try container.encode(name, forKey: .modelName)
        case .lmStudioModel(let name):
            try container.encode(Kind.lmStudio, forKey: .kind)
            try container.encode(name, forKey: .modelName)
        }
    }
}

// MARK: - Post-processing pipeline (Issue #16)

/// What a mode does with the transcript. Mirrors Rust `ModeKind`.
enum ModeKind: String, Codable, Hashable {
    case dictation
    case chat
}

/// One ordered pipeline step. `config` is opaque per-stage JSON; the only
/// built-in user with config today is auto-correct (`{"mode": "off"|"llm"}`),
/// so a `[String: String]` map covers v1.
struct PipelineStepConfig: Codable, Hashable, Identifiable {
    var stageId: String
    var enabled: Bool
    var config: [String: String]?

    var id: String { stageId }

    init(stageId: String, enabled: Bool = true, config: [String: String]? = nil) {
        self.stageId = stageId
        self.enabled = enabled
        self.config = config
    }
}

/// A stage available to the pipeline editor (built-in or plugin). Mirrors Rust
/// `StageCatalogEntryDto`.
struct StageCatalogEntry: Codable, Hashable, Identifiable {
    var stageId: String
    var displayName: String
    var isConfigurable: Bool
    var isPlugin: Bool

    var id: String { stageId }
}

struct ProcessingMode: Codable, Identifiable, Hashable {
    var id: String
    var name: String
    var prompt: String
    var postProcessingChoice: PostProcessingChoice?
    var dictionaryEnabled: Bool
    var kind: ModeKind
    var pipeline: [PipelineStepConfig]

    init(
        id: String,
        name: String,
        prompt: String,
        postProcessingChoice: PostProcessingChoice? = nil,
        dictionaryEnabled: Bool = true,
        kind: ModeKind = .dictation,
        pipeline: [PipelineStepConfig] = []
    ) {
        self.id = id
        self.name = name
        self.prompt = prompt
        self.postProcessingChoice = postProcessingChoice
        self.dictionaryEnabled = dictionaryEnabled
        self.kind = kind
        self.pipeline = pipeline
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.id = try container.decode(String.self, forKey: .id)
        self.name = try container.decode(String.self, forKey: .name)
        self.prompt = try container.decode(String.self, forKey: .prompt)
        self.postProcessingChoice = try container.decodeIfPresent(PostProcessingChoice.self, forKey: .postProcessingChoice)
        self.dictionaryEnabled = try container.decodeIfPresent(Bool.self, forKey: .dictionaryEnabled) ?? true
        self.kind = try container.decodeIfPresent(ModeKind.self, forKey: .kind) ?? .dictation
        self.pipeline = try container.decodeIfPresent([PipelineStepConfig].self, forKey: .pipeline) ?? []
    }

    /// The legacy default order, surfaced when the mode has no explicit pipeline.
    /// Mirrors `ProcessingMode::synthesized_pipeline` on the Rust side.
    func synthesizedPipeline(postProcessingEnabled: Bool) -> [PipelineStepConfig] {
        [
            PipelineStepConfig(stageId: "dictionary", enabled: dictionaryEnabled),
            PipelineStepConfig(stageId: "auto_correct", enabled: false, config: ["mode": "off"]),
            PipelineStepConfig(stageId: "llm", enabled: postProcessingEnabled),
        ]
    }

    static let cleanup = ProcessingMode(
        id: "cleanup",
        name: "Cleanup",
        prompt: "Fix punctuation, capitalization, and obvious recognition errors in the dictated text without changing its content. Return only the cleaned-up text."
    )
}

struct DictionaryEntry: Codable, Identifiable, Hashable {
    var id: String
    var pattern: String
    var replacement: String
    var caseSensitive: Bool
    var wholeWord: Bool

    init(id: String = UUID().uuidString, pattern: String = "", replacement: String = "", caseSensitive: Bool = false, wholeWord: Bool = true) {
        self.id = id
        self.pattern = pattern
        self.replacement = replacement
        self.caseSensitive = caseSensitive
        self.wholeWord = wholeWord
    }
}

struct HistoryEntry: Codable, Identifiable, Hashable {
    var id: String
    var text: String
    var timestamp: Int64
    var modeId: String
    var modeName: String
    var wasCancelled: Bool

    var date: Date {
        Date(timeIntervalSince1970: TimeInterval(timestamp))
    }
}

let historyMaxEntriesMin: UInt32 = 10
let historyMaxEntriesDefault: UInt32 = 100
let historyMaxEntriesLimit: UInt32 = 1000

struct TranscriptionLanguageOption: Identifiable, Hashable {
    let code: String

    var id: String { code }

    func label(locale: Locale) -> String {
        if code == "auto" {
            return L("Automatic", locale: locale)
        }
        return locale.localizedString(forLanguageCode: code)?.capitalized(with: locale)
            ?? code.uppercased()
    }

    static let automatic = TranscriptionLanguageOption(code: "auto")

    static let common: [TranscriptionLanguageOption] = [
        .automatic,
        TranscriptionLanguageOption(code: "de"),
        TranscriptionLanguageOption(code: "en"),
        TranscriptionLanguageOption(code: "fr"),
        TranscriptionLanguageOption(code: "es"),
        TranscriptionLanguageOption(code: "it"),
        TranscriptionLanguageOption(code: "nl"),
        TranscriptionLanguageOption(code: "pt"),
        TranscriptionLanguageOption(code: "tr"),
    ]

    static func option(for storedValue: String) -> TranscriptionLanguageOption? {
        let normalized = storedValue.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if normalized.isEmpty || normalized == "auto" {
            return automatic
        }
        return common.first(where: { $0.code == normalized })
    }
}

struct PreferredDevice: Codable, Equatable, Identifiable {
    var name: String
    var uid: String?
    var lastSelectedAt: Int64

    var id: String { uid ?? name }
}

// MARK: - Unified LLM registry (Issue #14)

/// Cloud vendor behind the OpenAI-compatible Chat-Completions API.
/// Raw values match the Rust `OpenAiCompatibleProvider` snake_case wire form.
enum OpenAiCompatibleProviderDTO: String, Codable, Hashable {
    case openAi = "open_ai"
    case mistral
    case deepSeek = "deep_seek"
    case grok

    /// Lowercase token used in `LlmModelRef::stable_id`. Distinct from `rawValue`
    /// (the snake_case wire form): Rust uses `openai`/`deepseek`, not
    /// `open_ai`/`deep_seek`. MUST stay in sync with Rust `LlmBackendKind::label_id`.
    var stableToken: String {
        switch self {
        case .openAi: return "openai"
        case .mistral: return "mistral"
        case .deepSeek: return "deepseek"
        case .grok: return "grok"
        }
    }
}

/// Which backend serves a model. Raw values match Rust `LlmBackendKind`.
enum LlmBackendKind: String, Codable, Hashable {
    case localGguf = "local_gguf"
    case ollama
    case lmStudio = "lm_studio"
    case openAi = "open_ai"
    case mistral
    case deepSeek = "deep_seek"
    case grok
    case anthropic
    case gemini

    var displayName: String {
        switch self {
        case .localGguf: return "Local model"
        case .ollama: return "Ollama"
        case .lmStudio: return "LM Studio"
        case .openAi: return "OpenAI"
        case .mistral: return "Mistral"
        case .deepSeek: return "DeepSeek"
        case .grok: return "Grok (xAI)"
        case .anthropic: return "Anthropic"
        case .gemini: return "Gemini"
        }
    }

    /// Cloud backends that need a single shared API key (have a Keychain slot).
    var isCloud: Bool {
        switch self {
        case .openAi, .mistral, .deepSeek, .grok, .anthropic, .gemini: return true
        case .localGguf, .ollama, .lmStudio: return false
        }
    }
}

enum LlmAvailability: String, Codable, Hashable {
    case ready
    case downloadable
    case downloading
    case corrupt
    case needsApiKey = "needs_api_key"
}

/// Backend-independent model identity. Tagged-enum mirror of Rust `LlmModelRef`
/// (same `{ "kind": ... }` wire form as `PostProcessingChoice`).
enum LlmModelRefDTO: Codable, Hashable {
    case localPreset(LlmPreset)
    case localCustom(id: String)
    case ollama(String)
    case lmStudio(String)
    case openAiCompatible(provider: OpenAiCompatibleProviderDTO, modelName: String)
    case anthropic(String)
    case gemini(String)

    private enum CodingKeys: String, CodingKey {
        case kind
        case preset
        case id
        case modelName
        case provider
    }

    private enum Kind: String, Codable {
        case localPreset = "local_preset"
        case localCustom = "local_custom"
        case ollama
        case lmStudio = "lm_studio"
        case openAiCompatible = "open_ai_compatible"
        case anthropic
        case gemini
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(Kind.self, forKey: .kind) {
        case .localPreset:
            self = .localPreset(try container.decode(LlmPreset.self, forKey: .preset))
        case .localCustom:
            self = .localCustom(id: try container.decode(String.self, forKey: .id))
        case .ollama:
            self = .ollama(try container.decode(String.self, forKey: .modelName))
        case .lmStudio:
            self = .lmStudio(try container.decode(String.self, forKey: .modelName))
        case .openAiCompatible:
            self = .openAiCompatible(
                provider: try container.decode(OpenAiCompatibleProviderDTO.self, forKey: .provider),
                modelName: try container.decode(String.self, forKey: .modelName)
            )
        case .anthropic:
            self = .anthropic(try container.decode(String.self, forKey: .modelName))
        case .gemini:
            self = .gemini(try container.decode(String.self, forKey: .modelName))
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .localPreset(let preset):
            try container.encode(Kind.localPreset, forKey: .kind)
            try container.encode(preset, forKey: .preset)
        case .localCustom(let id):
            try container.encode(Kind.localCustom, forKey: .kind)
            try container.encode(id, forKey: .id)
        case .ollama(let name):
            try container.encode(Kind.ollama, forKey: .kind)
            try container.encode(name, forKey: .modelName)
        case .lmStudio(let name):
            try container.encode(Kind.lmStudio, forKey: .kind)
            try container.encode(name, forKey: .modelName)
        case .openAiCompatible(let provider, let modelName):
            try container.encode(Kind.openAiCompatible, forKey: .kind)
            try container.encode(provider, forKey: .provider)
            try container.encode(modelName, forKey: .modelName)
        case .anthropic(let name):
            try container.encode(Kind.anthropic, forKey: .kind)
            try container.encode(name, forKey: .modelName)
        case .gemini(let name):
            try container.encode(Kind.gemini, forKey: .kind)
            try container.encode(name, forKey: .modelName)
        }
    }

    /// Canonical selection token. MUST match Rust `LlmModelRef::stable_id` exactly
    /// — it is the cross-language key into `AppSettings.enabledModelIds` and the
    /// registry's `stableId`.
    var stableId: String {
        switch self {
        case .localPreset(let preset): return "local_preset:\(preset.stableToken)"
        case .localCustom(let id): return "local_custom:\(id)"
        case .ollama(let name): return "ollama:\(name)"
        case .lmStudio(let name): return "lmstudio:\(name)"
        case .openAiCompatible(let provider, let modelName):
            return "\(provider.stableToken):\(modelName)"
        case .anthropic(let name): return "anthropic:\(name)"
        case .gemini(let name): return "gemini:\(name)"
        }
    }
}

struct LlmRegistryEntryDTO: Codable, Hashable, Identifiable {
    var stableId: String
    var modelRef: LlmModelRefDTO
    var backendKind: LlmBackendKind
    var displayName: String
    var detail: String
    var availability: LlmAvailability
    /// Whether the user enabled this model app-wide (mirrors Rust
    /// `LlmRegistryEntryDto.enabled`). Literal toggle state; pickers apply the
    /// "empty curation = show all" fallback themselves.
    var enabled: Bool = false
    var progressBasisPoints: UInt16?

    var id: String { stableId }
}

struct ApiKeyStatusDTO: Codable, Hashable, Identifiable {
    var backend: LlmBackendKind
    var hasKey: Bool

    var id: LlmBackendKind { backend }
}

struct AppSettings: Codable, Equatable {
    var onboardingCompleted: Bool
    var startupBehavior: StartupBehavior
    var inputDeviceName: String
    var preferredInputDevices: [PreferredDevice]
    var autoSwitchMicOnHotplug: Bool
    var showMicSwitchNotifications: Bool
    var hotkey: String
    var triggerMode: TriggerMode
    var transcriptionLanguage: String
    var insertTextAutomatically: Bool
    var insertDelayMs: UInt32
    var restoreClipboardAfterInsert: Bool
    var vadEnabled: Bool
    var vadThreshold: Float
    var vadSilenceMs: UInt32
    var showRecordingIndicator: Bool
    var waveformStyle: WaveformStyle
    var waveformColor: WaveformColor
    var largeRecordingIndicator: Bool
    var highContrastRecordingIndicator: Bool
    /// Live transcription in the recording bubble while speaking (#41). Only
    /// the final pass is ever inserted. Mirrors Rust `live_transcription_enabled`.
    var liveTranscriptionEnabled: Bool
    var saveAudioRecordings: Bool
    var saveTranscripts: Bool
    var saveDirectory: String
    var localModel: ModelPreset
    var localModelPath: String
    var localLlm: LlmPreset
    var localLlmPath: String
    var localLlmAutoUnloadSecs: UInt32
    var activeProvider: ProviderKind
    var activePostProcessingBackend: PostProcessingBackend
    var activeCustomLlmId: String
    var customLlmModels: [CustomLlmModel]
    /// Registry-selected post-processing model (incl. cloud). nil = legacy
    /// PostProcessingChoice resolution.
    var activePostProcessingModel: LlmModelRefDTO?
    /// Stable IDs of models enabled app-wide. Mirrors Rust
    /// `AppSettings.enabled_model_ids`. Empty = "show all" (the pickers' fallback).
    /// This is the general/post-processing LLM role's curation list.
    var enabledModelIds: [String]
    /// Per-role curation for transcription (Whisper) models (#28 AP1). Empty = all.
    var enabledTranscriptionIds: [String]
    var ollama: ExternalProviderSettings
    var lmStudio: ExternalProviderSettings
    var postProcessingEnabled: Bool
    var modes: [ProcessingMode]
    var activeModeId: String
    var uiLanguage: UiLanguage
    var dictionary: [DictionaryEntry]
    var historyEnabled: Bool
    var historyMaxEntries: UInt32
    /// Whisper inference thread count; 0 = auto (#43).
    var whisperThreadCount: UInt32
    /// Force a single Whisper segment (expert, #43).
    var whisperSingleSegment: Bool

    static let `default` = AppSettings(
        onboardingCompleted: false,
        startupBehavior: .askOnFirstLaunch,
        inputDeviceName: "System Default",
        preferredInputDevices: [],
        autoSwitchMicOnHotplug: true,
        showMicSwitchNotifications: true,
        hotkey: "Ctrl+Shift+Space",
        triggerMode: .toggle,
        transcriptionLanguage: "auto",
        insertTextAutomatically: true,
        insertDelayMs: 120,
        restoreClipboardAfterInsert: true,
        vadEnabled: false,
        vadThreshold: 0.014,
        vadSilenceMs: 900,
        showRecordingIndicator: true,
        waveformStyle: .centeredBars,
        waveformColor: .accent,
        largeRecordingIndicator: false,
        highContrastRecordingIndicator: false,
        liveTranscriptionEnabled: true,
        saveAudioRecordings: false,
        saveTranscripts: false,
        saveDirectory: "",
        localModel: .standard,
        localModelPath: "",
        localLlm: .medium,
        localLlmPath: "",
        localLlmAutoUnloadSecs: 180,
        activeProvider: .localWhisper,
        activePostProcessingBackend: .local,
        activeCustomLlmId: "",
        customLlmModels: [],
        activePostProcessingModel: nil,
        enabledModelIds: [],
        enabledTranscriptionIds: [],
        ollama: ExternalProviderSettings(endpoint: "http://127.0.0.1:11434", modelName: "whisper"),
        lmStudio: ExternalProviderSettings(endpoint: "http://127.0.0.1:1234", modelName: "openai/whisper-small"),
        postProcessingEnabled: false,
        modes: [.cleanup],
        activeModeId: "cleanup",
        uiLanguage: .system,
        dictionary: [],
        historyEnabled: true,
        historyMaxEntries: historyMaxEntriesDefault,
        whisperThreadCount: 0,
        whisperSingleSegment: false
    )
}

enum UiLanguage: String, Codable, CaseIterable, Identifiable {
    case system
    case en
    case de

    var id: String { rawValue }
}

struct DeviceDTO: Codable, Identifiable {
    var name: String
    var isSelected: Bool
    var uid: String?

    var id: String { name }
}

struct ModelStatusDTO: Codable, Identifiable, Equatable {
    var presetLabel: String
    var backendModelName: String
    var path: String
    var summary: String
    var isDownloaded: Bool
    var isDownloading: Bool
    var isCorrupt: Bool
    var progressBasisPoints: UInt16?
    var expectedSizeBytes: UInt64

    var id: String { backendModelName }

    static let empty = ModelStatusDTO(
        presetLabel: "Whisper Small",
        backendModelName: "small",
        path: "",
        summary: "No model status loaded yet.",
        isDownloaded: false,
        isDownloading: false,
        isCorrupt: false,
        progressBasisPoints: nil,
        expectedSizeBytes: ModelPreset.standard.downloadSizeBytes
    )
}

struct CustomLlmStatusDTO: Codable, Identifiable, Hashable {
    var id: String
    var name: String
    var sourceLabel: String
    var path: String
    var isDownloaded: Bool
    var isDownloading: Bool
    var isLoaded: Bool
    var needsDownload: Bool
    var progressBasisPoints: UInt16?
}

struct LlmModelStatusDTO: Codable, Identifiable, Equatable {
    var presetLabel: String
    var displayLabel: String
    var path: String
    var summary: String
    var isDownloaded: Bool
    var isDownloading: Bool
    var isLoaded: Bool
    var progressBasisPoints: UInt16?
    var expectedSizeBytes: UInt64

    var id: String { presetLabel }
}

struct DiagnosticItemDTO: Codable, Identifiable {
    var title: String
    var status: DiagnosticStatus
    var problem: String
    var recommendation: String

    var id: String { title + problem }
}

struct DiagnosticsDTO: Codable {
    var summary: String
    var items: [DiagnosticItemDTO]

    static let empty = DiagnosticsDTO(summary: "Diagnostics loading.", items: [])
}

struct RecordingLevelsDTO: Codable {
    var levels: [Float]

    static let empty = RecordingLevelsDTO(levels: [])
}

/// Live-transcript snapshot from `ow_get_streaming_transcript` (#41).
/// `revision` is globally monotonic on the Rust side — consumers keep the
/// highest value they have seen and ignore anything not strictly newer.
struct StreamingTranscriptDTO: Codable, Equatable {
    var revision: UInt64
    var committed: String
    var pending: String
    var isFinal: Bool
}

struct RuntimeStatusDTO: Codable, Equatable {
    var isRecording: Bool
    var isTranscribing: Bool
    var isPostProcessing: Bool
    var isCancelling: Bool
    var lastStatus: String
    var lastTranscript: String
    var lastDictationError: String
    var dictationErrorCount: UInt64
    var dictationSuccessCount: UInt64
    var dictationTriggerCount: UInt64
    var hotkeyRegistered: Bool
    var hotkeyText: String
    var startupSummary: String
    var providerSummary: String
    var activeModeName: String
    var onboardingCompleted: Bool
    var dictationBlockedByMissingModel: Bool
    var blockedModelLabel: String
    var blockedModelIsDownloading: Bool
    var blockedModelProgressBasisPoints: UInt16?
    var activeInputDeviceName: String
    var lastMicSwitchMessage: String
    var micSwitchEventCount: UInt64
    var historyRevision: UInt64
    var dictationModelWarming: Bool

    static let empty = RuntimeStatusDTO(
        isRecording: false,
        isTranscribing: false,
        isPostProcessing: false,
        isCancelling: false,
        lastStatus: "TorroWhisper is starting.",
        lastTranscript: "",
        lastDictationError: "",
        dictationErrorCount: 0,
        dictationSuccessCount: 0,
        dictationTriggerCount: 0,
        hotkeyRegistered: false,
        hotkeyText: "Ctrl+Shift+Space",
        startupSummary: "Startup status not synchronized yet.",
        providerSummary: "Local Whisper",
        activeModeName: "Standard",
        onboardingCompleted: false,
        dictationBlockedByMissingModel: false,
        blockedModelLabel: "",
        blockedModelIsDownloading: false,
        blockedModelProgressBasisPoints: nil,
        activeInputDeviceName: "",
        lastMicSwitchMessage: "",
        micSwitchEventCount: 0,
        historyRevision: 0,
        dictationModelWarming: false
    )
}

/// Per-stage latency of the most recent dictation (#43). Mirrors the Rust
/// `StageTimingDto`; `revision == 0` means no dictation has been measured yet.
struct StageTimingDTO: Codable, Equatable {
    var audioSecs: Float
    var whisperLoadSecs: Float
    var resampleSecs: Float
    var stateSecs: Float
    var inferenceSecs: Float
    var postProcessingSecs: Float
    var insertionSecs: Float
    var totalAfterStopSecs: Float
    var realTimeFactor: Float
    var revision: UInt64

    static let empty = StageTimingDTO(
        audioSecs: 0, whisperLoadSecs: 0, resampleSecs: 0, stateSecs: 0,
        inferenceSecs: 0, postProcessingSecs: 0, insertionSecs: 0,
        totalAfterStopSecs: 0, realTimeFactor: 0, revision: 0
    )
}

/// One measured benchmark run (#43). Mirrors the Rust `BenchmarkRowDto`.
struct BenchmarkRowDTO: Codable, Equatable, Identifiable {
    var kind: String
    var modelLabel: String
    var modelAvailable: Bool
    var threadCount: UInt32
    var loadSecs: Float
    var inferenceSecs: Float
    var realTimeFactor: Float
    var loadRssMb: Float
    var qualityScore: Float
    var transcript: String
    var note: String

    // Stable identity for SwiftUI lists: kind + model + thread count is unique
    // within a report.
    var id: String { "\(kind)-\(modelLabel)-\(threadCount)" }
}

/// Result of a benchmark run (#43). Mirrors the Rust `BenchmarkReportDto`.
struct BenchmarkReportDTO: Codable, Equatable {
    var audioSecs: Float
    var referenceText: String
    var rows: [BenchmarkRowDTO]
}

struct MicSwitchEventDTO: Codable {
    var from: String
    var to: String
    var wasRecording: Bool
    var message: String
}
