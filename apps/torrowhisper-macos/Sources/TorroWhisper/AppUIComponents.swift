import AppKit
import SwiftUI

enum SettingsSection: String, CaseIterable, Identifiable {
    case recording
    case modes
    case dictionary
    case history
    case languageModels = "language_models"
    case plugins
    case startup
    case updates
    case diagnostics
    case help

    var id: String { rawValue }

    func title(locale: Locale) -> String {
        switch self {
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
        case .plugins:
            return L("Plugins", locale: locale)
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
        case .plugins:
            return "puzzlepiece.extension.fill"
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
    let isActive: Bool
    let canDelete: Bool
    let onActivate: () -> Void
    let onEdit: () -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Button(action: onActivate) {
                HStack(spacing: 10) {
                    Image(systemName: isActive ? "largecircle.fill.circle" : "circle")
                        .font(.body)
                        .foregroundStyle(isActive ? Color.accentColor : Color.secondary.opacity(0.7))
                        .accessibilityHidden(true)

                    VStack(alignment: .leading, spacing: 2) {
                        Text(mode.name)
                            .font(.body.weight(.medium))
                            .foregroundStyle(.primary)
                        if mode.prompt.isEmpty {
                            Text("No prompt set", bundle: .module)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        } else {
                            Text(mode.prompt)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }

                    Spacer(minLength: 8)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(isActive ? [.isSelected] : [])

            Button(action: onEdit) {
                Image(systemName: "pencil")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
            .help(Text("Edit post-processing", bundle: .module))
            .accessibilityLabel(Text("Edit post-processing", bundle: .module))

            Button(action: onDelete) {
                Image(systemName: "trash")
                    .foregroundStyle(canDelete ? .secondary : Color.secondary.opacity(0.35))
            }
            .buttonStyle(.borderless)
            .disabled(!canDelete)
            .help(Text(canDelete ? "Delete post-processing" : "At least one post-processing must remain", bundle: .module))
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
                    .foregroundStyle(isActive ? Color.accentColor : Color.secondary.opacity(0.7))
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
                    .foregroundStyle(isSelected ? Color.accentColor : Color.secondary.opacity(0.7))
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

struct ModeEditorSheet: View {
    @ObservedObject var model: AppModel
    let onDone: () -> Void
    @Environment(\.locale) private var locale
    @State private var stageCatalog: [StageCatalogEntry] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Edit post-processing", bundle: .module)
                    .font(.title3.weight(.semibold))
                Spacer()
            }

            Form {
                Section {
                    TextField(text: model.modeBinding(for: \.name)) {
                        Text("Name", bundle: .module)
                    }
                }

                Section {
                    TextEditor(text: model.modeBinding(for: \.prompt))
                        .font(.body)
                        .frame(minHeight: 180)
                        .scrollContentBackground(.hidden)
                        .padding(6)
                        .background(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .fill(Color(nsColor: .textBackgroundColor))
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(Color.primary.opacity(0.08), lineWidth: 1)
                        )
                        .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
                } header: {
                    Text("Prompt", bundle: .module)
                }

                Section {
                    Picker(selection: model.modeChoiceBinding()) {
                        Text("Default (global)", bundle: .module)
                            .tag(Optional<PostProcessingChoice>.none)
                        ForEach(model.availablePostProcessingChoices) { choice in
                            Text(model.postProcessingChoicePickerLabel(choice))
                                .tag(Optional(choice))
                        }
                    } label: {
                        Text("Model", bundle: .module)
                    }

                    Text(modelHintText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } header: {
                    Text("Language model", bundle: .module)
                }

                Section {
                    Toggle(isOn: model.modeBinding(for: \.dictionaryEnabled)) {
                        Text("Apply dictionary in this mode", bundle: .module)
                    }
                } footer: {
                    Text("If enabled, global word replacements are applied to the transcript before this mode runs.", bundle: .module)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                pipelineSection
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
            .onAppear {
                stageCatalog = (try? BridgeClient().listPipelineStages()) ?? []
            }

            HStack {
                Spacer()
                Button(action: onDone) {
                    Text("Done", bundle: .module)
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(minWidth: 460, idealWidth: 520, minHeight: 380, idealHeight: 440)
    }

    private var modelHintText: String {
        if let choice = model.modeChoiceBinding().wrappedValue {
            return String(
                format: L("This profile uses: %@.", locale: locale),
                model.postProcessingChoiceLabel(choice)
            )
        }
        let global = model.postProcessingChoiceBinding.wrappedValue
        return String(
            format: L("Uses global model: %@.", locale: locale),
            model.postProcessingChoiceLabel(global)
        )
    }

    @ViewBuilder
    private var pipelineSection: some View {
        let steps = model.modeBinding(for: \.pipeline)
        Section {
            if steps.wrappedValue.isEmpty {
                Text("Automatic order: Dictionary → Auto-correct → LLM.", bundle: .module)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button {
                    steps.wrappedValue = model.editingMode.synthesizedPipeline(
                        postProcessingEnabled: model.settings.postProcessingEnabled
                    )
                } label: {
                    Text("Customize pipeline…", bundle: .module)
                }
            } else {
                ForEach(steps) { $step in
                    HStack(spacing: 8) {
                        Image(systemName: "line.3.horizontal")
                            .foregroundStyle(.tertiary)
                        Toggle(isOn: $step.enabled) {
                            Text(stageDisplayName(step.stageId))
                        }
                        .toggleStyle(.checkbox)
                        Spacer()
                        if step.stageId == "auto_correct" {
                            Picker("", selection: autoCorrectModeBinding($step)) {
                                Text("Off", bundle: .module).tag("off")
                                Text("LLM cleanup", bundle: .module).tag("llm")
                            }
                            .labelsHidden()
                            .frame(width: 130)
                        }
                    }
                }
                .onMove { from, to in
                    steps.wrappedValue.move(fromOffsets: from, toOffset: to)
                }

                Button(role: .destructive) {
                    steps.wrappedValue = []
                } label: {
                    Text("Reset to automatic", bundle: .module)
                }
            }
        } header: {
            Text("Pipeline", bundle: .module)
        } footer: {
            Text("Steps run top to bottom. Drag to reorder, toggle to enable. Plugins can add steps.", bundle: .module)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private func stageDisplayName(_ stageId: String) -> String {
        stageCatalog.first(where: { $0.stageId == stageId })?.displayName ?? stageId
    }

    private func autoCorrectModeBinding(_ step: Binding<PipelineStepConfig>) -> Binding<String> {
        Binding(
            get: { step.wrappedValue.config?["mode"] ?? "off" },
            set: { newValue in
                var config = step.wrappedValue.config ?? [:]
                config["mode"] = newValue
                step.wrappedValue.config = config
            }
        )
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
                        Text(L("Cancelled", locale: locale))
                            .font(.caption.weight(.semibold))
                            .padding(.vertical, 1)
                            .padding(.horizontal, 6)
                            .background(Color.orange.opacity(0.18), in: Capsule())
                            .foregroundStyle(.orange)
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
        Text(status.label(locale: locale))
            .font(.caption.weight(.semibold))
            .padding(.vertical, 3)
            .padding(.horizontal, 7)
            .background(backgroundColor.opacity(0.14), in: Capsule())
            .foregroundStyle(backgroundColor)
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

struct DiagnosticDisclosureCard: View {
    let item: DiagnosticItemDTO
    @Environment(\.locale) private var locale

    var body: some View {
        DisclosureGroup {
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
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.top, 6)
        } label: {
            HStack(spacing: 10) {
                Text(L(item.title, locale: locale))
                    .font(.body.weight(.medium))
                Spacer()
                DiagnosticStatusBadge(status: item.status)
            }
        }
    }
}

struct StepRail: View {
    let currentStep: Int
    @Environment(\.locale) private var locale

    private var steps: [String] {
        [
            L("Welcome", locale: locale),
            L("Audio & hotkey", locale: locale),
            L("Permissions", locale: locale),
            L("Language models", locale: locale),
            L("Start & behavior", locale: locale),
            L("Diagnostics", locale: locale),
        ]
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("Setup", bundle: .module)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .textCase(.uppercase)
                .padding(.horizontal, 16)
                .padding(.top, 16)
                .padding(.bottom, 8)

            ForEach(Array(steps.enumerated()), id: \.offset) { index, title in
                HStack(spacing: 10) {
                    ZStack {
                        Circle()
                            .fill(index == currentStep ? Color.accentColor : Color.secondary.opacity(0.18))
                            .frame(width: 20, height: 20)
                        if index < currentStep {
                            Image(systemName: "checkmark")
                                .font(.system(size: 10, weight: .bold))
                                .foregroundStyle(.white)
                        } else {
                            Text("\(index + 1)")
                                .font(.caption.weight(.semibold))
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
