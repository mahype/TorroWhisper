import AppKit
import SwiftUI

struct HotkeyRecorderField: View {
    let title: String
    let currentHotkey: String
    let isCapturing: Bool
    let preview: String
    let errorText: String?
    let warningText: String?
    let warningDetails: String?
    let onStartCapture: () -> Void
    let onCommit: (String) -> Void
    let onCancel: () -> Void
    let onClear: () -> Void
    let onPreview: (String) -> Void
    let onInvalid: (String) -> Void
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(Color(nsColor: .textBackgroundColor))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(isCapturing ? Color.accentColor : Color.secondary.opacity(0.16), lineWidth: isCapturing ? 1.5 : 1)
                        )

                    HStack(spacing: 8) {
                        Image(systemName: isCapturing ? "keyboard.badge.ellipsis" : "command")
                            .foregroundStyle(isCapturing ? Color.accentColor : Color.secondary)
                        Text(displayText)
                            .font(.system(.body, design: .rounded).weight(.medium))
                            .foregroundStyle(displayText == placeholderText ? .secondary : .primary)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)

                    if isCapturing {
                        HotkeyCaptureHost(
                            onPreview: onPreview,
                            onCommit: onCommit,
                            onCancel: onCancel,
                            onClear: onClear,
                            onInvalid: onInvalid
                        )
                        .frame(width: 0, height: 0)
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 30)

                if isCapturing {
                    Button(action: onCancel) {
                        Text("Cancel", bundle: .module)
                    }
                    Button(action: onClear) {
                        Text("Clear", bundle: .module)
                    }
                } else {
                    Button(action: onStartCapture) {
                        Text("Record", bundle: .module)
                    }
                    .buttonStyle(.borderedProminent)
                }
            }

            if let errorText, !errorText.isEmpty {
                Text(errorText)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .accessibilityLabel("\(L("Error", locale: locale)): \(errorText)")
            } else if let warningText, !warningText.isEmpty {
                HStack(alignment: .top, spacing: 4) {
                    Text(warningText)
                        .font(.caption)
                        .foregroundStyle(.orange)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityLabel("\(L("Warning", locale: locale)): \(warningText)")
                    if let warningDetails, !warningDetails.isEmpty {
                        Image(systemName: "info.circle")
                            .font(.caption)
                            .foregroundStyle(.orange)
                            .help(Text(warningDetails))
                            .accessibilityLabel(Text(warningDetails))
                    }
                    Spacer(minLength: 0)
                }
            }
        }
        .onChange(of: errorText) { newValue in
            guard let newValue, !newValue.isEmpty else { return }
            NSAccessibility.post(
                element: NSApp as Any,
                notification: .announcementRequested,
                userInfo: [
                    .announcement: "\(L("Error", locale: locale)): \(newValue)",
                    .priority: NSAccessibilityPriorityLevel.high.rawValue,
                ]
            )
        }
    }

    private var displayText: String {
        if isCapturing {
            if !preview.isEmpty {
                return hotkeyDisplayString(preview)
            }
            return placeholderText
        }

        if currentHotkey.isEmpty {
            return placeholderText
        }

        return hotkeyDisplayString(currentHotkey)
    }

    private var placeholderText: String {
        isCapturing
            ? L("Press your keyboard shortcut now", locale: locale)
            : L("No hotkey set", locale: locale)
    }
}

private struct HotkeyCaptureHost: NSViewRepresentable {
    let onPreview: (String) -> Void
    let onCommit: (String) -> Void
    let onCancel: () -> Void
    let onClear: () -> Void
    let onInvalid: (String) -> Void

    func makeNSView(context: Context) -> HotkeyCaptureView {
        let view = HotkeyCaptureView()
        view.onPreview = onPreview
        view.onCommit = onCommit
        view.onCancel = onCancel
        view.onClear = onClear
        view.onInvalid = onInvalid
        DispatchQueue.main.async {
            view.window?.makeFirstResponder(view)
        }
        return view
    }

    func updateNSView(_ nsView: HotkeyCaptureView, context: Context) {
        nsView.onPreview = onPreview
        nsView.onCommit = onCommit
        nsView.onCancel = onCancel
        nsView.onClear = onClear
        nsView.onInvalid = onInvalid
        DispatchQueue.main.async {
            if nsView.window?.firstResponder !== nsView {
                nsView.window?.makeFirstResponder(nsView)
            }
        }
    }
}

private final class HotkeyCaptureView: NSView {
    var onPreview: ((String) -> Void)?
    var onCommit: ((String) -> Void)?
    var onCancel: (() -> Void)?
    var onClear: (() -> Void)?
    var onInvalid: ((String) -> Void)?

