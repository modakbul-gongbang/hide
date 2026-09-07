import AppKit
import Testing
@testable import SwiftTerm

@Suite("Terminal repaint correctness")
@MainActor
struct TerminalRepaintTests {
    @Test func changingRowsDoNotRetainPastScreens() throws {
        let bounds = NSRect(x: 0, y: 0, width: 480, height: 240)
        let window = NSWindow(contentRect: bounds, styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let view = TerminalView(frame: bounds)
        view.bidiHostPolicy = .legacyLeftToRight
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
        for frame in 0..<100 {
            view.feed(text: "\u{1b}[HFRAME \(frame)\u{1b}[K")
            view.draw(bounds)
        }
        // Resource ownership is the contract here, not a timing threshold:
        // a renderer retains the current viewport, never a history of its
        // prepared screens that it must later destroy in one display pass.
        #expect(view.preparedRowCache.count <= view.getTerminal().rows + 1)
    }

    // AppKit can invalidate a layer again between display-link ticks, for
    // example while selecting text. Every requested backing-store repaint
    // must contain the terminal, not a transparent/black replacement frame.
    @Test func repeatedBackingStoreRepaintsPreserveText() throws {
        let bounds = NSRect(x: 0, y: 0, width: 480, height: 240)
        let window = NSWindow(contentRect: bounds, styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let view = TerminalView(frame: bounds)
        view.bidiHostPolicy = .legacyLeftToRight
        window.contentView = view
        defer { window.close() }
        view.feed(text: "REPAINT MUST PRESERVE THIS TEXT\r\n선택 중에도 화면 유지")

        func paint() throws -> Data {
            let bitmap = try #require(NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: 480, pixelsHigh: 240,
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
            ))
            let bytes = try #require(bitmap.bitmapData)
            bytes.update(repeating: 0, count: bitmap.bytesPerRow * bitmap.pixelsHigh)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
            view.draw(bounds)
            NSGraphicsContext.restoreGraphicsState()
            return Data(bytes: bytes, count: bitmap.bytesPerRow * bitmap.pixelsHigh)
        }

        let first = try paint()
        #expect(first.contains { $0 != 0 })
        for _ in 0..<3 {
            let repaint = try paint()
            #expect(repaint.elementsEqual(first), "A second AppKit draw must not publish an empty backing store")
        }
        view.selection.startSelection(row: 0, col: 0)
        view.selection.dragExtend(row: 0, col: 10)
        let selected = try paint()
        #expect(!selected.elementsEqual(first), "Selection must update without waiting for terminal output")
        #expect(try paint().elementsEqual(selected))
        view.selection.selectNone()
        view.feed(text: "\u{1b}[HNEW INPUT")
        let typed = try paint()
        #expect(!typed.elementsEqual(first), "A changed row must replace its cached glyphs")
        #expect(try paint().elementsEqual(typed))
    }
}
