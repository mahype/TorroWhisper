import CoreAudio
import Foundation

/// Narrow Core Audio bridge for temporarily lowering the current system output.
/// SwiftUI owns the requested percentage; this object only snapshots, applies,
/// and restores device properties for the lifetime of a recording.
@MainActor
final class RecordingOutputVolumeController {
    private struct ScalarSnapshot {
        let element: AudioObjectPropertyElement
        let value: Float32
    }

    private struct MuteSnapshot {
        let element: AudioObjectPropertyElement
        let value: UInt32
    }

    private struct DeviceSnapshot {
        let deviceID: AudioDeviceID
        let volumes: [ScalarSnapshot]
        let mutes: [MuteSnapshot]
    }

    private var isRecording = false
    private var requestedPercent: UInt32 = 100
    private var snapshot: DeviceSnapshot?
    private var isMonitoring = false
    private var listenerBlock: AudioObjectPropertyListenerBlock?
    private let listenerQueue = DispatchQueue(label: "com.gettorro.TorroWhisper.default-output-listener")
    private var defaultOutputAddress = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    func startMonitoring() {
        guard !isMonitoring else { return }

        // Core Audio calls on the queue above, so hop explicitly to the owner
        // actor before touching the recording snapshot.
        let block: AudioObjectPropertyListenerBlock = { @Sendable [weak self] _, _ in
            Task { @MainActor in
                self?.defaultOutputDeviceDidChange()
            }
        }
        let status = AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &defaultOutputAddress,
            listenerQueue,
            block
        )
        guard status == noErr else { return }
        listenerBlock = block
        isMonitoring = true
    }

    func stopMonitoring() {
        guard isMonitoring, let listenerBlock else { return }
        AudioObjectRemovePropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &defaultOutputAddress,
            listenerQueue,
            listenerBlock
        )
        self.listenerBlock = nil
        isMonitoring = false
    }

    func update(isRecording: Bool, outputVolumePercent: UInt32) {
        let percent = min(outputVolumePercent, 100)

        guard isRecording else {
            restoreCurrentDevice()
            self.isRecording = false
            requestedPercent = percent
            return
        }

        let startedRecording = !self.isRecording
        let percentageChanged = requestedPercent != percent
        self.isRecording = true
        requestedPercent = percent

        guard startedRecording || percentageChanged else { return }
        restoreCurrentDevice()
        applyToCurrentDevice()
    }

    /// Restores the captured values even if recording ends through cancellation,
    /// an error, or normal app termination.
    func restore() {
        restoreCurrentDevice()
        isRecording = false
    }

    nonisolated static func attenuatedVolume(original: Float32, percent: UInt32) -> Float32 {
        let factor = Float32(min(percent, 100)) / 100
        return max(0, min(original * factor, 1))
    }

    private func defaultOutputDeviceDidChange() {
        guard isRecording else { return }
        restoreCurrentDevice()
        applyToCurrentDevice()
    }

    private func applyToCurrentDevice() {
        guard requestedPercent < 100, let deviceID = currentOutputDevice() else { return }

        if requestedPercent == 0 {
            let originalMutes = readMutes(deviceID: deviceID)
            let changedMutes = originalMutes.filter {
                writeMute(1, deviceID: deviceID, element: $0.element)
            }
            if !changedMutes.isEmpty, changedMutes.count == originalMutes.count {
                snapshot = DeviceSnapshot(deviceID: deviceID, volumes: [], mutes: changedMutes)
                return
            }
            // A device with multiple software-controlled channels must be
            // changed atomically. Do not leave only one channel muted.
            for mute in changedMutes {
                _ = writeMute(mute.value, deviceID: deviceID, element: mute.element)
            }
        }

        let originalVolumes = readVolumes(deviceID: deviceID)
        let changedVolumes = originalVolumes.filter {
            writeVolume(
                Self.attenuatedVolume(original: $0.value, percent: requestedPercent),
                deviceID: deviceID,
                element: $0.element
            )
        }
        guard !changedVolumes.isEmpty, changedVolumes.count == originalVolumes.count else {
            // Same all-or-nothing rule for channel volumes: partial ducking is
            // more surprising than leaving an unsupported device untouched.
            for volume in changedVolumes {
                _ = writeVolume(volume.value, deviceID: deviceID, element: volume.element)
            }
            return
        }
        snapshot = DeviceSnapshot(deviceID: deviceID, volumes: changedVolumes, mutes: [])
    }

    private func restoreCurrentDevice() {
        guard let snapshot else { return }
        for volume in snapshot.volumes {
            _ = writeVolume(volume.value, deviceID: snapshot.deviceID, element: volume.element)
        }
        for mute in snapshot.mutes {
            _ = writeMute(mute.value, deviceID: snapshot.deviceID, element: mute.element)
        }
        self.snapshot = nil
    }

    private func currentOutputDevice() -> AudioDeviceID? {
        var deviceID = AudioDeviceID(kAudioObjectUnknown)
        var dataSize = UInt32(MemoryLayout<AudioDeviceID>.size)
        var address = defaultOutputAddress
        let status = AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            0,
            nil,
            &dataSize,
            &deviceID
        )
        guard status == noErr, deviceID != kAudioObjectUnknown else { return nil }
        return deviceID
    }

    private func readVolumes(deviceID: AudioDeviceID) -> [ScalarSnapshot] {
        settableElements(deviceID: deviceID, selector: kAudioDevicePropertyVolumeScalar)
            .compactMap { element in
                var value: Float32 = 0
                var dataSize = UInt32(MemoryLayout<Float32>.size)
                var address = outputAddress(
                    selector: kAudioDevicePropertyVolumeScalar,
                    element: element
                )
                guard AudioObjectGetPropertyData(
                    deviceID,
                    &address,
                    0,
                    nil,
                    &dataSize,
                    &value
                ) == noErr else {
                    return nil
                }
                return ScalarSnapshot(element: element, value: value)
            }
    }

    private func readMutes(deviceID: AudioDeviceID) -> [MuteSnapshot] {
        settableElements(deviceID: deviceID, selector: kAudioDevicePropertyMute)
            .compactMap { element in
                var value: UInt32 = 0
                var dataSize = UInt32(MemoryLayout<UInt32>.size)
                var address = outputAddress(selector: kAudioDevicePropertyMute, element: element)
                guard AudioObjectGetPropertyData(
                    deviceID,
                    &address,
                    0,
                    nil,
                    &dataSize,
                    &value
                ) == noErr else {
                    return nil
                }
                return MuteSnapshot(element: element, value: value)
            }
    }

    private func settableElements(
        deviceID: AudioDeviceID,
        selector: AudioObjectPropertySelector
    ) -> [AudioObjectPropertyElement] {
        if isSettable(deviceID: deviceID, selector: selector, element: kAudioObjectPropertyElementMain) {
            return [kAudioObjectPropertyElementMain]
        }

        // Standard macOS output devices expose channel elements from 1 upward.
        // Querying absent elements is harmless and avoids assumptions about the
        // device's stream layout (stereo, HDMI, aggregate, and virtual devices).
        return (1 ... 32).compactMap { channel in
            let element = AudioObjectPropertyElement(channel)
            return isSettable(deviceID: deviceID, selector: selector, element: element)
                ? element
                : nil
        }
    }

    private func isSettable(
        deviceID: AudioDeviceID,
        selector: AudioObjectPropertySelector,
        element: AudioObjectPropertyElement
    ) -> Bool {
        var address = outputAddress(selector: selector, element: element)
        guard AudioObjectHasProperty(deviceID, &address) else { return false }
        var settable: DarwinBoolean = false
        return AudioObjectIsPropertySettable(deviceID, &address, &settable) == noErr
            && settable.boolValue
    }

    private func writeVolume(
        _ value: Float32,
        deviceID: AudioDeviceID,
        element: AudioObjectPropertyElement
    ) -> Bool {
        var value = value
        var address = outputAddress(
            selector: kAudioDevicePropertyVolumeScalar,
            element: element
        )
        return AudioObjectSetPropertyData(
            deviceID,
            &address,
            0,
            nil,
            UInt32(MemoryLayout<Float32>.size),
            &value
        ) == noErr
    }

    private func writeMute(
        _ value: UInt32,
        deviceID: AudioDeviceID,
        element: AudioObjectPropertyElement
    ) -> Bool {
        var value = value
        var address = outputAddress(selector: kAudioDevicePropertyMute, element: element)
        return AudioObjectSetPropertyData(
            deviceID,
            &address,
            0,
            nil,
            UInt32(MemoryLayout<UInt32>.size),
            &value
        ) == noErr
    }

    private func outputAddress(
        selector: AudioObjectPropertySelector,
        element: AudioObjectPropertyElement
    ) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: element
        )
    }
}
