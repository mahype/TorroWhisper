import Darwin
import Foundation

/// Headless stand-in for the app body; uses the production startup gate.
@main
enum SingleInstanceProbe {
    static func main() {
        let status = SingleInstanceGuard.run(lockURL: URL(fileURLWithPath: CommandLine.arguments[1])) {
            FileHandle.standardOutput.write(Data("started\n".utf8))
            if CommandLine.arguments.count > 2 {
                // Keep a child alive past the holder's exit to check CLOEXEC.
                let child = Process()
                child.executableURL = URL(fileURLWithPath: "/bin/sleep")
                child.arguments = ["30"]
                child.standardInput = FileHandle.nullDevice
                child.standardOutput = FileHandle.nullDevice
                child.standardError = FileHandle.nullDevice
                do {
                    try child.run()
                    FileHandle.standardOutput.write(Data("child=\(child.processIdentifier)\n".utf8))
                } catch {
                    fatalError("Could not spawn probe child: \(error)")
                }
            }
            _ = readLine()
        }
        exit(status)
    }
}
