import Darwin
import SwiftUI

/// Acquire ownership before SwiftUI constructs the delegate, AppModel or bridge.
@main
enum TorroWhisperMain {
    @MainActor
    static func main() {
        let status = SingleInstanceGuard.run {
            TorroWhisperApp.main()
        }
        // Do not use NSApp.terminate here: a rejected launch must never create
        // AppKit state or invoke the delegate's settings/session cleanup.
        exit(status)
    }
}
