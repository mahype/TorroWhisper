import AVFoundation
import SwiftUI

/// The overview page — the settings window's landing section. It carries the
/// one loud brand moment (the borderless hero, design guide §Hero / §Fenster)
/// and, below it, at-a-glance status tiles for the app's independent services.
/// The other sidebar sections stay plain native forms.
struct OverviewView: View {
    @ObservedObject var model: AppModel
    @Environment(\.locale) private var locale

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                // The one thing the user can decide here, on the surface rather
                // than in a permanent footer bar: what dictation is doing, and
                // the button that starts or stops it.
                dictationCard

                VStack(alignment: .leading, spacing: 10) {
                    Text("At a glance", bundle: .module)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.secondary)

                    // Independent services sit side by side as tiles, not stacked
                    // as a list (design guide §Status-Kachel).
                    LazyVGrid(
                        columns: [
                            GridItem(.flexible(), spacing: 10),
                            GridItem(.flexible(), spacing: 10),
                        ],
                        spacing: 10
                    ) {
                        StatusTile(title: L("Microphone", locale: locale), state: micState, status: micStatus)
                        StatusTile(title: L("Accessibility", locale: locale), state: accessibilityState, status: accessibilityStatus)
                        StatusTile(title: model.selectedModelDisplayName, state: modelState, status: modelStatus)
                        StatusTile(title: L("Hotkey", locale: locale), state: hotkeyState, status: hotkeyStatus)
                    }
                }
            }
            .padding(20)
            .frame(maxWidth: 720, alignment: .top)
            .frame(maxWidth: .infinity)
        }
        .background(.background.secondary)
        // The hero is the header of this pane: pinned above the scroll content at
        // full width, its red running up behind the toolbar. The toolbar keeps no
        // background and no title — the wordmark takes that role. An empty title
        // (not a removed modifier) so a title a previous pane set does not linger.
        .safeAreaInset(edge: .top, spacing: 0) {
            TorroBrandHero(
                product: "WHISPER",
                tagline: L("Local dictation for your Mac. Speak — and the text lands where you're typing.", locale: locale)
            )
        }
        .navigationTitle("")
        .toolbarBackground(.hidden, for: .windowToolbar)
    }

    // MARK: - Dictation

    /// A status tile that also carries the app's primary action. Dictation is
    /// normally started with the hotkey; this is the same decision spelled out
    /// on the surface, where the guide wants it — not parked in a footer bar
    /// under every pane.
    private var dictationCard: some View {
        HStack(alignment: .center, spacing: 10) {
            Circle()
                .fill(dictationStatus.color)
                .frame(width: TorroMetrics.statusDot, height: TorroMetrics.statusDot)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 2) {
                Text("Dictation", bundle: .module)
                    .font(.headline)
                Text(dictationState)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Spacer(minLength: 10)

            Button {
                model.toggleDictation()
            } label: {
                Text(model.runtime.isRecording ? "Stop" : "Start dictation", bundle: .module)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .torroCard()
        // The dot does not explain itself (design guide §Statuspunkt): the
        // sentence beside it says the same thing, and the pair is announced once.
        .accessibilityElement(children: .contain)
    }

    /// Broken is red; everything else the runtime reports — recording,
    /// transcribing, post-processing — is the app working as intended and stays
    /// green. Brand red is never a status color, and system red means broken,
    /// not "busy" (design guide §Statusfarben).
    private var dictationStatus: OverviewStatus {
        model.bridgeError == nil ? .ok : .error
    }

    private var dictationState: String {
        if let error = model.bridgeError {
            return error
        }
        if model.runtime.isRecording {
            return L("Recording active", locale: locale)
        }
        if model.runtime.isPostProcessing {
            return L("Post-processing in progress", locale: locale)
        }
        if model.runtime.isTranscribing {
            return L("Transcription in progress", locale: locale)
        }
        if model.runtime.dictationModelWarming {
            return L("Loading speech model…", locale: locale)
        }
        return model.runtime.lastStatus.isEmpty
            ? L("Ready", locale: locale)
            : L(model.runtime.lastStatus, locale: locale)
    }

    // MARK: - Microphone

    private var micStatus: OverviewStatus {
        switch model.microphoneAuthorizationStatus {
        case .authorized: return .ok
        case .denied, .restricted: return .error
        default: return .attention
        }
    }

    private var micState: String {
        micStatus == .ok ? L("Ready", locale: locale) : L("Not granted", locale: locale)
    }

    // MARK: - Accessibility

    private var accessibilityStatus: OverviewStatus {
        model.accessibilityTrusted ? .ok : .attention
    }

    private var accessibilityState: String {
        model.accessibilityTrusted ? L("Ready", locale: locale) : L("Not granted", locale: locale)
    }

    // MARK: - Transcription model

    /// The download status of the currently selected transcription model,
    /// resolved the same way the onboarding step does.
    private var whisperStatus: ModelStatusDTO? {
        if model.modelStatusList.isEmpty {
            return model.modelStatus
        }
        return model.modelStatusList.first { $0.backendModelName == model.settings.localModel.whisperModel }
    }

    private var modelStatus: OverviewStatus {
        let status = whisperStatus
        if status?.isDownloaded ?? false {
            return .ok
        }
        return .attention
    }

    private var modelState: String {
        let status = whisperStatus
        if status?.isDownloaded ?? false {
            return L("Ready", locale: locale)
        }
        if status?.isDownloading ?? false {
            return L("Loading…", locale: locale)
        }
        return L("Not downloaded", locale: locale)
    }

    // MARK: - Hotkey

    private var hotkeyStatus: OverviewStatus {
        model.runtime.hotkeyRegistered ? .ok : .attention
    }

    private var hotkeyState: String {
        let text = model.runtime.hotkeyText
        return text.isEmpty ? model.settings.hotkey : text
    }
}

/// Semantic status of a service, mapped to a system color. Brand red is never a
/// status color (AGENTS.md) — status uses the system green/orange/red/gray.
enum OverviewStatus {
    case ok
    case attention
    case error
    case inactive

    var color: Color {
        switch self {
        case .ok: return .green
        case .attention: return .orange
        case .error: return .red
        case .inactive: return .gray
        }
    }
}

/// A status tile: a colored dot (its own column), a title and a one-line state,
/// on a Torro card (design guide §Status-Kachel). The dot does not explain
/// itself, so the tile carries a tooltip and a combined accessibility label.
struct StatusTile: View {
    let title: String
    let state: String
    let status: OverviewStatus

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Circle()
                .fill(status.color)
                .frame(width: TorroMetrics.statusDot, height: TorroMetrics.statusDot)
                .padding(.top, 5)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.headline)
                Text(state)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Spacer(minLength: 0)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .torroCard()
        .help("\(title): \(state)")
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(title): \(state)")
    }
}