    override var acceptsFirstResponder: Bool { true }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }

    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)

        if event.keyCode == 53 {
            onCancel?()
            return
        }

        if modifiers.intersection(hotkeyRelevantModifierMask).isEmpty
            && (event.keyCode == 51 || event.keyCode == 117)
        {
            onClear?()
            return
        }

        let modifierTokens = hotkeyModifierTokens(from: modifiers)
        guard let keyToken = hotkeyKeyToken(for: event) else {
            onInvalid?(unsupportedHotkeyMessage(locale: .current))
            return
        }

        let hotkey = (modifierTokens + [keyToken]).joined(separator: "+")
        onPreview?(hotkey)
        onCommit?(hotkey)
    }

    override func flagsChanged(with event: NSEvent) {
        let modifiers = hotkeyModifierTokens(from: event.modifierFlags.intersection(.deviceIndependentFlagsMask))
        if modifiers.isEmpty {
            onPreview?("")
        } else {
            onPreview?(modifiers.joined(separator: "+"))
        }
    }
}

struct HotkeyNamedKeySpec {
    let token: String
    let keyCodes: [UInt16]
    let aliases: [String]
}

let hotkeyRelevantModifierMask: NSEvent.ModifierFlags = [.command, .control, .option, .shift]

func unsupportedHotkeyMessage(locale: Locale) -> String {
    L("This key is not currently supported as a global hotkey in the native macOS app.", locale: locale)
}

let hotkeyNamedKeySpecs: [HotkeyNamedKeySpec] = [
    HotkeyNamedKeySpec(token: "Enter", keyCodes: [36, 76], aliases: ["Return"]),
    HotkeyNamedKeySpec(token: "Tab", keyCodes: [48], aliases: []),
    HotkeyNamedKeySpec(token: "Space", keyCodes: [49], aliases: []),
    HotkeyNamedKeySpec(token: "Backspace", keyCodes: [51], aliases: []),
    HotkeyNamedKeySpec(token: "Escape", keyCodes: [53], aliases: ["Esc"]),
    HotkeyNamedKeySpec(token: "Delete", keyCodes: [117], aliases: ["ForwardDelete"]),
    HotkeyNamedKeySpec(token: "Insert", keyCodes: [114], aliases: []),
    HotkeyNamedKeySpec(token: "Home", keyCodes: [115], aliases: []),
    HotkeyNamedKeySpec(token: "End", keyCodes: [119], aliases: []),
    HotkeyNamedKeySpec(token: "PageUp", keyCodes: [116], aliases: []),
    HotkeyNamedKeySpec(token: "PageDown", keyCodes: [121], aliases: []),
    HotkeyNamedKeySpec(token: "Left", keyCodes: [123], aliases: ["ArrowLeft"]),
    HotkeyNamedKeySpec(token: "Right", keyCodes: [124], aliases: ["ArrowRight"]),
    HotkeyNamedKeySpec(token: "Down", keyCodes: [125], aliases: ["ArrowDown"]),
    HotkeyNamedKeySpec(token: "Up", keyCodes: [126], aliases: ["ArrowUp"]),
    HotkeyNamedKeySpec(token: "F1", keyCodes: [122], aliases: []),
    HotkeyNamedKeySpec(token: "F2", keyCodes: [120], aliases: []),
    HotkeyNamedKeySpec(token: "F3", keyCodes: [99], aliases: []),
    HotkeyNamedKeySpec(token: "F4", keyCodes: [118], aliases: []),
    HotkeyNamedKeySpec(token: "F5", keyCodes: [96], aliases: []),
    HotkeyNamedKeySpec(token: "F6", keyCodes: [97], aliases: []),
    HotkeyNamedKeySpec(token: "F7", keyCodes: [98], aliases: []),
    HotkeyNamedKeySpec(token: "F8", keyCodes: [100], aliases: []),
    HotkeyNamedKeySpec(token: "F9", keyCodes: [101], aliases: []),
    HotkeyNamedKeySpec(token: "F10", keyCodes: [109], aliases: []),
    HotkeyNamedKeySpec(token: "F11", keyCodes: [103], aliases: []),
    HotkeyNamedKeySpec(token: "F12", keyCodes: [111], aliases: []),
    HotkeyNamedKeySpec(token: "F13", keyCodes: [105], aliases: []),
    HotkeyNamedKeySpec(token: "F14", keyCodes: [107], aliases: []),
    HotkeyNamedKeySpec(token: "F15", keyCodes: [113], aliases: []),
    HotkeyNamedKeySpec(token: "F16", keyCodes: [106], aliases: []),
    HotkeyNamedKeySpec(token: "F17", keyCodes: [64], aliases: []),
    HotkeyNamedKeySpec(token: "F18", keyCodes: [79], aliases: []),
    HotkeyNamedKeySpec(token: "F19", keyCodes: [80], aliases: []),
    HotkeyNamedKeySpec(token: "F20", keyCodes: [90], aliases: []),
    HotkeyNamedKeySpec(token: "Numpad0", keyCodes: [82], aliases: ["Num0"]),
    HotkeyNamedKeySpec(token: "Numpad1", keyCodes: [83], aliases: ["Num1"]),
    HotkeyNamedKeySpec(token: "Numpad2", keyCodes: [84], aliases: ["Num2"]),
    HotkeyNamedKeySpec(token: "Numpad3", keyCodes: [85], aliases: ["Num3"]),
    HotkeyNamedKeySpec(token: "Numpad4", keyCodes: [86], aliases: ["Num4"]),
    HotkeyNamedKeySpec(token: "Numpad5", keyCodes: [87], aliases: ["Num5"]),
    HotkeyNamedKeySpec(token: "Numpad6", keyCodes: [88], aliases: ["Num6"]),
    HotkeyNamedKeySpec(token: "Numpad7", keyCodes: [89], aliases: ["Num7"]),
    HotkeyNamedKeySpec(token: "Numpad8", keyCodes: [91], aliases: ["Num8"]),
    HotkeyNamedKeySpec(token: "Numpad9", keyCodes: [92], aliases: ["Num9"]),
    HotkeyNamedKeySpec(token: "NumpadEnter", keyCodes: [76], aliases: ["NumEnter"]),
    HotkeyNamedKeySpec(token: "NumpadAdd", keyCodes: [69], aliases: ["NumAdd", "NumpadPlus"]),
    HotkeyNamedKeySpec(token: "NumpadSubtract", keyCodes: [78], aliases: ["NumSubtract", "NumpadMinus"]),
    HotkeyNamedKeySpec(token: "NumpadMultiply", keyCodes: [67], aliases: ["NumMultiply"]),
    HotkeyNamedKeySpec(token: "NumpadDivide", keyCodes: [75], aliases: ["NumDivide"]),
    HotkeyNamedKeySpec(token: "NumpadDecimal", keyCodes: [65], aliases: ["NumDecimal"]),
    HotkeyNamedKeySpec(token: "NumpadEqual", keyCodes: [81], aliases: ["NumEqual"]),
    HotkeyNamedKeySpec(token: "AudioVolumeUp", keyCodes: [72], aliases: ["VolumeUp"]),
    HotkeyNamedKeySpec(token: "AudioVolumeDown", keyCodes: [73], aliases: ["VolumeDown"]),
    HotkeyNamedKeySpec(token: "AudioVolumeMute", keyCodes: [74], aliases: ["VolumeMute"]),
]

