import AppKit
// ArtCraft geometry, independently applied to the approved original master.
let input = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
let canvas = 1024
let body = CGFloat(824)
let inset = CGFloat(100)
let radius = body * 112 / 512
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: canvas, pixelsHigh: canvas,
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
if let pixels = rep.bitmapData { pixels.initialize(repeating: 0, count: rep.bytesPerRow * canvas) }
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
let rect = NSRect(x: inset, y: inset, width: body, height: body)
NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).addClip()
NSImage(contentsOf: input)!.draw(in: rect, from: .zero, operation: .copy, fraction: 1)
NSGraphicsContext.restoreGraphicsState()
try rep.representation(using: .png, properties: [:])!.write(to: output)
