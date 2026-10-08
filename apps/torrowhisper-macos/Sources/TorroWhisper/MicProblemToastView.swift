import SwiftUI

/// Toast for a microphone that delivers no audio (#76): muted at the CoreAudio
/// level, blocked by the microphone permission, or silent for another reason.
/// macOS hides most of this (System Settings only shows the input volume), so
/// the toast names the one cause it found and offers buttons that fix it or
/// open the exact place to look. Styled like `MicSwitchToastView`, but
/// clickable.
struct MicProblemToastView: View {
    struct Action {
        let label: String
        let isPrimary: Bool
        let perform: () -> Void
    }

    let symbol: String
    let title: String
    let detail: String
    let actions: [Action]

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: symbol)
                .font(.system(size: 18, weight: .semibold))
                .foregroundStyle(.white)
                .padding(.top, 1)

            VStack(alignment: .leading, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.white)
                        .lineLimit(2)
                    if !detail.isEmpty {
                        Text(detail)
                            .font(.system(size: 11))
                            .foregroundStyle(.white.opacity(0.75))
                            .lineLimit(4)
                    }
                }
                .multilineTextAlignment(.leading)
                .fixedSize(horizontal: false, vertical: true)

                if !actions.isEmpty {
                    HStack(spacing: 8) {
                        ForEach(Array(actions.enumerated()), id: \.offset) { _, action in
                            Button(action: action.perform) {
                                Text(action.label)
                                    .font(.system(size: 12, weight: .semibold))
                                    .foregroundStyle(action.isPrimary ? Color.black : Color.white)
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 5)
                                    .background(
                                        Capsule().fill(action.isPrimary ? Color.white : Color.white.opacity(0.16))
                                    )
                            }
                            .buttonStyle(.plain)
                            .fixedSize()
                        }
                    }
                }
            }

            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(Color.black.opacity(0.78))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(Color.white.opacity(0.08), lineWidth: 0.5)
        )
        .shadow(color: .black.opacity(0.35), radius: 16, x: 0, y: 6)
    }
}
