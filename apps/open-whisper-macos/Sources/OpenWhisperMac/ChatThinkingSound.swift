import AVFoundation

/// Soft, looping "thinking" cue played while the assistant is generating an
/// answer. It signals that work is happening and softens the wait — the same
/// trick voice assistants (e.g. ChatGPT voice mode) use to mask latency,
/// which matters here because an agent turn can take many seconds.
///
/// The tone is synthesized in code (a gentle periodic swell), so no audio asset
/// has to be bundled — `Resources` is excluded from SPM's resource handling in
/// this target, so a code-generated buffer is the clean option.
@MainActor
final class ChatThinkingSound {
    private let engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    private var running = false

    init() {
        engine.attach(player)
    }

    /// Starts the looping cue. No-op if already running.
    func start() {
        guard !running else { return }
        let format = engine.outputNode.outputFormat(forBus: 0)
        guard format.sampleRate > 0, let buffer = Self.makePulse(format: format) else { return }

        engine.connect(player, to: engine.mainMixerNode, format: format)
        do {
            if !engine.isRunning { try engine.start() }
        } catch {
            return
        }
        player.scheduleBuffer(buffer, at: nil, options: .loops, completionHandler: nil)
        player.play()
        running = true
    }

    /// Stops the cue. Safe to call when not running.
    func stop() {
        guard running else { return }
        running = false
        player.stop()
        engine.stop()
    }

    /// One loop period: a short raised-cosine sine swell followed by silence, so
    /// it pulses gently rather than droning. Low amplitude to stay unobtrusive.
    private static func makePulse(format: AVAudioFormat) -> AVAudioPCMBuffer? {
        let sampleRate = format.sampleRate
        let period = 1.4        // seconds between pulses
        let swell = 0.20        // seconds of audible tone
        let frequency = 396.0   // soft low-mid pitch
        let amplitude: Float = 0.11

        let frames = AVAudioFrameCount(sampleRate * period)
        guard frames > 0, let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) else {
            return nil
        }
        buffer.frameLength = frames
        guard let channels = buffer.floatChannelData else { return nil }

        let swellFrames = max(1, Int(sampleRate * swell))
        for channel in 0..<Int(format.channelCount) {
            let samples = channels[channel]
            for i in 0..<Int(frames) {
                var value: Float = 0
                if i < swellFrames {
                    let t = Double(i) / sampleRate
                    // Raised-cosine envelope → no click at on/offset.
                    let env = 0.5 - 0.5 * cos(2 * .pi * Double(i) / Double(swellFrames))
                    value = Float(sin(2 * .pi * frequency * t) * env) * amplitude
                }
                samples[i] = value
            }
        }
        return buffer
    }
}
