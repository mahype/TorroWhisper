import AppKit
import Carbon

@MainActor
enum HotkeyAssignmentAdvisor {
    static func assertCanAssign(_ hotkey: String, allowNoOpHotkeys: [String] = []) throws {
        let trimmedHotkey = hotkey.trimmingCharacters(in: .whitespacesAndNewlines)
        if allowNoOpHotkeys.contains(where: { hotkeySignature($0) == hotkeySignature(trimmedHotkey) }) {
            return
        }

        let assignment = try parseHotkey(trimmedHotkey)

        if isReservedBySystem(assignment) {
            throw HotkeyAssignmentAdvisorError(
                message: L("This shortcut is already reserved by macOS. Change it in System Settings under Keyboard › Keyboard Shortcuts, or pick a different combination.", locale: .current)
            )
        }

        try assertTemporaryRegistration(assignment)
    }

    private static func parseHotkey(_ hotkey: String) throws -> HotkeyCandidate {
        let tokens = hotkey
            .split(separator: "+")
            .map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }

        guard let keyToken = tokens.last else {
            throw HotkeyAssignmentAdvisorError(message: L("Hotkey must not be empty.", locale: .current))
        }

        var modifiers: NSEvent.ModifierFlags = []
        for token in tokens.dropLast() {
            guard let modifier = modifierFlag(forToken: token) else {
                throw HotkeyAssignmentAdvisorError(message: unsupportedHotkeyMessage(locale: .current))
            }
            modifiers.formUnion(modifier)
        }

        guard let keyCode = keyCode(forToken: keyToken) else {
            throw HotkeyAssignmentAdvisorError(message: unsupportedHotkeyMessage(locale: .current))
        }

        return HotkeyCandidate(
            keyCode: keyCode,
            modifiers: modifiers.intersection(hotkeyRelevantModifierMask)
        )
    }

    private static func modifierFlag(forToken token: String) -> NSEvent.ModifierFlags? {
        switch hotkeyNormalizedToken(token) {
        case "cmd", "command", "super", "commandorcontrol", "commandorctrl", "cmdorctrl", "cmdorcontrol":
            return .command
        case "ctrl", "control":
            return .control
        case "option", "alt":
            return .option
        case "shift":
            return .shift
        default:
            return nil
        }
    }

    private static func keyCode(forToken token: String) -> UInt32? {
        let normalizedToken = hotkeyNormalizedToken(token)

        if let keyCode = hotkeyNamedKeyCodeByNormalizedToken[normalizedToken] {
            return UInt32(keyCode)
        }
        if let keyCode = hotkeyStandardKeyCodeByNormalizedToken[normalizedToken] {
            return keyCode
        }
        if let keyCode = hotkeyExtendedKeyCodeByNormalizedToken[normalizedToken] {
            return keyCode
        }

        return nil
    }

    private static func isReservedBySystem(_ candidate: HotkeyCandidate) -> Bool {
        var symbolicHotKeys: Unmanaged<CFArray>?
        guard CopySymbolicHotKeys(&symbolicHotKeys) == noErr, let symbolicHotKeys else {
            return false
        }

        let entries = symbolicHotKeys.takeRetainedValue() as NSArray
        for case let entry as NSDictionary in entries {
            guard let isEnabled = entry[kHISymbolicHotKeyEnabled as String] as? Bool, isEnabled,
                  let keyCode = entry[kHISymbolicHotKeyCode as String] as? NSNumber,
                  let modifiers = entry[kHISymbolicHotKeyModifiers as String] as? NSNumber else {
                continue
            }

            if keyCode.uint32Value == candidate.keyCode && modifiers.uint64Value == candidate.modifiers.rawValue {
                return true
            }
        }

        return false
    }

    private static func assertTemporaryRegistration(_ candidate: HotkeyCandidate) throws {
        var hotKeyRef: EventHotKeyRef?
        let hotKeyID = EventHotKeyID(signature: tempHotKeySignature, id: 1)
        let status = RegisterEventHotKey(
            candidate.keyCode,
            UInt32(candidate.modifiers.rawValue),
            hotKeyID,
            GetApplicationEventTarget(),
            0,
            &hotKeyRef
        )

        guard status == noErr else {
            throw HotkeyAssignmentAdvisorError(
                message: registrationErrorMessage(for: status, modifiers: candidate.modifiers)
            )
        }

        if let hotKeyRef {
            let unregisterStatus = UnregisterEventHotKey(hotKeyRef)
            guard unregisterStatus == noErr else {
                throw HotkeyAssignmentAdvisorError(
                    message: String(
                        format: L("The shortcut could be tested but couldn't be released cleanly afterwards (OSStatus %d). Please try again.", locale: .current),
                        Int(unregisterStatus)
                    )
                )
            }
        }
    }

    private static func registrationErrorMessage(
        for status: OSStatus,
        modifiers: NSEvent.ModifierFlags
    ) -> String {
        if status == eventHotKeyExistsErr {
            return L("This shortcut is already used by Donny. If it is currently active, leave it unchanged or pick a new combination.", locale: .current)
        }

        if status == eventInternalErr {
            if modifiers == [.option] || modifiers == [.option, .shift] {
                return L("macOS currently rejects this option-only combination internally. Add Ctrl or Cmd, or choose a different shortcut.", locale: .current)
            }

            return L("macOS rejected the registration of this shortcut internally. This usually affects reserved or system-blocked shortcuts.", locale: .current)
        }

        return String(
            format: L("macOS couldn't register this shortcut right now (OSStatus %d). It is probably reserved or already exclusively in use.", locale: .current),
            Int(status)
        )
    }

    private static func hotkeySignature(_ hotkey: String) -> String {
        let tokens = hotkey
            .split(separator: "+")
            .map { hotkeyNormalizedToken(String($0)) }
            .filter { !$0.isEmpty }

        guard let keyToken = tokens.last else {
            return hotkeyNormalizedToken(hotkey)
        }

        var modifiers: NSEvent.ModifierFlags = []
        for token in tokens.dropLast() {
            if let modifier = modifierFlag(forToken: token) {
                modifiers.formUnion(modifier)
            }
        }

        var canonicalTokens: [String] = []
        if modifiers.contains(.command) {
            canonicalTokens.append("cmd")
        }
        if modifiers.contains(.control) {
            canonicalTokens.append("ctrl")
        }
        if modifiers.contains(.option) {
            canonicalTokens.append("option")
        }
        if modifiers.contains(.shift) {
            canonicalTokens.append("shift")
        }

        if let keyCode = keyCode(forToken: keyToken) {
            canonicalTokens.append("keycode:\(keyCode)")
        } else {
            canonicalTokens.append(keyToken)
        }

        return canonicalTokens.joined(separator: "+")
    }
}

