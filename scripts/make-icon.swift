// Draws AppIcon.icns (macOS), icon.ico (Windows) and icon.png (README, Linux):
//   swift scripts/make-icon.swift assets
import AppKit

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "assets"
let iconset = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("RackIP.iconset")
try? FileManager.default.removeItem(at: iconset)
try! FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

func render(_ size: Int) -> Data {
    let s = CGFloat(size)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)

    let inset = s * 0.1
    let rect = NSRect(x: inset, y: inset, width: s - inset * 2, height: s - inset * 2)
    let tile = NSBezierPath(roundedRect: rect, xRadius: rect.width * 0.225, yRadius: rect.width * 0.225)
    NSGradient(colors: [NSColor(red: 0.07, green: 0.38, blue: 0.33, alpha: 1),
                        NSColor(red: 0.04, green: 0.16, blue: 0.20, alpha: 1)])!
        .draw(in: tile, angle: -90)

    // An RJ45 jack seen from the front, coordinates as fractions of the icon.
    func r(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) -> NSRect {
        NSRect(x: x * s, y: y * s, width: w * s, height: h * s)
    }
    func rounded(_ rect: NSRect, _ radius: CGFloat) -> NSBezierPath {
        NSBezierPath(roundedRect: rect, xRadius: radius * s, yRadius: radius * s)
    }

    // Metal housing
    NSGradient(colors: [NSColor(white: 0.96, alpha: 1), NSColor(white: 0.72, alpha: 1)])!
        .draw(in: rounded(r(0.21, 0.23, 0.58, 0.54), 0.05), angle: -90)

    // Opening: wide plug cavity with the latch slot below it
    let cavity = rounded(r(0.28, 0.40, 0.44, 0.30), 0.02)
    cavity.append(rounded(r(0.40, 0.29, 0.20, 0.14), 0.015))
    NSColor(red: 0.05, green: 0.08, blue: 0.10, alpha: 1).setFill()
    cavity.fill()

    // Eight contacts
    NSColor(red: 0.93, green: 0.72, blue: 0.28, alpha: 1).setFill()
    for i in 0..<8 {
        rounded(r(0.315 + CGFloat(i) * 0.0485, 0.56, 0.022, 0.12), 0.006).fill()
    }

    // Link and activity LEDs
    NSColor(red: 0.20, green: 0.85, blue: 0.40, alpha: 1).setFill()
    rounded(r(0.26, 0.285, 0.09, 0.045), 0.01).fill()
    NSColor(red: 1.00, green: 0.66, blue: 0.10, alpha: 1).setFill()
    rounded(r(0.65, 0.285, 0.09, 0.045), 0.01).fill()

    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for base in [16, 32, 128, 256, 512] {
    try! render(base).write(to: iconset.appendingPathComponent("icon_\(base)x\(base).png"))
    try! render(base * 2).write(to: iconset.appendingPathComponent("icon_\(base)x\(base)@2x.png"))
}

let p = Process()
p.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
p.arguments = ["-c", "icns", iconset.path, "-o", "\(outDir)/AppIcon.icns"]
try! p.run(); p.waitUntilExit()

try! render(512).write(to: URL(fileURLWithPath: "\(outDir)/icon.png"))

// ICO with PNG-compressed entries (supported since Windows Vista).
let icoSizes = [16, 24, 32, 48, 64, 128, 256]
let images = icoSizes.map(render)
var ico = Data([0, 0, 1, 0, UInt8(icoSizes.count), 0])
var offset = 6 + 16 * icoSizes.count
func le16(_ v: Int) -> [UInt8] { [UInt8(v & 0xFF), UInt8(v >> 8 & 0xFF)] }
func le32(_ v: Int) -> [UInt8] { le16(v & 0xFFFF) + le16(v >> 16) }
for (size, png) in zip(icoSizes, images) {
    let dim = UInt8(size == 256 ? 0 : size)
    ico.append(contentsOf: [dim, dim, 0, 0] + le16(1) + le16(32) + le32(png.count) + le32(offset))
    offset += png.count
}
images.forEach { ico.append($0) }
try! ico.write(to: URL(fileURLWithPath: "\(outDir)/icon.ico"))

print("Wrote \(outDir)/AppIcon.icns, icon.ico, icon.png")
