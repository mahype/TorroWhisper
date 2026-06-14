import AVFoundation
import Foundation

/// Speaks chat answers through the configured backend (#17). v1 is utterance-
/// at-a-time (whole answer, then spoken); OpenAI audio is synthesized in Rust
/// (which reads the API key) and played serially so successive answers don't
/// overlap. Any OpenAI failure falls back to the offline system voice, and the
/// real reason is logged.
@MainActor
final class ChatTtsPlayer: NSObject, AVAudioPlayerDelegate {
    private let synthesizer = AVSpeechSynthesizer()
    private let bridge = BridgeClient()
    private var audioPlayer: AVAudioPlayer?
    private var pendingAudio: [Data] = []
    private var synthQueue: [String] = []
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
        case .piper, .openAi:
            // .openAi is migrated to Piper (cloud TTS removed).
            bridge.pluginLog(
                pluginId: "chat", level: "info",
                message: "TTS: Piper voice '\(settings.piperVoice)'"
            )
            speakLocal(trimmed)
        }
    }

    func stop() {
        synthesizer.stopSpeaking(at: .immediate)
        audioPlayer?.stop()
        audioPlayer = nil
        pendingAudio.removeAll()
        synthQueue.removeAll()
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

    // MARK: Local Piper voice (synthesized in Rust)

    private func speakLocal(_ text: String) {
        synthQueue.append(text)
        pumpLocal()
    }

    /// Synthesizes queued answers strictly one at a time, so playback order
    /// matches answer order even if several answers arrive in one tick. Falls
    /// back to the offline system voice if local synthesis fails (e.g. the model
    /// isn't downloaded yet).
    private func pumpLocal() {
        guard !synthesizing, !synthQueue.isEmpty else { return }
        synthesizing = true
        let text = synthQueue.removeFirst()
        let voice = settings.piperVoice.isEmpty ? "de_DE-thorsten-high" : settings.piperVoice
        let rate = settings.rate
        Task { [weak self] in
            // The bridge call blocks while the Piper subprocess runs (and on the
            // first call, while the model downloads) — run it off the main actor.
            // It does not touch the bridge runtime, so this is safe.
            let result: Result<Data, Error> = await Task.detached {
                do {
                    return .success(try BridgeClient().chatTtsSynthesize(text: text, voice: voice, rate: rate))
                } catch {
                    return .failure(error)
                }
            }.value

            guard let self else { return }
            switch result {
            case .success(let audio):
                self.pendingAudio.append(audio)
                self.playNextIfIdle()
            case .failure(let error):
                let message = (error as? BridgeError)?.message ?? error.localizedDescription
                self.bridge.pluginLog(
                    pluginId: "chat", level: "warn",
                    message: "TTS: Piper failed (\(message)) — using system voice"
                )
                self.speakSystem(text)
            }
            self.synthesizing = false
            self.pumpLocal()
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
