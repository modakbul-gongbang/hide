import AppKit
import Testing

@testable import SwiftTerm

@MainActor
@Suite("SwiftTerm IME overlay rendering")
struct SwiftTermImeOverlayTests {
    @Test func shortCompositionDoesNotPaintTheUnusedRestOfTheTerminalRow() throws {
        let size = NSSize(width: 240, height: 28)
        let overlay = DictationOverlayTextView(frame: NSRect(origin: .zero, size: size))
        overlay.drawsBackground = false
        overlay.textContainerInset = .zero
        overlay.textContainer?.lineFragmentPadding = 0
        overlay.textContainer?.containerSize = size
        overlay.overlayBackgroundColor = .systemRed
        overlay.textStorage?.setAttributedString(NSAttributedString(
            string: "한글",
            attributes: [
                .font: NSFont.monospacedSystemFont(ofSize: 14, weight: .regular),
                .foregroundColor: NSColor.black,
            ]
        ))
        if let container = overlay.textContainer {
            overlay.layoutManager?.ensureLayout(for: container)
        }

        let bitmap = try #require(overlay.bitmapImageRepForCachingDisplay(in: overlay.bounds))
        overlay.cacheDisplay(in: overlay.bounds, to: bitmap)

        let paintedCompositionBackground = (0..<(bitmap.pixelsWide / 3)).contains { x in
            (0..<bitmap.pixelsHigh).contains { y in
                guard let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB) else {
                    return false
                }
                return color.redComponent > 0.7
                    && color.greenComponent < 0.4
                    && color.blueComponent < 0.4
                    && color.alphaComponent > 0.5
            }
        }
        let unusedRowPixel = try #require(bitmap.colorAt(
            x: Int(Double(bitmap.pixelsWide) * 0.9),
            y: bitmap.pixelsHigh / 2
        )?.usingColorSpace(.deviceRGB))
        #expect(paintedCompositionBackground)
        #expect(unusedRowPixel.alphaComponent < 0.05)
    }
}
