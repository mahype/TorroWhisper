import Foundation
import IOKit.hid

@MainActor
final class KeyboardHardwareMonitor {
    var onKeyboardChanged: (() -> Void)?

    private var manager: IOHIDManager?
    private var pendingNotify: DispatchWorkItem?
    private var isRunning = false
    private static let debounceMilliseconds: Int = 300

    func start() {
        guard !isRunning else { return }

        let manager = IOHIDManagerCreate(kCFAllocatorDefault, IOOptionBits(kIOHIDOptionsTypeNone))
        let matching: [String: Any] = [
            kIOHIDDeviceUsagePageKey: kHIDPage_GenericDesktop,
            kIOHIDDeviceUsageKey: kHIDUsage_GD_Keyboard,
        ]
        IOHIDManagerSetDeviceMatching(manager, matching as CFDictionary)

        let context = Unmanaged.passUnretained(self).toOpaque()
        IOHIDManagerRegisterDeviceMatchingCallback(manager, keyboardHardwareEventCallback, context)
        IOHIDManagerRegisterDeviceRemovalCallback(manager, keyboardHardwareEventCallback, context)

        IOHIDManagerScheduleWithRunLoop(manager, CFRunLoopGetMain(), CFRunLoopMode.defaultMode.rawValue)

        let openStatus = IOHIDManagerOpen(manager, IOOptionBits(kIOHIDOptionsTypeNone))
        guard openStatus == kIOReturnSuccess else {
            IOHIDManagerUnscheduleFromRunLoop(manager, CFRunLoopGetMain(), CFRunLoopMode.defaultMode.rawValue)
            return
        }

        self.manager = manager
        isRunning = true
    }

    func stop() {
        guard isRunning, let manager else { return }
        IOHIDManagerUnscheduleFromRunLoop(manager, CFRunLoopGetMain(), CFRunLoopMode.defaultMode.rawValue)
        IOHIDManagerClose(manager, IOOptionBits(kIOHIDOptionsTypeNone))
        self.manager = nil
        isRunning = false
        pendingNotify?.cancel()
        pendingNotify = nil
    }

    fileprivate func handleHardwareEvent() {
        pendingNotify?.cancel()
        let workItem = DispatchWorkItem { [weak self] in
            self?.onKeyboardChanged?()
        }
        pendingNotify = workItem
        DispatchQueue.main.asyncAfter(
            deadline: .now() + .milliseconds(Self.debounceMilliseconds),
            execute: workItem
        )
    }
}

private let keyboardHardwareEventCallback: IOHIDDeviceCallback = { context, _, _, _ in
    guard let context else { return }
    let monitor = Unmanaged<KeyboardHardwareMonitor>.fromOpaque(context).takeUnretainedValue()
    DispatchQueue.main.async {
        monitor.handleHardwareEvent()
    }
}
