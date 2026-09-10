import AppKit
import SwiftUI

enum SettingsSection: String, CaseIterable, Identifiable {
    case overview
    case recording
    case modes
    case dictionary
    case history
    case languageModels = "language_models"
    case startup
    case updates
    case diagnostics
    case help

    var id: String { rawValue }

    func title(locale: Locale) -> String {
        switch self {
        case .overview:
            return L("Overview", locale: locale)
        case .recording:
            return L("Recording", locale: locale)
        case .modes:
            return L("Post-processing", locale: locale)
        case .dictionary:
            return L("Dictionary", locale: locale)
        case .history:
            return L("History", locale: locale)
        case .languageModels:
            return L("Language models", locale: locale)
        case .startup:
            return L("Start & behavior", locale: locale)
        case .updates:
            return L("Updates", locale: locale)
        case .diagnostics:
            return L("Diagnostics", locale: locale)
        case .help:
            return L("Help", locale: locale)
        }
    }

    var symbolName: String {
        switch self {
        case .overview:
            return "square.grid.2x2.fill"
        case .recording:
            return "mic.fill"
        case .modes:
            return "square.text.square"
        case .dictionary:
            return "character.book.closed.fill"
        case .history:
            return "clock.arrow.circlepath"
        case .languageModels:
            return "brain.head.profile"
        case .startup:
            return "power.circle.fill"
        case .updates:
            return "arrow.triangle.2.circlepath"
        case .diagnostics:
            return "checklist"
        case .help:
            return "questionmark.circle"
        }
    }
}

struct ModeListTile: View {
    let mode: ProcessingMode
    let summary: String
    let isActive: Bool
    let onActivate: () -> Void
    let onEdit: () -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Button(action: onActivate) {
                HStack(spacing: 10) {
                    Image(systemName: isActive ? "largecircle.fill.circle" : "circle")
                        .font(.body)
                        .foregroundStyle(isActive ? Color.torroAccent : Color.secondary.opacity(0.7))
                        .accessibilityHidden(true)

                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            Text(mode.name)
                                .font(.body.weight(.medium))
                                .foregroundStyle(.primary)
                                .lineLimit(1)
                            if isActive {
                                Text("Active", bundle: .module)
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                        }
                        Text(summary)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .help(summary)
                    }

                    Spacer(minLength: 8)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(isActive ? [.isSelected] : [])

            Button(action: onEdit) {
                Text("Edit", bundle: .module)
            }
            .buttonStyle(.bordered)
            .help(Text("Edit post-processing", bundle: .module))
            .accessibilityLabel(Text("Edit post-processing", bundle: .module))

            Button(action: onDelete) {
                Image(systemName: "trash")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
            .help(Text("Delete post-processing", bundle: .module))
            .accessibilityLabel(Text("Delete post-processing", bundle: .module))
        }
        .onHover { hovering in
            if hovering {
                NSCursor.pointingHand.push()
            } else {
                NSCursor.pop()
            }
        }
    }
}

struct PostProcessingOffTile: View {
    let isActive: Bool
    let onActivate: () -> Void

