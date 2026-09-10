import Darwin
import Foundation
import OSLog

enum SingleInstanceGuard {
    /// Fixed per-user identity, shared by app copies, versions and SPM builds.
    /// Keep this outside caches/temp directories, which may be purged while running.
    static var lockURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/torrowhisper", isDirectory: true)
            .appendingPathComponent("instance.lock")
    }

    /// Runs all app initialization under an OS-owned lock. The injectable path
    /// lets process-level tests exercise this gate without touching user data.
    static func run(lockURL: URL = Self.lockURL, application: () -> Void) -> Int32 {
        let logger = Logger(subsystem: "com.gettorro.TorroWhisper", category: "startup")
        do {
            try FileManager.default.createDirectory(
                at: lockURL.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            // CLOEXEC keeps spawned helpers from retaining the lock after the
            // app exits. Never truncate or unlink this file: all contenders must
            // lock the same inode, even after normal shutdown or SIGKILL.
            let descriptor = lockURL.withUnsafeFileSystemRepresentation { path in
                open(path!, O_CREAT | O_RDWR | O_CLOEXEC | O_NOFOLLOW, S_IRUSR | S_IWUSR)
            }
            guard descriptor >= 0 else {
                throw posixError(operation: "open", code: errno)
            }
            defer { close(descriptor) }

            var result: Int32
            repeat {
                result = flock(descriptor, LOCK_EX | LOCK_NB)
            } while result != 0 && errno == EINTR

            if result != 0 {
                let code = errno
                guard code == EWOULDBLOCK || code == EAGAIN else {
                    throw posixError(operation: "flock", code: code)
                }
                logger.notice("TorroWhisper is already running; ignoring duplicate launch.")
                return EXIT_SUCCESS
            }

            // Synchronous for the entire SwiftUI run loop, including termination
            // callbacks. If AppKit exits the process, the kernel closes the fd.
            application()
            return EXIT_SUCCESS
        } catch {
            // Bridge logging would initialize the runtime before ownership is
            // established. Use unified logging + stderr and fail closed instead.
            let message = "TorroWhisper could not acquire its instance lock: \(error.localizedDescription)"
            logger.error("\(message, privacy: .public)")
            FileHandle.standardError.write(Data("\(message)\n".utf8))
            return EXIT_FAILURE
        }
    }

    private static func posixError(operation: String, code: Int32) -> NSError {
        NSError(
            domain: NSPOSIXErrorDomain,
            code: Int(code),
            userInfo: [NSLocalizedDescriptionKey: "\(operation): \(String(cString: strerror(code)))"]
        )
    }
}
