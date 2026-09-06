import AppKit
import Testing
@testable import SwiftTerm

@Suite("Terminal display frames")
@MainActor
struct TerminalFrameGateTests {
    @Test func streamingFramesPaintOnceAndHiddenFramesAreStillParsed() throws {
        let bounds = NSRect(x: 0, y: 0, width: 480, height: 240)
        let window = NSWindow(contentRect: bounds, styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let view = TerminalView(frame: bounds)
        window.contentView = view
        defer { window.close() }
        let bitmap = try #require(NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: 480, pixelsHigh: 240,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ))
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
        defer { NSGraphicsContext.restoreGraphicsState() }
        var draws = 0
        view.terminalContentsDidDraw = { draws += 1 }
        for frame in 0..<120 {
            view.isHidden = frame >= 60
            for chunk in 0..<20 {
                view.feed(text: "\u{1b}[HFRAME \(frame) CHUNK \(chunk)")
            }
            let before = draws
            view.displayFrame(period: 1.0 / 120)
            for _ in 0..<20 { view.draw(bounds) }
            #expect(draws - before == (frame < 60 ? 1 : 0))
        }
        #expect(draws == 60)
        #expect(view.getTerminal().getLine(row: 0)?.translateToString().contains("FRAME 119 CHUNK 19") == true)
        view.isHidden = false
        view.displayFrame(period: 1.0 / 120)
        view.draw(bounds)
        #expect(draws == 61)
    }
}