private struct HotkeyCandidate {
    let keyCode: UInt32
    let modifiers: NSEvent.ModifierFlags
}

private struct HotkeyAssignmentAdvisorError: LocalizedError {
    let message: String

    var errorDescription: String? { message }
}

private let tempHotKeySignature: OSType = 0x6F77686B // "owhk"

private let hotkeyStandardKeyCodeByNormalizedToken: [String: UInt32] = {
    var map: [String: UInt32] = [
        "a": 0x00,
        "s": 0x01,
        "d": 0x02,
        "f": 0x03,
        "h": 0x04,
        "g": 0x05,
        "z": 0x06,
        "x": 0x07,
        "c": 0x08,
        "v": 0x09,
        "b": 0x0B,
        "q": 0x0C,
        "w": 0x0D,
        "e": 0x0E,
        "r": 0x0F,
        "y": 0x10,
        "t": 0x11,
        "1": 0x12,
        "2": 0x13,
        "3": 0x14,
        "4": 0x15,
        "6": 0x16,
        "5": 0x17,
        "=": 0x18,
        "9": 0x19,
        "7": 0x1A,
        "-": 0x1B,
        "8": 0x1C,
        "0": 0x1D,
        "]": 0x1E,
        "o": 0x1F,
        "u": 0x20,
        "[": 0x21,
        "i": 0x22,
        "p": 0x23,
        "l": 0x25,
        "j": 0x26,
        "'": 0x27,
        "k": 0x28,
        ";": 0x29,
        "\\": 0x2A,
        ",": 0x2B,
        "/": 0x2C,
        "n": 0x2D,
        "m": 0x2E,
        ".": 0x2F,
        "`": 0x32,
    ]

    for token in "abcdefghijklmnopqrstuvwxyz".map(String.init) {
        if let keyCode = map[token] {
            map["key\(token)"] = keyCode
        }
    }

    for digit in 0...9 {
        if let keyCode = map["\(digit)"] {
            map["digit\(digit)"] = keyCode
        }
    }

    let aliases: [String: String] = [
        "backquote": "`",
        "backslash": "\\",
        "bracketleft": "[",
        "bracketright": "]",
        "comma": ",",
        "equal": "=",
        "minus": "-",
        "period": ".",
        "quote": "'",
        "semicolon": ";",
        "slash": "/",
    ]
    for (alias, token) in aliases {
        if let keyCode = map[token] {
            map[alias] = keyCode
        }
    }

    return map
}()

private let hotkeyExtendedKeyCodeByNormalizedToken: [String: UInt32] = [
    "capslock": 0x39,
    "numpaddecimal": 0x41,
    "numdecimal": 0x41,
    "numpadmultiply": 0x43,
    "nummultiply": 0x43,
    "numpadadd": 0x45,
    "numadd": 0x45,
    "numpadplus": 0x45,
    "numplus": 0x45,
    "numlock": 0x47,
    "audiovolumeup": 0x48,
    "volumeup": 0x48,
    "audiovolumedown": 0x49,
    "volumedown": 0x49,
    "audiovolumemute": 0x4A,
    "volumemute": 0x4A,
    "numpaddivide": 0x4B,
    "numdivide": 0x4B,
    "numpadenter": 0x4C,
    "numenter": 0x4C,
    "numpadsubtract": 0x4E,
    "numsubtract": 0x4E,
    "f18": 0x4F,
    "f19": 0x50,
    "numpadequal": 0x51,
    "numequal": 0x51,
    "numpad0": 0x52,
    "num0": 0x52,
    "numpad1": 0x53,
    "num1": 0x53,
    "numpad2": 0x54,
    "num2": 0x54,
    "numpad3": 0x55,
    "num3": 0x55,
    "numpad4": 0x56,
    "num4": 0x56,
    "numpad5": 0x57,
    "num5": 0x57,
    "numpad6": 0x58,
    "num6": 0x58,
    "numpad7": 0x59,
    "num7": 0x59,
    "f20": 0x5A,
    "numpad8": 0x5B,
    "num8": 0x5B,
    "numpad9": 0x5C,
    "num9": 0x5C,
    "f13": 0x69,
    "f16": 0x6A,
    "f14": 0x6B,
    "f15": 0x71,
    "f17": 0x40,
    "printscreen": 0x46,
]