    var body: some View {
        Button(action: onActivate) {
            HStack(spacing: 10) {
                Image(systemName: isActive ? "largecircle.fill.circle" : "circle")
                    .font(.body)
                    .foregroundStyle(isActive ? Color.torroAccent : Color.secondary.opacity(0.7))
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: 2) {
                    Text("Off", bundle: .module)
                        .font(.body.weight(.medium))
                        .foregroundStyle(.primary)
                    Text("Transcription is used as-is.", bundle: .module)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                Spacer(minLength: 8)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isActive ? [.isSelected] : [])
        .onHover { hovering in
            if hovering {
                NSCursor.pointingHand.push()
            } else {
                NSCursor.pop()
            }
        }
    }
}

struct ModelPresetTile: View {
    let preset: ModelPreset
    let isSelected: Bool
    let action: () -> Void
    @Environment(\.locale) private var locale

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(isSelected ? Color.torroAccent : Color.secondary.opacity(0.7))
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: 2) {
                    Text(preset.displayName)
                        .font(.body.weight(.medium))
                        .foregroundStyle(.primary)
                    Text(preset.description(locale: locale))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                Spacer(minLength: 8)

                Text("\(L("approx.", locale: locale)) \(preset.downloadSizeText)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

struct DictionaryEntryRow: View {
    @Binding var pattern: String
    @Binding var replacement: String
    @Binding var caseSensitive: Bool
    @Binding var wholeWord: Bool
    let onDelete: () -> Void
    @Environment(\.locale) private var locale

    init(
        patternBinding: Binding<String>,
        replacementBinding: Binding<String>,
        caseSensitiveBinding: Binding<Bool>,
        wholeWordBinding: Binding<Bool>,
        onDelete: @escaping () -> Void
    ) {
        self._pattern = patternBinding
        self._replacement = replacementBinding
        self._caseSensitive = caseSensitiveBinding
        self._wholeWord = wholeWordBinding
        self.onDelete = onDelete
    }

    var body: some View {
        HStack(spacing: 8) {
            TextField(L("Heard", locale: locale), text: $pattern)
                .textFieldStyle(.roundedBorder)
                .frame(maxWidth: .infinity)
                .accessibilityLabel(Text("Heard", bundle: .module))

            Image(systemName: "arrow.right")
                .font(.caption)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)

            TextField(L("Replacement", locale: locale), text: $replacement)
                .textFieldStyle(.roundedBorder)
                .frame(maxWidth: .infinity)
                .accessibilityLabel(Text("Replacement", bundle: .module))

            Toggle(isOn: $caseSensitive) {
                Text("Aa")
                    .font(.caption.weight(.semibold))
                    .monospaced()
            }
            .toggleStyle(.button)
            .controlSize(.small)
            .help(Text("Match case", bundle: .module))
            .accessibilityLabel(Text("Match case", bundle: .module))

            Toggle(isOn: $wholeWord) {
                Text("W")
                    .font(.caption.weight(.semibold))
                    .monospaced()
            }
            .toggleStyle(.button)
            .controlSize(.small)
            .help(Text("Whole word only", bundle: .module))
            .accessibilityLabel(Text("Whole word only", bundle: .module))

            Button(role: .destructive, action: onDelete) {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
            .help(Text("Delete entry", bundle: .module))
            .accessibilityLabel(Text("Delete entry", bundle: .module))
        }
        .padding(.vertical, 2)
    }
}

struct HistoryEntryRow: View {
    let entry: HistoryEntry
    let onDelete: () -> Void
    let onCopy: () -> Void
    @Environment(\.locale) private var locale

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(formattedTimestamp)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.secondary)
                    if !entry.modeName.isEmpty {
                        Text("·")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(entry.modeName)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    if entry.wasCancelled {
                        TorroChip(text: L("Cancelled", locale: locale), kind: .exception)
                    }
                    Spacer(minLength: 0)
                }
                Text(entry.text)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .lineLimit(4)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel(accessibilityText)

            VStack(spacing: 6) {
                Button(action: onCopy) {
                    Image(systemName: "doc.on.doc")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.borderless)
                .help(Text("Copy to clipboard", bundle: .module))
                .accessibilityLabel(Text("Copy to clipboard", bundle: .module))

                Button(role: .destructive, action: onDelete) {
                    Image(systemName: "trash")
                }
                .buttonStyle(.borderless)
                .help(Text("Delete entry", bundle: .module))
                .accessibilityLabel(Text("Delete entry", bundle: .module))
            }
        }
        .padding(.vertical, 4)
    }

    private var accessibilityText: String {
        var parts: [String] = [formattedTimestamp]
        if !entry.modeName.isEmpty {
            parts.append(entry.modeName)
        }
        if entry.wasCancelled {
            parts.append(L("Cancelled", locale: locale))
        }
        parts.append(entry.text)
        return parts.joined(separator: ", ")
    }

