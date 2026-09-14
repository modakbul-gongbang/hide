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

@MainActor
@Suite("SwiftTerm IME overlay cost")
struct SwiftTermImeOverlayCostTests {
    private func composing(_ text: String, in view: TerminalView) {
        view.setMarkedText(
            NSAttributedString(string: text),
            selectedRange: NSRange(location: text.utf16.count, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
    }

    @Test func everyCompositionStepSharesOneAttributeSet() throws {
        let view = TerminalView(frame: NSRect(x: 0, y: 0, width: 480, height: 240), font: nil)
        composing("ㅎ", in: view)
        let first = try #require(view.markedTextOverlay?.textStorage?.attributes(at: 0, effectiveRange: nil))
        composing("한", in: view)
        let second = try #require(view.markedTextOverlay?.textStorage?.attributes(at: 0, effectiveRange: nil))
        #expect(view.markedTextOverlay?.string == "한")
        let firstParagraph = try #require(first[.paragraphStyle] as? NSParagraphStyle)
        let secondParagraph = try #require(second[.paragraphStyle] as? NSParagraphStyle)
        #expect(firstParagraph === secondParagraph, "a fresh paragraph style per step made every attribute dictionary a new table entry")
        #expect((first[.font] as? NSFont) === (second[.font] as? NSFont))
    }

    @Test func anchorRefreshRebuildsOnlyAfterTheCaretMoved() throws {
        let view = TerminalView(frame: NSRect(x: 0, y: 0, width: 480, height: 240), font: nil)
        composing("한", in: view)
        let overlay = try #require(view.markedTextOverlay)
        let placed = overlay.frame
        view.refreshMarkedTextOverlayAnchor()
        #expect(view.markedTextOverlay === overlay)
        #expect(overlay.frame == placed, "an unmoved caret must not lay the overlay out again")
        // The caret advances on the display pass after the PTY echo lands,
        // so the test moves it the way that pass does.
        view.caretView.frame.origin.y -= view.cellDimension.height
        view.refreshMarkedTextOverlayAnchor()
        #expect(overlay.frame.origin.y == placed.origin.y - view.cellDimension.height, "a moved caret re-anchors the overlay")
        #expect(overlay.string == "한")
    }
}
