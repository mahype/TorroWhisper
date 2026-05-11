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

struct ProcessingMode: Codable, Identifiable, Hashable {
    var id: String
    var name: String
    var prompt: String
    var postProcessingChoice: PostProcessingChoice?

    init(id: String, name: String, prompt: String, postProcessingChoice: PostProcessingChoice? = nil) {
        self.id = id
        self.name = name
        self.prompt = prompt
        self.postProcessingChoice = postProcessingChoice
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.id = try container.decode(String.self, forKey: .id)
        self.name = try container.decode(String.self, forKey: .name)
        self.prompt = try container.decode(String.self, forKey: .prompt)
        self.postProcessingChoice = try container.decodeIfPresent(PostProcessingChoice.self, forKey: .postProcessingChoice)
    }

    static let cleanup = ProcessingMode(
        id: "cleanup",
        name: "Cleanup",
        prompt: "Fix punctuation, capitalization, and obvious recognition errors in the dictated text without changing its content. Return only the cleaned-up text."
    )
}

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
    var localModel: ModelPreset
    var localModelPath: String
    var localLlm: LlmPreset
    var localLlmPath: String
    var localLlmAutoUnloadSecs: UInt32
    var activeProvider: ProviderKind
    var activePostProcessingBackend: PostProcessingBackend
    var activeCustomLlmId: String
    var customLlmModels: [CustomLlmModel]
    var ollama: ExternalProviderSettings
    var lmStudio: ExternalProviderSettings
    var postProcessingEnabled: Bool
    var modes: [ProcessingMode]
    var activeModeId: String
    var uiLanguage: UiLanguage

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
        localModel: .standard,
        localModelPath: "",
        localLlm: .medium,
        localLlmPath: "",
        localLlmAutoUnloadSecs: 180,
        activeProvider: .localWhisper,
        activePostProcessingBackend: .local,
        activeCustomLlmId: "",
        customLlmModels: [],
        ollama: ExternalProviderSettings(endpoint: "http://127.0.0.1:11434", modelName: "whisper"),
        lmStudio: ExternalProviderSettings(endpoint: "http://127.0.0.1:1234", modelName: "openai/whisper-small"),
        postProcessingEnabled: false,
        modes: [.cleanup],
        activeModeId: "cleanup",
        uiLanguage: .system
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

struct ModelStatusDTO: Codable, Identifiable {
    var presetLabel: String
    var backendModelName: String
    var path: String
    var summary: String
    var isDownloaded: Bool
    var isDownloading: Bool
    var progressBasisPoints: UInt16?
    var expectedSizeBytes: UInt64

    var id: String { backendModelName }

    static let empty = ModelStatusDTO(
        presetLabel: "Whisper Small",
        backendModelName: "small",
        path: "",
        summary: "Noch kein Modellstatus geladen.",
        isDownloaded: false,
        isDownloading: false,
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

struct LlmModelStatusDTO: Codable, Identifiable {
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

    static let empty = DiagnosticsDTO(summary: "Diagnose wird geladen.", items: [])
}

struct RecordingLevelsDTO: Codable {
    var levels: [Float]

    static let empty = RecordingLevelsDTO(levels: [])
}

struct RuntimeStatusDTO: Codable {
    var isRecording: Bool
    var isTranscribing: Bool
    var isPostProcessing: Bool
    var lastStatus: String
    var lastTranscript: String
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

    static let empty = RuntimeStatusDTO(
        isRecording: false,
        isTranscribing: false,
        isPostProcessing: false,
        lastStatus: "Open Whisper wird gestartet.",
        lastTranscript: "",
        dictationTriggerCount: 0,
        hotkeyRegistered: false,
        hotkeyText: "Ctrl+Shift+Space",
        startupSummary: "Systemstart noch nicht synchronisiert.",
        providerSummary: "Local Whisper",
        activeModeName: "Standard",
        onboardingCompleted: false,
        dictationBlockedByMissingModel: false,
        blockedModelLabel: "",
        blockedModelIsDownloading: false,
        blockedModelProgressBasisPoints: nil,
        activeInputDeviceName: "",
        lastMicSwitchMessage: "",
        micSwitchEventCount: 0
    )
}

struct MicSwitchEventDTO: Codable {
    var from: String
    var to: String
    var wasRecording: Bool
    var message: String
}