    private var formattedTimestamp: String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.dateStyle = .short
        formatter.timeStyle = .short
        return formatter.string(from: entry.date)
    }
}

struct DiagnosticStatusBadge: View {
    let status: DiagnosticStatus
    @Environment(\.locale) private var locale

    var body: some View {
        TorroStatusChip(text: status.label(locale: locale), color: backgroundColor)
    }

    private var backgroundColor: Color {
        switch status {
        case .ok:
            return .green
        case .info:
            return .secondary
        case .warning:
            return .orange
        case .error:
            return .red
        }
    }
}

struct DiagnosticStatusRow: View {
    let item: DiagnosticItemDTO
    /// Invoked when the user taps the item's remedy button. Nil when the caller
    /// cannot act on fixes.
    var onFix: ((DiagnosticFix) -> Void)?
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(L(item.title, locale: locale))
                    .font(.body.weight(.medium))
                    .fixedSize()

                if !needsAction {
                    Text(L(item.problem, locale: locale))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }

                Spacer(minLength: 8)

                DiagnosticStatusBadge(status: item.status)
                    .fixedSize()
            }

            if needsAction {
                actionDetails
            }
        }
    }

    /// Healthy and informational checks stay on one line. A warning or error is
    /// exceptional and therefore gets its explanation without hiding it behind
    /// a small disclosure target.
    private var needsAction: Bool {
        item.status == .warning || item.status == .error
    }

    private var actionDetails: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(L(item.problem, locale: locale))
                .font(.caption)
                .fixedSize(horizontal: false, vertical: true)
                .multilineTextAlignment(.leading)
            Text(L(item.recommendation, locale: locale))
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .multilineTextAlignment(.leading)

            if let fix = item.fix, let onFix {
                Button {
                    onFix(fix)
                } label: {
                    Text(L(fix.buttonKey, locale: locale))
                }
                .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct StepRail: View {
    let currentStep: Int
    @Environment(\.locale) private var locale

    private var steps: [String] {
        [
            L("Recording", locale: locale),
            L("Transcription", locale: locale),
            L("Start & behavior", locale: locale),
            L("Ready", locale: locale),
        ]
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            // Brand header — the wizard is the one place the app introduces
            // itself, so it leads with the signet rather than a bare label.
            HStack(spacing: 10) {
                TorroLogoTile(size: 32)

                VStack(alignment: .leading, spacing: 1) {
                    Text(verbatim: "TorroWhisper")
                        .font(.headline)
                        .foregroundStyle(.primary)
                    Text("Setup", bundle: .module)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .textCase(.uppercase)
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 16)
            .padding(.bottom, 12)

            ForEach(Array(steps.enumerated()), id: \.offset) { index, title in
                HStack(spacing: 10) {
                    // Number circle 18 pt, digits monospaced so the column does
                    // not jitter between steps (design guide §Schritt-Zeile).
                    ZStack {
                        Circle()
                            .fill(index == currentStep ? Color.torroAccent : Color.secondary.opacity(0.18))
                            .frame(width: 18, height: 18)
                        if index < currentStep {
                            Image(systemName: "checkmark")
                                .font(.system(size: 10, weight: .bold))
                                .foregroundStyle(.white)
                        } else {
                            Text("\(index + 1)")
                                .font(.caption.weight(.bold).monospacedDigit())
                                .foregroundStyle(index == currentStep ? Color.white : Color.secondary)
                        }
                    }

                    Text(title)
                        .font(.subheadline)
                        .fontWeight(index == currentStep ? .semibold : .regular)
                        .foregroundStyle(index == currentStep ? Color.primary : Color.secondary)

                    Spacer()
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 6)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(
                    String(
                        format: L("Step %d of %d: %@", locale: locale),
                        index + 1,
                        steps.count,
                        title
                    )
                )
                .accessibilityAddTraits(index == currentStep ? [.isSelected] : [])
            }

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .background(Color(nsColor: .underPageBackgroundColor))
    }
}
