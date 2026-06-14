import AVFoundation
import Foundation
import Security

/// Speaks chat answers through the configured backend (#17). v1 is utterance-
/// at-a-time (whole answer, then spoken); OpenAI audio is queued and played
/// serially so successive answers don't overlap. Any OpenAI failure (missing
/// key, network, bad status) silently falls back to the offline system voice.
@MainActor
final class ChatTtsPlayer: NSObject, AVAudioPlayerDelegate {
    private let synthesizer = AVSpeechSynthesizer()
    private let bridge = BridgeClient()
    private var audioPlayer: AVAudioPlayer?
    private var pendingAudio: [Data] = []
    private var openAiQueue: [String] = []
    private var synthesizing = false
    private var settings: ChatTtsSettingsDTO = ChatSettingsDTO.default.tts

    func configure(_ tts: ChatTtsSettingsDTO) {
        settings = tts
    }

    func speak(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        switch settings.provider {
        case .system:
            bridge.pluginLog(
                pluginId: "chat", level: "info",
                message: "TTS: system voice '\(settings.systemVoice.isEmpty ? "default" : settings.systemVoice)'"
            )
            speakSystem(trimmed)
        case .openAi:
            bridge.pluginLog(
                pluginId: "chat", level: "info",
                message: "TTS: OpenAI voice '\(settings.openaiVoice)'"
            )
            speakOpenAi(trimmed)
        }
    }

    func stop() {
        synthesizer.stopSpeaking(at: .immediate)
        audioPlayer?.stop()
        audioPlayer = nil
        pendingAudio.removeAll()
        openAiQueue.removeAll()
    }

    // MARK: System voice

    private func speakSystem(_ text: String) {
        let utterance = AVSpeechUtterance(string: text)
        if !settings.systemVoice.isEmpty {
            utterance.voice = AVSpeechSynthesisVoice(identifier: settings.systemVoice)
        }
        utterance.rate = Self.systemRate(settings.rate)
        synthesizer.speak(utterance)
    }

    /// Maps the normalized 0–1 rate onto AVSpeech's native range, with 0.5 ≈ the
    /// system default.
    private static func systemRate(_ normalized: Float) -> Float {
        let clamped = max(0, min(1, normalized))
        let lo = AVSpeechUtteranceMinimumSpeechRate
        let hi = AVSpeechUtteranceMaximumSpeechRate
        return lo + (hi - lo) * clamped
    }

    // MARK: OpenAI voice

    private func speakOpenAi(_ text: String) {
        openAiQueue.append(text)
        pumpOpenAi()
    }

    /// Synthesizes queued OpenAI answers strictly one at a time, so playback
    /// order matches answer order even if several answers arrive in one tick.
    private func pumpOpenAi() {
        guard !synthesizing, !openAiQueue.isEmpty else { return }
        synthesizing = true
        let text = openAiQueue.removeFirst()
        let voice = settings.openaiVoice.isEmpty ? "alloy" : settings.openaiVoice
        let rate = settings.rate
        Task { [weak self] in
            let audio = await OpenAiTts.synthesize(text: text, voice: voice, rate: rate)
            guard let self else { return }
            if let audio {
                self.pendingAudio.append(audio)
                self.playNextIfIdle()
            } else {
                // No key / network / API error → keep the answer audible offline.
                self.bridge.pluginLog(
                    pluginId: "chat", level: "warn",
                    message: "TTS: OpenAI unavailable (no API key, network or API error) — using system voice"
                )
                self.speakSystem(text)
            }
            self.synthesizing = false
            self.pumpOpenAi()
        }
    }

    private func playNextIfIdle() {
        guard audioPlayer?.isPlaying != true, !pendingAudio.isEmpty else { return }
        let data = pendingAudio.removeFirst()
        do {
            let player = try AVAudioPlayer(data: data)
            player.delegate = self
            audioPlayer = player
            player.play()
        } catch {
            audioPlayer = nil
            playNextIfIdle()
        }
    }

    nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        Task { @MainActor [weak self] in
            self?.audioPlayer = nil
            self?.playNextIfIdle()
        }
    }
}

/// OpenAI text-to-speech (`/v1/audio/speech`). Returns MP3 bytes, or nil on any
/// failure so the caller can fall back.
enum OpenAiTts {
    static func synthesize(text: String, voice: String, rate: Float) async -> Data? {
        guard let key = TtsKeychain.openAiKey(),
              let url = URL(string: "https://api.openai.com/v1/audio/speech")
        else { return nil }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = [
            "model": "gpt-4o-mini-tts",
            "input": text,
            "voice": voice,
            "response_format": "mp3",
            "speed": speed(for: rate),
        ]
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)

        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
                return nil
            }
            return data
        } catch {
            return nil
        }
    }

    /// Normalized 0–1 rate → OpenAI `speed` (0.25–4.0), centered so 0.5 ≈ 1.0×.
    private static func speed(for rate: Float) -> Double {
        let clamped = max(0, min(1, rate))
        if clamped <= 0.5 {
            return Double(0.5 + clamped) // 0 → 0.5×, 0.5 → 1.0×
        }
        return Double(1.0 + (clamped - 0.5) * 2.0) // 0.5 → 1.0×, 1 → 2.0×
    }
}

/// Reads the OpenAI API key straight from the macOS Keychain for the Swift-side
/// TTS path. Mirrors `crates/open-whisper-bridge/src/llm/keychain.rs` — the
/// service name and account string MUST stay in sync with that module.
enum TtsKeychain {
    private static let service = "dev.awesome.open-whisper.llm"
    private static let openAiAccount = "openai_api_key"

    static func openAiKey() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: openAiAccount,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data,
              let key = String(data: data, encoding: .utf8)
        else { return nil }
        let trimmed = key.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
