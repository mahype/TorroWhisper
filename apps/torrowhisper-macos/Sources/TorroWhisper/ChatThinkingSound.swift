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
        guard format.sampleRate > 0, let buffer = Self.makeMotif(format: format) else { return }

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

    /// One loop: a soft three-note rising arpeggio (a D-major triad) followed by
    /// a rest, so it reads as a gentle "thinking" motif rather than a single
    /// repeating beep. Low amplitude, soft sine + a quiet octave for warmth.
    private static func makeMotif(format: AVAudioFormat) -> AVAudioPCMBuffer? {
        let sampleRate = format.sampleRate
        let period = 2.4 // seconds per loop (motif + rest)
        let amplitude: Float = 0.10

        // (frequency Hz, start s, duration s)
        let notes: [(Double, Double, Double)] = [
            (587.33, 0.00, 0.17), // D5
            (739.99, 0.20, 0.17), // F#5
            (880.00, 0.40, 0.26), // A5
        ]

        let frames = AVAudioFrameCount(sampleRate * period)
        guard frames > 0, let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) else {
            return nil
        }
        buffer.frameLength = frames
        guard let channels = buffer.floatChannelData else { return nil }

        let total = Int(frames)
        var mono = [Float](repeating: 0, count: total)
        for (freq, start, dur) in notes {
            let startFrame = Int(start * sampleRate)
            let noteFrames = Int(dur * sampleRate)
            for i in 0..<noteFrames {
                let idx = startFrame + i
                if idx >= total { break }
                let t = Double(i) / sampleRate
                // Raised-cosine envelope → soft attack/release, no clicks.
                let env = 0.5 - 0.5 * cos(2 * .pi * Double(i) / Double(noteFrames))
                let fundamental = sin(2 * .pi * freq * t)
                let octave = 0.25 * sin(2 * .pi * freq * 2 * t)
                mono[idx] += Float((fundamental + octave) * env) * amplitude
            }
        }

        for channel in 0..<Int(format.channelCount) {
            let samples = channels[channel]
            for i in 0..<total {
                samples[i] = mono[i]
            }
        }
        return buffer
    }

    /// Stops the cue. Safe to call when not running.
    func stop() {
        guard running else { return }
        running = false
        player.stop()
        engine.stop()
    }
}