let hotkeyNamedTokenByKeyCode: [UInt16: String] = {
    var map: [UInt16: String] = [:]
    for spec in hotkeyNamedKeySpecs {
        for keyCode in spec.keyCodes {
            map[keyCode] = spec.token
        }
    }
    return map
}()

let hotkeyNamedKeyCodeByNormalizedToken: [String: UInt16] = {
    var map: [String: UInt16] = [:]
    for spec in hotkeyNamedKeySpecs {
        guard let keyCode = spec.keyCodes.first else {
            continue
        }
        for token in [spec.token] + spec.aliases {
            map[hotkeyNormalizedToken(token)] = keyCode
        }
    }
    return map
}()

private func hotkeyModifierTokens(from flags: NSEvent.ModifierFlags) -> [String] {
    let relevant = flags.intersection(hotkeyRelevantModifierMask)
    var tokens: [String] = []

    if relevant.contains(.command) {
        tokens.append("Cmd")
    }
    if relevant.contains(.control) {
        tokens.append("Ctrl")
    }
    if relevant.contains(.option) {
        tokens.append("Option")
    }
    if relevant.contains(.shift) {
        tokens.append("Shift")
    }

    return tokens
}

private func hotkeyKeyToken(for event: NSEvent) -> String? {
    if let namedToken = hotkeyNamedTokenByKeyCode[event.keyCode] {
        return namedToken
    }

    guard let character = event.charactersIgnoringModifiers?.trimmingCharacters(in: .whitespacesAndNewlines), character.count == 1 else {
        return nil
    }

    let scalar = character.unicodeScalars.first!
    if CharacterSet.letters.contains(scalar) {
        return character.uppercased()
    }
    if CharacterSet.decimalDigits.contains(scalar) {
        return character
    }

    switch character {
    case "-":
        return "-"
    case "=":
        return "="
    case "[":
        return "["
    case "]":
        return "]"
    case "\\":
        return "\\"
    case ";":
        return ";"
    case "'":
        return "'"
    case ",":
        return ","
    case ".":
        return "."
    case "/":
        return "/"
    case "`":
        return "`"
    default:
        return nil
    }
}

func hotkeyNormalizedToken(_ token: String) -> String {
    token
        .trimmingCharacters(in: .whitespacesAndNewlines)
        .lowercased()
}

func hotkeyDisplayString(_ hotkey: String) -> String {
    hotkey
        .split(separator: "+")
        .map { token in
            switch token.lowercased() {
            case "cmd", "command", "super":
                return "⌘"
            case "ctrl", "control":
                return "⌃"
            case "option", "alt":
                return "⌥"
            case "shift":
                return "⇧"
            case "space":
                return "Space"
            case "enter":
                return "Enter"
            case "backspace":
                return "Backspace"
            case "escape", "esc":
                return "Esc"
            default:
                return String(token)
            }
        }
        .joined(separator: " ")
}
