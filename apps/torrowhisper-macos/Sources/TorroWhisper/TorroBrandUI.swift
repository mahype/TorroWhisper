import SwiftUI

// Torro brand UI shell — the reusable building blocks from the design guide's
// `swift/TorroBrandUI.swift` drop-in (repo `mahype/torro-design`, `app-design.md`).
//
// The colors and the horns signet (`TorroHorns`, `TorroLogoTile`) live in
// `TorroBrand.swift` and are reused here — this file adds the product wordmark,
// the hero, the card surface, the button style, the tile badge, the chips and
// the sheet chrome on top.
//
// LICENSE NOTE: the guide's wordmark and hero are set in Frutiger LT 95
// UltraBlack. This repo is public and must not ship Frutiger (see AGENTS.md
// §"Lizenz-Grenze"), so the display cut is deliberately replaced by a heavy
// system font. In this app the brand carries through color and the signet,
// not the typeface.

// MARK: - Drawn brand shape (single horn)

/// A single horn swinging outward, traced from the TORROFORMS wordmark
/// (`logo/wordmark/torroforms-wordmark.svg`). Mirror it for the right side.
/// The inward-turned two-horn signet is `TorroHorns` in `TorroBrand.swift`.
struct TorroHorn: Shape {
    private static let artwork = CGRect(
        x: 46.429688, y: 59.398438, width: 55.765624, height: 68.832031
    )

    /// Width : height of the drawing, so callers can size it without distortion.
    static let aspectRatio = artwork.width / artwork.height

    func path(in rect: CGRect) -> Path {
        let scaleX = rect.width / Self.artwork.width
        let scaleY = rect.height / Self.artwork.height

        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
            CGPoint(
                x: rect.minX + (x - Self.artwork.minX) * scaleX,
                y: rect.minY + (y - Self.artwork.minY) * scaleY
            )
        }

        var path = Path()
        path.move(to: p(102.195312, 128.230469))
        path.addCurve(to: p(46.429688, 100.71875), control1: p(92.625, 128.230469), control2: p(45.835938, 128.230469))
        path.addCurve(to: p(82.910156, 59.398438), control1: p(47.664062, 76.40625), control2: p(82.910156, 59.398438))
        path.addCurve(to: p(79.394531, 88.992188), control1: p(82.910156, 59.398438), control2: p(77.566406, 85.226562))
        path.addCurve(to: p(102.195312, 93.8125), control1: p(80.632812, 91.546875), control2: p(83.589844, 94.261719))
        path.closeSubpath()
        return path
    }
}

// MARK: - Wordmark

/// The product lockup after the TORROFORMS pattern: horns on both sides, the
/// name in a heavy system cut (Frutiger is not shipped here — see the license
/// note above), "TORRO" in the leading tone and the product name in the
/// secondary tone.
///
///     TorroWordmark(product: "WHISPER", style: .onBrand)  // on the red hero
///     TorroWordmark(product: "WHISPER", style: .still)    // sidebar foot
struct TorroWordmark: View {
    /// Color treatment. `.onBrand` reads on the red hero / dark ground; `.still`
    /// is theme-aware for a light or dark sidebar foot — a white signet on a
    /// light ground is unreadable (AGENTS.md), so it uses the primary tone.
    enum Style {
        case onBrand
        case still
    }

    /// Product half of the name, uppercased (e.g. "WHISPER").
    var product: String
    /// Cap height. Everything else derives from it — the original proportions.
    var capHeight: CGFloat
    var style: Style
    /// For VoiceOver; empty → "Torro" + product name.
    var accessibilityName: String

    init(
        product: String,
        capHeight: CGFloat = 11,
        style: Style = .onBrand,
        accessibilityName: String = ""
    ) {
        self.product = product
        self.capHeight = capHeight
        self.style = style
        self.accessibilityName = accessibilityName.isEmpty
            ? "Torro" + product.capitalized
            : accessibilityName
    }

    private var hornHeight: CGFloat { capHeight * 2.0 }
    private var hornWidth: CGFloat { hornHeight * TorroHorn.aspectRatio }

    private var torroColor: Color { style == .onBrand ? .white : .primary }
    private var productColor: Color { style == .onBrand ? .torroSilver : .secondary }
    private var hornColor: Color { style == .onBrand ? .white : .primary }

    var body: some View {
        HStack(alignment: .lastTextBaseline, spacing: capHeight * 0.14) {
            horn(mirrored: false)
            Text(verbatim: "TORRO").foregroundStyle(torroColor)
                + Text(verbatim: product).foregroundStyle(productColor)
            horn(mirrored: true)
        }
        // A heavy system cut stands in for Frutiger's UltraBlack. Frutiger's
        // caps sit at ~0.7 em, so the same size heuristic keeps the geometry.
        .font(.system(size: capHeight / 0.7, weight: .heavy))
        .lineLimit(1)
        .fixedSize()
        .accessibilityElement()
        .accessibilityLabel(accessibilityName)
    }

