import AppKit
import SwiftUI

struct ModeEditorSheet: View {
    @ObservedObject var model: AppModel
    let onCancel: () -> Void
    let onSave: (ProcessingMode) throws -> Void
    @Environment(\.locale) private var locale
    @State private var draft: ProcessingMode
    @State private var stageCatalog: [StageCatalogEntry] = []
    @State private var saveError: String?

    init(
        model: AppModel,
        mode: ProcessingMode,
        onCancel: @escaping () -> Void,
        onSave: @escaping (ProcessingMode) throws -> Void
    ) {
        self.model = model
        self.onCancel = onCancel
        self.onSave = onSave
        _draft = State(initialValue: mode)
    }

    var body: some View {
        TorroSheetFrame(
            symbol: "square.text.square",
            title: sheetTitle,
            subtitle: Text("Changes are only applied when you save.", bundle: .module),
            errorText: trimmedName.isEmpty ? L("Enter a name.", locale: locale) : saveError
        ) {
            HStack(spacing: 0) {
                VStack(alignment: .leading, spacing: 14) {
                    editorLabel("Name")
                    TextField("", text: $draft.name)
                        .textFieldStyle(.roundedBorder)
                        .accessibilityLabel(Text("Name", bundle: .module))

                    editorLabel("Prompt")
                    TextEditor(text: $draft.prompt)
                        .font(.body)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
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
                        .accessibilityLabel(Text("Prompt", bundle: .module))
                }
                .padding(20)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)

                Divider()

                VStack(alignment: .leading, spacing: 12) {
                    editorSection("Language model") {
                        Picker(selection: $draft.postProcessingChoice) {
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
                            .lineLimit(2)
                            .help(modelHintText)
                    }

                    Divider()

                    editorSection("Dictionary") {
                        Toggle(isOn: dictionaryBinding) {
                            Text("Apply dictionary", bundle: .module)
                        }
                        .toggleStyle(.checkbox)
                        Text("Uses global word replacements for this post-processing.", bundle: .module)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }

                    Divider()

                    editorSection("Processing steps") {
                        pipelineContent
                    }
                }
                .padding(20)
                .frame(width: 380)
                .frame(maxHeight: .infinity, alignment: .topLeading)
            }
            .onAppear {
                stageCatalog = (try? BridgeClient().listPipelineStages()) ?? []
            }
        } footer: {
            Button(action: onCancel) {
                Text("Cancel", bundle: .module)
            }
            .keyboardShortcut(.cancelAction)

            Button {
                var normalizedDraft = draft
                normalizedDraft.name = trimmedName
                do {
                    try onSave(normalizedDraft)
                } catch {
                    saveError = error.localizedDescription
                }
            } label: {
                Text("Save", bundle: .module)
            }
            .buttonStyle(.borderedProminent)
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(trimmedName.isEmpty)
        }
        .frame(width: 860, height: 620)
    }

    private var sheetTitle: Text {
        if model.settings.modes.contains(where: { $0.id == draft.id }) {
            Text("Edit post-processing", bundle: .module)
        } else {
            Text("New post-processing", bundle: .module)
        }
    }

    private var trimmedName: String {
        draft.name.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var dictionaryBinding: Binding<Bool> {
        Binding(get: { draft.effectiveDictionaryEnabled }, set: { draft.setDictionaryEnabled($0) })
    }

    private func editorLabel(_ title: LocalizedStringKey) -> some View {
        Text(title, bundle: .module)
            .font(.headline)
    }

    private func editorSection<Content: View>(
        _ title: LocalizedStringKey,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 9) {
            Text(title, bundle: .module)
                .font(.headline)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var modelHintText: String {
        if let choice = draft.postProcessingChoice {
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
    private var pipelineContent: some View {
        if draft.pipeline.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(draft.synthesizedPipeline(postProcessingEnabled: model.settings.postProcessingEnabled)) { step in
                    Toggle(isOn: .constant(step.stageId == "auto_correct" ? step.isAIAutocorrectionEnabled : step.enabled)) {
                        Text(stageDisplayName(step.stageId))
                    }
                    .toggleStyle(.checkbox)
                    .disabled(true)
                }
                Button {
                    draft.pipeline = draft.synthesizedPipeline(
                        postProcessingEnabled: model.settings.postProcessingEnabled
                    )
                } label: {
                    Text("Customize steps…", bundle: .module)
                }
                pipelineHint
            }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(draft.pipeline.indices), id: \.self) { index in
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 8) {
                            Toggle(isOn: stepEnabledBinding(index)) {
                                Text(stageDisplayName(draft.pipeline[index].stageId))
                            }
                            .toggleStyle(.checkbox)

                            Spacer(minLength: 4)

                            Button {
                                draft.pipeline.swapAt(index, index - 1)
                            } label: {
                                Image(systemName: "arrow.up")
                            }
                            .buttonStyle(.borderless)
                            .disabled(index == draft.pipeline.startIndex)
                            .help(Text("Move up", bundle: .module))
                            .accessibilityLabel(Text("Move up", bundle: .module))

                            Button {
                                draft.pipeline.swapAt(index, index + 1)
                            } label: {
                                Image(systemName: "arrow.down")
                            }
                            .buttonStyle(.borderless)
                            .disabled(index == draft.pipeline.index(before: draft.pipeline.endIndex))
                            .help(Text("Move down", bundle: .module))
                            .accessibilityLabel(Text("Move down", bundle: .module))
                        }

                        if draft.pipeline[index].stageId == "auto_correct",
                           draft.pipeline[index].isAIAutocorrectionEnabled {
                            Text("Corrects spelling, grammar and punctuation.", bundle: .module)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        if draft.pipeline[index].stageId == "auto_correct",
                           let unsupported = draft.pipeline[index].unsupportedAutocorrectionMode {
                            Text(String(format: L("Unsupported correction: %@. Toggle AI auto-correction to replace it.", locale: locale), unsupported))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }

                Button {
                    draft.pipeline = []
                } label: {
                    Text("Reset to automatic", bundle: .module)
                }

                pipelineHint
            }
        }
    }

    private func stageDisplayName(_ stageId: String) -> String {
        switch stageId {
        case "dictionary": return L("Dictionary", locale: locale)
        case "auto_correct": return L("AI auto-correction", locale: locale)
        case "llm": return L("Language model", locale: locale)
        default: return stageCatalog.first(where: { $0.stageId == stageId })?.displayName ?? stageId
        }
    }

    private var pipelineHint: some View {
        Text("Steps run from top to bottom.", bundle: .module)
            .font(.caption)
            .foregroundStyle(.secondary)
    }

    private func stepEnabledBinding(_ index: Int) -> Binding<Bool> {
        Binding(
            get: {
                let step = draft.pipeline[index]
                return step.stageId == "auto_correct" ? step.isAIAutocorrectionEnabled : step.enabled
            },
            set: { value in
                if draft.pipeline[index].stageId == "auto_correct" {
                    draft.pipeline[index].setAIAutocorrectionEnabled(value)
                } else if draft.pipeline[index].stageId == "dictionary" {
                    draft.setDictionaryEnabled(value)
                } else {
                    draft.pipeline[index].enabled = value
                }
            }
        )
    }
}
