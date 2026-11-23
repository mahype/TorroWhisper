import Foundation

/// Runs the user's start/stop commands around a recording.
///
/// The commands are an integration point: anything that should follow the
/// recording lifecycle (muting another app's microphone, pausing media, turning
/// on a light) hangs off them without TorroWhisper knowing about it. Both run
/// via `/bin/sh -c` on a serial background queue so they can never block
/// dictation, and a command that hangs is killed after `timeout` seconds.
///
/// The stop command fires on *every* way a recording ends — normal stop, cancel,
/// error and app quit — so whatever the start command changed can always be put
/// back. Transitions are detected from `isRecording`, so repeated state updates
/// (the menu refreshes on every change) never run a command twice.
@MainActor
final class RecordingCommandRunner {
    /// Environment variable carrying the event name to the command.
    nonisolated static let eventVariable = "TORROWHISPER_EVENT"
    static let startedEvent = "recording_started"
    static let stoppedEvent = "recording_stopped"

    /// Executes one command. The default runs it through `/bin/sh -c`.
    typealias Executor = @Sendable (_ command: String, _ event: String) -> Void

    private let execute: Executor
    private var wasRecording = false
    /// The stop command that belongs to the recording currently in flight, taken
    /// from the settings at the moment it started so editing the field mid-take
    /// cannot strand the start command's side effect.
    private var pendingStopCommand: String?

    init(execute: Executor? = nil, log: (@Sendable (String) -> Void)? = nil) {
        self.execute =
            execute
            ?? Self.shellExecutor(
                log: log ?? { message in
                    BridgeClient().logMessage(level: "info", message: message)
                })
    }

    /// Feed the current recording state. Safe to call as often as you like.
    func update(isRecording: Bool, startCommand: String, stopCommand: String) {
        defer { wasRecording = isRecording }

        if isRecording, !wasRecording {
            pendingStopCommand = stopCommand
            run(startCommand, event: Self.startedEvent)
        } else if !isRecording, wasRecording {
            let command = pendingStopCommand ?? stopCommand
            pendingStopCommand = nil
            run(command, event: Self.stoppedEvent)
        }
    }

    /// Call when the app quits: closes a recording that is still open so the
    /// stop command can undo what the start command did.
    func finish() {
        guard wasRecording else { return }
        wasRecording = false
        let command = pendingStopCommand ?? ""
        pendingStopCommand = nil
        run(command, event: Self.stoppedEvent)
    }

    private func run(_ command: String, event: String) {
        let trimmed = command.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        execute(trimmed, event)
    }

    // MARK: - Default executor

    nonisolated private static let queue = DispatchQueue(label: "com.gettorro.TorroWhisper.recording-commands")

    nonisolated private static func shellExecutor(
        timeout: TimeInterval = 10,
        log: @escaping @Sendable (String) -> Void
    ) -> Executor {
        return { command, event in
            queue.async {
                let process = Process()
                process.executableURL = URL(fileURLWithPath: "/bin/sh")
                process.arguments = ["-c", command]
                var environment = ProcessInfo.processInfo.environment
                environment[eventVariable] = event
                process.environment = environment
                process.standardInput = FileHandle.nullDevice
                process.standardOutput = FileHandle.nullDevice
                process.standardError = FileHandle.nullDevice

                let finished = DispatchSemaphore(value: 0)
                process.terminationHandler = { _ in finished.signal() }
                do {
                    try process.run()
                } catch {
                    log("Recording command (\(event)) could not start: \(error.localizedDescription)")
                    return
                }
                if finished.wait(timeout: .now() + timeout) == .timedOut {
                    process.terminate()
                    log("Recording command (\(event)) timed out after \(Int(timeout)) s and was stopped.")
                } else if process.terminationStatus != 0 {
                    log("Recording command (\(event)) exited with status \(process.terminationStatus).")
                }
            }
        }
    }
}
