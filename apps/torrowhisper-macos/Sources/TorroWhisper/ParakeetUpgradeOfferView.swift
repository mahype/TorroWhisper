import SwiftUI

/// One-time offer after an update, shown while a deprecated Parakeet model is
/// in use (#67). The user keeps dictating with the current model either way:
/// "Switch now" prepares the successor in the background, "Later" only hides
/// this window — Settings keeps offering the upgrade.
struct ParakeetUpgradeOfferView: View {
    @ObservedObject var model: AppModel
    var onClose: () -> Void
    @Environment(\.locale) private var locale

    var body: some View {
        let current = model.parakeetInfo(model.effectiveParakeetModel)?.displayLabel ?? ""
        let target = model.parakeetUpgradeTarget
        let targetName = target?.displayLabel ?? ""

        TorroSheetFrame(
            symbol: "waveform",
            title: Text(String(format: L("%@ is available", locale: locale), targetName)),
            subtitle: Text("A better transcription model is ready to download.", bundle: .module)
        ) {
            VStack(alignment: .leading, spacing: 14) {
                Text(
                    String(
                        format: L("Your current transcription model %@ is deprecated.", locale: locale),
                        current
                    )
                )
                .fixedSize(horizontal: false, vertical: true)

                VStack(alignment: .leading, spacing: 8) {
                    benefit(
                        symbol: "checkmark.seal",
                        text: L("Fewer recognition errors in German and English", locale: locale)
                    )
                    benefit(
                        symbol: "bolt",
                        text: L("Same speed, same 25 languages", locale: locale)
                    )
                    benefit(
                        symbol: "mic",
                        text: L("Keep dictating while it downloads", locale: locale)
                    )
                }

                if let target {
                    Label {
                        Text(
                            String(
                                format: L("Download about %@ · the old model is removed afterwards", locale: locale),
                                ByteCountFormatter.string(
                                    fromByteCount: Int64(target.approxSizeBytes),
                                    countStyle: .file
                                )
                            )
                        )
                    } icon: {
                        Image(systemName: "arrow.down.circle")
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 20)
            .padding(.vertical, 18)
        } footer: {
            Button {
                model.dismissParakeetUpgradeOffer()
                onClose()
            } label: {
                Text("Later", bundle: .module)
            }
            .keyboardShortcut(.cancelAction)

            Button {
                model.upgradeParakeet()
                onClose()
            } label: {
                Text("Switch now", bundle: .module)
            }
            .buttonStyle(.borderedProminent)
            .keyboardShortcut(.defaultAction)
        }
        .frame(width: 460)
    }

    private func benefit(symbol: String, text: String) -> some View {
        Label {
            Text(text)
        } icon: {
            Image(systemName: symbol)
                .foregroundStyle(Color.torroAccent)
                .frame(width: 18)
        }
    }
}
