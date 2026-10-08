import CoreAudio
import Foundation

@MainActor
final class AudioDeviceMonitor {
    var onDevicesChanged: (() -> Void)?

    private var isRunning = false
    private var listenerBlock: AudioObjectPropertyListenerBlock?
    private var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    func start() {
        guard !isRunning else { return }

        // @Sendable is load-bearing: CoreAudio invokes this block on the utility
        // queue passed below. Without it the closure silently inherits start()'s
        // MainActor isolation — the compiler accepts it, but any build with actor
        // isolation checks enabled asserts that isolation on entry to the block,
        // before the hop below can run, and SIGTRAPs on every device change.
        let block: AudioObjectPropertyListenerBlock = { @Sendable [weak self] _, _ in
            Task { @MainActor in
                self?.onDevicesChanged?()
            }
        }

        let status = AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            DispatchQueue.global(qos: .utility),
            block
        )

        if status == noErr {
            listenerBlock = block
            isRunning = true
        }
    }

    func stop() {
        guard isRunning, let block = listenerBlock else { return }
        AudioObjectRemovePropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            DispatchQueue.global(qos: .utility),
            block
        )
        listenerBlock = nil
        isRunning = false
    }

    static func currentInputDevices() -> [(name: String, uid: String?)] {
        allDeviceIDs().compactMap { id -> (name: String, uid: String?)? in
            guard hasInputStreams(deviceID: id) else { return nil }
            let name = stringProperty(deviceID: id, selector: kAudioObjectPropertyName)
            let uid = stringProperty(deviceID: id, selector: kAudioDevicePropertyDeviceUID)
            guard let name else { return nil }
            return (name: name, uid: uid)
        }
    }

    /// Whether the named input device is muted at the CoreAudio level (#76).
    /// macOS hides this flag — System Settings only shows the input volume, so
    /// a device muted by another app (e.g. a meeting client syncing its mute
    /// button) looks "on and loud" while delivering only zeros. Returns nil
    /// when the device is unknown or has no mute control.
    static func isInputMuted(deviceName: String) -> Bool? {
        guard let deviceID = inputDeviceID(named: deviceName) else { return nil }
        for element in muteElements {
            var address = muteAddress(element: element)
            guard AudioObjectHasProperty(deviceID, &address) else { continue }
            var muted: UInt32 = 0
            var dataSize = UInt32(MemoryLayout<UInt32>.size)
            if AudioObjectGetPropertyData(deviceID, &address, 0, nil, &dataSize, &muted) == noErr {
                return muted != 0
            }
        }
        return nil
    }

    /// Clears (or sets) the CoreAudio mute flag of the named input device.
    /// Only ever called on an explicit user click — another app may have muted
    /// the device on purpose (#76). Returns true when at least one mute control
    /// accepted the new value.
    @discardableResult
    static func setInputMuted(_ muted: Bool, deviceName: String) -> Bool {
        guard let deviceID = inputDeviceID(named: deviceName) else { return false }
        var changed = false
        for element in muteElements {
            var address = muteAddress(element: element)
            var settable: DarwinBoolean = false
            guard AudioObjectHasProperty(deviceID, &address),
                  AudioObjectIsPropertySettable(deviceID, &address, &settable) == noErr,
                  settable.boolValue
            else { continue }
            var value: UInt32 = muted ? 1 : 0
            let dataSize = UInt32(MemoryLayout<UInt32>.size)
            if AudioObjectSetPropertyData(deviceID, &address, 0, nil, dataSize, &value) == noErr {
                changed = true
            }
        }
        return changed
    }

    /// Some devices expose mute on the main element, others only per channel.
    private static let muteElements: [AudioObjectPropertyElement] = [kAudioObjectPropertyElementMain, 1]

    private static func muteAddress(element: AudioObjectPropertyElement) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyMute,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: element
        )
    }

    private static func inputDeviceID(named name: String) -> AudioObjectID? {
        allDeviceIDs().first { id in
            hasInputStreams(deviceID: id)
                && stringProperty(deviceID: id, selector: kAudioObjectPropertyName) == name
        }
    }

    private static func allDeviceIDs() -> [AudioObjectID] {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )

        var dataSize: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            0,
            nil,
            &dataSize
        ) == noErr else {
            return []
        }

        let count = Int(dataSize) / MemoryLayout<AudioObjectID>.size
        guard count > 0 else { return [] }

        var ids = [AudioObjectID](repeating: 0, count: count)
        guard AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            0,
            nil,
            &dataSize,
            &ids
        ) == noErr else {
            return []
        }
        return ids
    }

    private static func hasInputStreams(deviceID: AudioObjectID) -> Bool {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreams,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain
        )
        var dataSize: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(deviceID, &address, 0, nil, &dataSize) == noErr else {
            return false
        }
        return dataSize > 0
    }

    private static func stringProperty(deviceID: AudioObjectID, selector: AudioObjectPropertySelector) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var dataSize: UInt32 = UInt32(MemoryLayout<CFString?>.size)
        var value: CFString? = nil
        let status = withUnsafeMutablePointer(to: &value) { pointer -> OSStatus in
            AudioObjectGetPropertyData(deviceID, &address, 0, nil, &dataSize, pointer)
        }
        guard status == noErr, let value else { return nil }
        return value as String
    }
}
