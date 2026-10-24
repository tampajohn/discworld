// Package existing game artwork as opaque PNGs for Apple's app-icon catalog.
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

let sourceURL = URL(fileURLWithPath: CommandLine.arguments[1])
let outputURL = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
guard let source = CGImageSourceCreateWithURL(sourceURL as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
    fatalError("Cannot decode app-icon source")
}
for size in Set(CommandLine.arguments.dropFirst(3).compactMap(Int.init)) {
    guard let context = CGContext(data: nil, width: size, height: size,
        bitsPerComponent: 8, bytesPerRow: size * 4,
        space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
        fatalError("Cannot allocate app-icon raster")
    }
    context.setFillColor(CGColor(gray: 0, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: size, height: size))
    // Fit the complete artwork, without cutting off the logo or adding a mask.
    let scale = min(Double(size) / Double(image.width), Double(size) / Double(image.height))
    let width = Double(image.width) * scale, height = Double(image.height) * scale
    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: (Double(size) - width) / 2,
        y: (Double(size) - height) / 2, width: width, height: height))
    let url = outputURL.appendingPathComponent("icon-\(size).png")
    guard let raster = context.makeImage(),
          let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        fatalError("Cannot create app-icon PNG")
    }
    CGImageDestinationAddImage(destination, raster, nil)
    guard CGImageDestinationFinalize(destination) else { fatalError("Cannot write app-icon PNG") }
}