    private func horn(mirrored: Bool) -> some View {
        TorroHorn()
            .fill(hornColor)
            .frame(width: hornWidth, height: hornHeight)
            .scaleEffect(x: mirrored ? -1 : 1)
            // The horns end on the baseline, exactly like the letters.
            .alignmentGuide(.lastTextBaseline) { $0[.bottom] }
    }
}

// MARK: - Hero

/// The one loud brand moment per window: a red gradient ground, the wordmark,
/// one line of plain text, the signet as a bleed-off watermark on the right.
///
/// A full-bleed header band, not a card (design guide §Hero / §Fenster; recipe
/// mirrored from TorroMail's `BrandHero`). Pin it above the scroll content with
/// `.safeAreaInset(edge: .top, spacing: 0)` so its red runs up to the window's
/// top edge behind a background-less toolbar. The text column matches the 720-pt
/// content column below, so the wordmark and the cards share a left edge.
struct TorroBrandHero: View {
    var product: String
    var tagline: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            TorroWordmark(product: product, capHeight: 15, style: .onBrand)
            // No `.fixedSize(...)` here: outside a ScrollView it drives the
            // window's fitting-size negotiation into the text's minimum width
            // and the whole split view lays out collapsed and clipped. Plain
            // wrapping needs no help in this stack.
            Text(tagline)
                .font(.callout)
                .foregroundStyle(.white.opacity(0.92))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 20)
        .frame(maxWidth: 720)
        .frame(maxWidth: .infinity)
        .padding(.top, 6)
        .padding(.bottom, 16)
        .background {
            // Diagonal, top left to bottom right (design guide §Der Hero).
            LinearGradient(
                colors: [.torroRed, .torroRedDeep],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            // The signet as a watermark, cropped by the band on the right rather
            // than floating. An overlay, not a sibling — its fixed 150-pt height
            // would otherwise set the band height and the red would spill past.
            .overlay(alignment: .trailing) {
                TorroHorns()
                    .fill(.white.opacity(0.10))
                    .frame(width: 260, height: 150)
                    .offset(x: 95)
            }
            // Clip before extending: the expanded frame is what the signet is
            // cropped against, so the red and the crop reach the window's top
            // edge behind the background-less toolbar.
            .clipped()
            .shadow(color: .black.opacity(0.25), radius: 7, y: 2)
            .ignoresSafeArea(edges: .top)
        }
    }
}

// MARK: - Card surface

/// The raised surface every panel sits on: a lit top edge fading down over a
/// soft double shadow. The shadow hangs on the card ground alone — on the whole
/// panel every label would cast one too.
struct TorroCard: ViewModifier {
    @Environment(\.colorScheme) private var colorScheme
    var cornerRadius: CGFloat
    var isHighlighted: Bool

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        return content
            .background {
                shape
                    .fill(.background)
                    .shadow(color: .black.opacity(colorScheme == .dark ? 0.5 : 0.14), radius: 5, y: 3)
                    .shadow(color: .black.opacity(colorScheme == .dark ? 0.3 : 0.05), radius: 1, y: 1)
            }
            .overlay {
                shape.strokeBorder(edgeGradient, lineWidth: 1)
            }
            .contentShape(shape)
    }

    private var edgeGradient: LinearGradient {
        let top: Color
        let bottom: Color
        if colorScheme == .dark {
            top = .white.opacity(isHighlighted ? 0.34 : 0.20)
            bottom = .white.opacity(isHighlighted ? 0.10 : 0.05)
        } else {
            top = .white.opacity(0.9)
            bottom = .black.opacity(isHighlighted ? 0.18 : 0.10)
        }
        return LinearGradient(colors: [top, bottom], startPoint: .top, endPoint: .bottom)
    }
}

extension View {
    /// Lays the content on the Torro card surface. `isHighlighted` strengthens
    /// the edge line — for the hover of clickable cards (`easeOut 0.12s`).
    func torroCard(cornerRadius: CGFloat = 12, isHighlighted: Bool = false) -> some View {
        modifier(TorroCard(cornerRadius: cornerRadius, isHighlighted: isHighlighted))
    }

}

// MARK: - Shared metrics

enum TorroMetrics {
    /// Status dot diameter. The guide allows 8–9 pt; pinning it here keeps the
    /// dot the same size in the overview, the settings foot and the indicator
    /// instead of drifting per view.
    static let statusDot: CGFloat = 9
}

// MARK: - Button tint

/// Drops the app-wide brand tint from ordinary push buttons.
///
/// Brand red is a ground/accent color. On a system button the tint colors the
/// *label* instead of the fill, and red text on a dark ground is unreadable —
/// the design guide rules it out outright ("Kein Button trägt seine Bedeutung
/// über die Textfarbe"). Applied once at the app root it reaches every button
/// that has not chosen a style of its own, so a new button is neutral by
/// default instead of having to remember to opt out.
///
/// Buttons that set their own style stay untouched on purpose: `borderedProminent`
/// footer primaries keep the red *fill* with a white label, and the occasional
/// `borderless` text accent (the guide's "Log öffnen") is allowed to be red.
struct TorroNeutralButtonStyle: PrimitiveButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        // Inside a PrimitiveButtonStyle the style resets to `.automatic`, so this
        // rebuilds the button in its native look — only the tint is cleared.
        Button(configuration).tint(nil)
    }
}

