import SwiftUI

/// Torro brand palette and signet.
///
/// Source of truth is the `mahype/torro-design` repository (see AGENTS.md);
/// the values here mirror its `tokens/tokens.json`. The red is a fixed brand
/// value — it stays `#D50C0C` in light and dark mode and must not be shifted
/// or desaturated to "fit" an appearance.
extension Color {
    /// Torro Red — the core brand color.
    static let torroRed = Color(red: 213 / 255, green: 12 / 255, blue: 12 / 255)
    /// Darker red for gradients, hover and depth.
    static let torroRedDeep = Color(red: 165 / 255, green: 10 / 255, blue: 10 / 255)
    /// Brand black — bull outline and text.
    static let torroBlack = Color(red: 14 / 255, green: 14 / 255, blue: 15 / 255)
    /// Silver, used as the wordmark's secondary tone.
    static let torroSilver = Color(red: 196 / 255, green: 195 / 255, blue: 195 / 255)

    /// The app's accent. Named separately from `torroRed` so call sites read as
    /// "the accent" rather than hardcoding the brand color, and so a future
    /// user-selectable accent has one place to hook into.
    static var torroAccent: Color { .torroRed }
}

/// The Torro signet: two horns turned inward.
///
/// Converted from `Resources/Brand/horns-white.svg` (the brand repo's
/// `logo/horns/`) into a resolution-independent shape. It is a `Shape`, not a
/// bundled image, so it tints via `foregroundStyle` — the guide's white / red /
/// black variants are the same geometry in a different color — and so it needs
/// no resource plumbing through the `.app` bundle.
///
/// The source viewBox is 150×150; the geometry is normalized to its content
/// bounding box and aspect-fitted into whatever rect it is given. Its natural
/// aspect ratio is ~1.73:1, so lay it out with `.aspectRatio(contentMode: .fit)`.
struct TorroHorns: Shape {
    /// Edge length of the source viewBox the path coordinates below are in.
    static let sourceViewBox: CGFloat = 150

    /// Content bounding box of the horns inside the source viewBox.
    ///
    /// Scaled by 10 this matches the white horns in `torro-logo-square.svg`
    /// exactly (x 111…1377, y 376…1110 in its 1500 viewBox) — the signet and
    /// the app icon are the same geometry, so `TorroLogoTile` can reproduce the
    /// official icon placement from these numbers instead of guessing padding.
    static let contentBox = CGRect(x: 11.1, y: 37.6, width: 126.6, height: 73.4)

    static let aspectRatio: CGFloat = contentBox.width / contentBox.height

    func path(in rect: CGRect) -> Path {
        let box = Self.contentBox
        // Aspect-fit the content box into the target rect.
        let scale = min(rect.width / box.width, rect.height / box.height)
        let drawn = CGSize(width: box.width * scale, height: box.height * scale)
        let originX = rect.minX + (rect.width - drawn.width) / 2
        let originY = rect.minY + (rect.height - drawn.height) / 2

        // Maps a point from the source viewBox into the target rect.
        func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
            CGPoint(x: originX + (x - box.minX) * scale,
                    y: originY + (y - box.minY) * scale)
        }

        var path = Path()

        // Left horn
        path.move(to: pt(54.0, 108.4))
        path.addCurve(to: pt(31.6, 103.3), control1: pt(44.9, 106.9), control2: pt(38.5, 105.5))
        path.addCurve(to: pt(12.3, 78.6), control1: pt(18.0, 99.0), control2: pt(11.1, 90.2))
        path.addCurve(to: pt(25.2, 56.5), control1: pt(13.1, 70.8), control2: pt(16.8, 64.4))
        path.addCurve(to: pt(50.1, 38.7), control1: pt(33.3, 48.7), control2: pt(49.0, 37.6))
        path.addCurve(to: pt(48.9, 50.0), control1: pt(50.4, 39.2), control2: pt(49.9, 44.2))
        path.addCurve(to: pt(47.8, 68.6), control1: pt(46.9, 61.3), control2: pt(46.6, 66.8))
        path.addCurve(to: pt(61.3, 72.8), control1: pt(49.3, 70.9), control2: pt(53.4, 72.2))
        path.addLine(to: pt(69.5, 73.5))
        path.addLine(to: pt(69.5, 91.0))
        path.addLine(to: pt(69.5, 108.5))
        path.addLine(to: pt(63.0, 108.6))
        path.addCurve(to: pt(54.0, 108.4), control1: pt(59.4, 108.7), control2: pt(55.4, 108.6))
        path.closeSubpath()

