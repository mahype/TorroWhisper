import AppKit
import SwiftUI

struct FeedbackView: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 12) {
                TorroBadge(symbol: "bubble.left.and.text.bubble.right.fill", size: 34, variant: .own)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Send feedback", bundle: .module)
                        .font(.headline)
                    Text("Thanks for helping make TorroWhisper better. Pick a channel:", bundle: .module)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
            }

            VStack(spacing: 10) {
                FeedbackChannelTile(
                    iconSystemName: "ladybug.fill",
                    title: "GitHub Issues",
                    subtitle: L("Report bugs or submit feature requests.", locale: .current),
                    actionLabel: L("Open on GitHub", locale: .current),
                    action: openGitHubIssues
                )
            }

            Spacer(minLength: 0)
        }
        .padding(20)
        .frame(minWidth: 420, minHeight: 280)
    }

    private func openGitHubIssues() {
        guard let url = URL(string: "https://github.com/mahype/TorroWhisper/issues") else { return }
        NSWorkspace.shared.open(url)
    }
}

struct FeedbackChannelTile: View {
    let iconSystemName: String
    let title: String
    let subtitle: String
    let actionLabel: String
    let action: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            // A foreign service (GitHub) — the brand does not rub off, so the
            // badge is the neutral variant (design guide §Kachel-Badges).
            TorroBadge(symbol: iconSystemName, size: 34, variant: .foreign)

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.body.weight(.medium))
                    .foregroundStyle(.primary)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Spacer(minLength: 8)

            Button(actionLabel, action: action)
                .controlSize(.regular)
        }
        .padding(12)
        .torroCard(cornerRadius: 10)
    }
}