// MARK: - Tile badge

/// The rounded tile that stands for "the app's own thing" (`.own`, brand red)
/// or a foreign one (`.foreign`, neutral system fill) — the brand does not rub
/// off on third parties.
struct TorroBadge: View {
    enum Variant {
        case own
        case foreign
    }

    var symbol: String
    var size: CGFloat
    var variant: Variant

    init(symbol: String, size: CGFloat = 34, variant: Variant = .own) {
        self.symbol = symbol
        self.size = size
        self.variant = variant
    }

    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.24, style: .continuous)
            .fill(fill)
            .frame(width: size, height: size)
            .overlay {
                Image(systemName: symbol)
                    .font(.system(size: size * 0.41, weight: .semibold))
                    .foregroundStyle(symbolColor)
            }
            .shadow(color: .black.opacity(variant == .own ? 0.2 : 0), radius: 1.5, y: 1)
    }

    private var fill: AnyShapeStyle {
        switch variant {
        case .own:
            return AnyShapeStyle(Color.torroRed.gradient)
        case .foreign:
            return AnyShapeStyle(.quaternary)
        }
    }

    private var symbolColor: Color {
        variant == .own ? .white : .secondary
    }
}

// MARK: - Chips

/// The guide's chip family (`app-design.md` §Chips & Kapseln). One capsule shape,
/// four meanings — a counter in tint, "new" in brand red, an orange exception,
/// a neutral pill.
struct TorroChip: View {
    enum Kind {
        /// Waiting actions on a card: tinted capsule, white monospaced digits.
        case counter
        /// News entries: brand-red capsule.
        case new
        /// A deviation from the default ("customized"): orange.
        case exception
        /// A neutral state ("standard"): faint primary fill.
        case neutral
    }

    var text: String
    var kind: Kind

    var body: some View {
        Text(text)
            .font(font)
            .monospacedDigit()
            .foregroundStyle(foreground)
            .padding(.vertical, 2)
            .padding(.horizontal, 7)
            .background(background, in: Capsule())
    }

    private var font: Font {
        switch kind {
        case .counter: return .caption.weight(.semibold)
        case .new: return .caption2.weight(.bold)
        case .exception, .neutral: return .caption2.weight(.semibold)
        }
    }

    private var foreground: Color {
        switch kind {
        case .counter, .new: return .white
        case .exception: return .orange
        case .neutral: return .secondary
        }
    }

    private var background: AnyShapeStyle {
        switch kind {
        case .counter: return AnyShapeStyle(Color.torroAccent)
        case .new: return AnyShapeStyle(Color.torroRed)
        case .exception: return AnyShapeStyle(Color.orange.opacity(0.16))
        case .neutral: return AnyShapeStyle(Color.primary.opacity(0.06))
        }
    }
}

/// A status capsule tinted by a semantic system color (green/orange/red/gray),
/// used for at-a-glance verdicts. The color is a system status color, never
/// brand red (AGENTS.md).
struct TorroStatusChip: View {
    var text: String
    var color: Color

    var body: some View {
        Text(text)
            .font(.caption.weight(.semibold))
            .padding(.vertical, 3)
            .padding(.horizontal, 7)
            .background(color.opacity(0.14), in: Capsule())
            .foregroundStyle(color)
    }
}

// MARK: - Sheet chrome

/// The guide's sheet/wizard frame: a head (a brand-red SF symbol · title · an
/// optional subtitle) — Divider — content — Divider — a foot.
///
/// The foot's left slot is the error message and belongs to the frame, so every
/// sheet reports failures in the same place and the same way; `footer` fills the
/// right side with Cancel and exactly one primary button.
struct TorroSheetFrame<Content: View, Footer: View>: View {
    var symbol: String
    var title: Text
    var subtitle: Text?
    var errorText: String?
    @ViewBuilder var content: () -> Content
    @ViewBuilder var footer: () -> Footer

    init(
        symbol: String,
        title: Text,
        subtitle: Text? = nil,
        errorText: String? = nil,
        @ViewBuilder content: @escaping () -> Content,
        @ViewBuilder footer: @escaping () -> Footer
    ) {
        self.symbol = symbol
        self.title = title
        self.subtitle = subtitle
        self.errorText = errorText
        self.content = content
        self.footer = footer
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Image(systemName: symbol)
                    .font(.title2)
                    .foregroundStyle(Color.torroAccent)
                    .frame(width: 26)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    title.font(.headline)
                    if let subtitle {
                        subtitle
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 16)

            Divider()

            content()

            Divider()

            HStack(spacing: 10) {
                if let errorText, !errorText.isEmpty {
                    Label {
                        Text(errorText)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    } icon: {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)
                    }
                    .labelStyle(.titleAndIcon)
                }
                Spacer(minLength: 0)
                footer()
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
        }
    }
}