        // Right horn
        path.move(to: pt(79.5, 107.9))
        path.addCurve(to: pt(79.2, 90.0), control1: pt(79.2, 107.1), control2: pt(79.1, 99.0))
        path.addLine(to: pt(79.5, 73.5))
        path.addLine(to: pt(87.7, 72.8))
        path.addCurve(to: pt(101.2, 68.6), control1: pt(95.6, 72.2), control2: pt(99.7, 70.9))
        path.addCurve(to: pt(100.1, 50.0), control1: pt(102.4, 66.8), control2: pt(102.1, 61.3))
        path.addCurve(to: pt(98.9, 38.7), control1: pt(99.1, 44.2), control2: pt(98.6, 39.2))
        path.addCurve(to: pt(123.8, 56.5), control1: pt(100.0, 37.6), control2: pt(115.7, 48.7))
        path.addCurve(to: pt(133.7, 68.4), control1: pt(128.0, 60.4), control2: pt(132.3, 65.6))
        path.addCurve(to: pt(136.0, 87.5), control1: pt(136.7, 74.4), control2: pt(137.7, 82.4))
        path.addCurve(to: pt(121.2, 102.0), control1: pt(134.1, 93.4), control2: pt(127.7, 99.6))
        path.addCurve(to: pt(79.5, 107.9), control1: pt(106.9, 107.4), control2: pt(80.7, 111.0))
        path.closeSubpath()

        return path
    }
}

/// The audio-level glyph from the product app icon: five rounded vertical bars.
///
/// Converted from the 24×24 glyph baked into `Resources/Brand/torrowhisper-icon.svg`
/// (the brand repo's `logo/icon-square/products/`). It is a `Shape`, not a bundled
/// image, so it tints via `foregroundStyle` and needs no resource plumbing. The
/// glyph is the app's "function glyph" in the Torro product-icon system (design
/// guide, sections 07/08) — horns identify the family, this bar identifies Whisper.
///
/// The source is drawn as five round-capped strokes; here each stroke is a filled
/// capsule of the same width and centre so the fill reproduces the round caps
/// exactly. The 24×24 box is aspect-fitted into whatever rect it is given.
struct TorroWaveform: Shape {
    /// Edge length of the source glyph viewBox the coordinates below are in.
    static let sourceViewBox: CGFloat = 24
    /// Stroke width of the source bars, in glyph units.
    private static let strokeWidth: CGFloat = 2.2
    /// Bar centre-x and its vertical extent (y-top, y-bottom), in glyph units —
    /// taken 1:1 from the SVG's five `<line>` elements.
    private static let bars: [(x: CGFloat, top: CGFloat, bottom: CGFloat)] = [
        (4, 10, 14), (8, 7, 17), (12, 4, 20), (16, 8, 16), (20, 11, 13),
    ]

    func path(in rect: CGRect) -> Path {
        let vb = Self.sourceViewBox
        let scale = min(rect.width, rect.height) / vb
        let drawn = vb * scale
        let originX = rect.minX + (rect.width - drawn) / 2
        let originY = rect.minY + (rect.height - drawn) / 2
        let w = Self.strokeWidth * scale

        var path = Path()
        for bar in Self.bars {
            let cx = originX + bar.x * scale
            let top = originY + bar.top * scale - w / 2
            let height = (bar.bottom - bar.top) * scale + w
            let box = CGRect(x: cx - w / 2, y: top, width: w, height: height)
            path.addRoundedRect(in: box, cornerSize: CGSize(width: w / 2, height: w / 2))
        }
        return path
    }
}

/// The app-icon lockup at UI scale: the horns signet above the audio-level glyph
/// on the brand's red-gradient tile. Use where the app represents itself
/// (onboarding, About) — the guide asks for the product icon rather than a
/// shrunken wordmark at small sizes.
///
/// Signet and glyph are placed at the ratios taken from
/// `Resources/Brand/torrowhisper-icon.svg` (a 120-unit icon: horns at x35.5 y19
/// w49 h28, waveform at x37 y56 w46 h46), so this is the app icon in miniature
/// rather than a lookalike.
struct TorroLogoTile: View {
    var size: CGFloat = 44

    /// Scale from the 120-unit source icon to the requested tile size.
    private var k: CGFloat { size / 120 }

    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.2237, style: .continuous)
            .fill(
                LinearGradient(colors: [.torroRed, .torroRedDeep],
                               startPoint: .top, endPoint: .bottom)
            )
            .frame(width: size, height: size)
            .overlay {
                TorroHorns()
                    .fill(.white)
                    .frame(width: 49 * k, height: 28 * k)
                    .position(x: 60 * k, y: 33 * k)
            }
            .overlay {
                TorroWaveform()
                    .fill(.white)
                    .frame(width: 46 * k, height: 46 * k)
                    .position(x: 60 * k, y: 79 * k)
            }
            .accessibilityHidden(true)
    }
}
