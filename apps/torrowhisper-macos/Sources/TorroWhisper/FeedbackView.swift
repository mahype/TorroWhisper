import AppKit
import SwiftUI

struct FeedbackView: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Send feedback", bundle: .module)
                    .font(.title2.weight(.semibold))
                Text("Thanks for helping make TorroWhisper better. Pick a channel:", bundle: .module)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            VStack(spacing: 10) {
                FeedbackChannelTile(
                    iconSystemName: "ladybug.fill",
                    iconColor: .accentColor,
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
    let iconColor: Color
    let title: String
    let subtitle: String
    let actionLabel: String
    let action: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: iconSystemName)
                .font(.title3)
                .foregroundStyle(iconColor)
                .frame(width: 28)
                .accessibilityHidden(true)

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
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(Color(nsColor: .controlBackgroundColor))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(Color.secondary.opacity(0.15))
        )
    }
}
