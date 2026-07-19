#!/usr/bin/env swift
// Generates the TorroWhisper AppIcon.icns from the brand vector.
// Run from the repo root: `swift scripts/generate-app-icon.swift`.
//
// The source of truth is apps/torrowhisper-macos/Resources/Brand/torrowhisper-icon.svg,
// mirrored from the mahype/torro-design repo (logo/icon-square/products/). It is
// the product-family icon (design guide, sections 07/08): the white horns signet
// on top, the white audio-level glyph below, both on the Torro-Red gradient
// #D50C0C→#A50A0A. The SVG is rendered as-is — no recoloring, no shadow (see
// AGENTS.md "Design & Marke"). The only thing this script adds is the macOS
// squircle mask, because the brand asset is a hard square and macOS expects the
// rounded tile.
//
// NSImage reads SVG natively (macOS 11+), so this needs no external tooling.

import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let fm = FileManager.default
let cwd = fm.currentDirectoryPath
let resourcesDir = "\(cwd)/apps/torrowhisper-macos/Resources"
let brandSVG = "\(resourcesDir)/Brand/torrowhisper-icon.svg"
let iconsetDir = "\(resourcesDir)/AppIcon.iconset"
let icnsPath = "\(resourcesDir)/AppIcon.icns"

guard fm.fileExists(atPath: brandSVG) else {
    FileHandle.standardError.write(Data("Run from repo root — \(brandSVG) not found\n".utf8))
    exit(1)
}

guard let logo = NSImage(contentsOfFile: brandSVG) else {
    FileHandle.standardError.write(Data("Could not load \(brandSVG)\n".utf8))
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

    // macOS Big Sur+ squircle ratio ≈ 0.2237 of the tile edge.
    let cornerRadius = s * 0.2237
    ctx.addPath(CGPath(roundedRect: rect,
                       cornerWidth: cornerRadius,
                       cornerHeight: cornerRadius,
                       transform: nil))
    ctx.clip()

    // Draw the brand square full-bleed; the clip above rounds it off.
    let nsCtx = NSGraphicsContext(cgContext: ctx, flipped: false)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = nsCtx
    logo.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
    NSGraphicsContext.restoreGraphicsState()

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
    try data.write(to: URL(fileURLWithPath: "\(iconsetDir)/\(name)"))
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
