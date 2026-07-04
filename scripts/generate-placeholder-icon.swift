#!/usr/bin/env swift
// Generates a placeholder AppIcon.icns for DonnyWhisper.
// Run from the repo root: `swift scripts/generate-placeholder-icon.swift`.
// Replace the resulting icns with a real brand icon when design is ready.
//
// Uses Core Graphics + Core Text only — no AppKit lockFocus — so it runs
// reliably in plain `swift` interpreter mode without an NSApplication.

import AppKit
import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers

let fm = FileManager.default
let cwd = fm.currentDirectoryPath
let resourcesDir = "\(cwd)/apps/donnywhisper-macos/Resources"
let iconsetDir = "\(resourcesDir)/AppIcon.iconset"
let icnsPath = "\(resourcesDir)/AppIcon.icns"

guard fm.fileExists(atPath: resourcesDir) else {
    FileHandle.standardError.write(Data("Run from repo root — \(resourcesDir) not found\n".utf8))
    exit(1)
}

try? fm.removeItem(atPath: iconsetDir)
try fm.createDirectory(atPath: iconsetDir, withIntermediateDirectories: true)

let sizes: [(name: String, pixels: Int)] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]

func renderIcon(pixels: Int) -> Data {
    let s = CGFloat(pixels)
    let colorSpace = CGColorSpaceCreateDeviceRGB()
    guard let ctx = CGContext(
        data: nil,
        width: pixels,
        height: pixels,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: colorSpace,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        fatalError("CGContext creation failed for \(pixels)px")
    }

    let rect = CGRect(x: 0, y: 0, width: s, height: s)

    // Rounded-rect clip (macOS Big Sur+ squircle ratio ≈ 0.2237)
    let cornerRadius = s * 0.2237
    let clipPath = CGPath(roundedRect: rect,
                          cornerWidth: cornerRadius,
                          cornerHeight: cornerRadius,
                          transform: nil)
    ctx.addPath(clipPath)
    ctx.clip()

    // Diagonal blue→violet gradient
    let colors = [
        CGColor(srgbRed: 0.27, green: 0.36, blue: 0.96, alpha: 1.0),
        CGColor(srgbRed: 0.55, green: 0.30, blue: 0.93, alpha: 1.0),
    ] as CFArray
    guard let gradient = CGGradient(colorsSpace: colorSpace,
                                    colors: colors,
                                    locations: [0.0, 1.0]) else {
        fatalError("CGGradient creation failed")
    }
    ctx.drawLinearGradient(gradient,
                           start: CGPoint(x: 0, y: s),
                           end: CGPoint(x: s, y: 0),
                           options: [])

    // "OW" monogram centered
    let monogram = "OW"
    let fontSize = s * 0.38
    let font = CTFontCreateWithName("SFProRounded-Bold" as CFString, fontSize, nil)
    let attrs: [NSAttributedString.Key: Any] = [
        .font: font,
        .foregroundColor: CGColor(gray: 1.0, alpha: 1.0),
        .kern: -fontSize * 0.04,
    ]
    let attributed = NSAttributedString(string: monogram, attributes: attrs)
    let line = CTLineCreateWithAttributedString(attributed)
    let bounds = CTLineGetBoundsWithOptions(line, .useOpticalBounds)

    let textX = (s - bounds.width) / 2 - bounds.origin.x
    let textY = (s - bounds.height) / 2 - bounds.origin.y
    ctx.textPosition = CGPoint(x: textX, y: textY)
    CTLineDraw(line, ctx)

    guard let cgImage = ctx.makeImage() else {
        fatalError("makeImage failed for \(pixels)px")
    }

    let mutableData = CFDataCreateMutable(nil, 0)!
    guard let dest = CGImageDestinationCreateWithData(mutableData,
                                                       UTType.png.identifier as CFString,
                                                       1, nil) else {
        fatalError("CGImageDestination creation failed")
    }
    CGImageDestinationAddImage(dest, cgImage, nil)
    guard CGImageDestinationFinalize(dest) else {
        fatalError("CGImageDestinationFinalize failed")
    }
    return mutableData as Data
}

for (name, pixels) in sizes {
    let data = renderIcon(pixels: pixels)
    let outURL = URL(fileURLWithPath: "\(iconsetDir)/\(name)")
    try data.write(to: outURL)
    print("  \(name) (\(pixels)px)")
}

let process = Process()
process.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
process.arguments = ["-c", "icns", "-o", icnsPath, iconsetDir]
try process.run()
process.waitUntilExit()

if process.terminationStatus != 0 {
    FileHandle.standardError.write(Data("iconutil failed with status \(process.terminationStatus)\n".utf8))
    exit(1)
}

try? fm.removeItem(atPath: iconsetDir)
print("Wrote \(icnsPath)")
