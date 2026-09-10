import AppKit
import SwiftUI
import Testing
@testable import HerdrMacOS
@testable import SwiftTerm

@Suite("Terminal command-click contract", .serialized)
@MainActor
struct TerminalLinkActivationTests {
    @Test func linksRequireCommandWhileOrdinaryClicksAndDragsKeepTheirRoute() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("hide-link-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        var opened: [String] = []
        var clicks = 0
        let host = NSHostingView(rootView: TerminalHost(bridge: bridge, paneID: "link-fixture", textScale: 1,
            onFocus: {}, onOpenLink: { opened.append($0) }))
        host.frame = NSRect(x: 0, y: 0, width: 640, height: 320)
        host.layoutSubtreeIfNeeded()
        func terminal(in view: NSView) -> ImeTerminalView? {
            (view as? ImeTerminalView) ?? view.subviews.lazy.compactMap { terminal(in: $0) }.first
        }
        let view = try #require(terminal(in: host))
        view.onOrdinaryClick = { _, _, _ in clicks += 1 }
        view.feed(text: "https://example.com\r\n\u{1b}]8;;https://example.org\u{7}explicit\u{1b}]8;;\u{7}")
        func event(_ type: NSEvent.EventType, row: Int = 0, col: Int = 2,
                   modifiers: NSEvent.ModifierFlags = []) -> NSEvent {
            let point = NSPoint(x: CGFloat(col) * view.cellDimension.width + 1,
                y: view.bounds.height - (CGFloat(row) + 0.5) * view.cellDimension.height)
            return NSEvent.mouseEvent(with: type, location: view.convert(point, to: nil), modifierFlags: modifiers,
                timestamp: 0, windowNumber: 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
        }
        for row in [0, 1] {
            view.mouseMoved(with: event(.mouseMoved, row: row))
            #expect(view.linkHighlightRange == nil, "Unmodified hover must not advertise activation")
            view.mouseDown(with: event(.leftMouseDown, row: row))
            view.mouseUp(with: event(.leftMouseUp, row: row))
        }
        #expect(opened.isEmpty)
        #expect(clicks == 2)
        for row in [0, 1] {
            view.mouseDown(with: event(.leftMouseDown, row: row, modifiers: .command))
            view.mouseUp(with: event(.leftMouseUp, row: row, modifiers: .command))
        }
        #expect(opened == ["https://example.com", "https://example.org"])
        #expect(clicks == 2, "An activated link cannot replay into the TUI")
        view.mouseDown(with: event(.leftMouseDown))
        view.mouseDragged(with: event(.leftMouseDragged, col: 8))
        view.mouseUp(with: event(.leftMouseUp, col: 8))
        #expect(view.getSelection() != nil)
        #expect(clicks == 2)
        #expect(opened.count == 2)
        view.mouseDown(with: event(.leftMouseDown, modifiers: .option))
        #expect(view.pointerRouting.pressRoute == .application)
        view.mouseDragged(with: event(.leftMouseDragged, col: 8, modifiers: .option))
        view.mouseUp(with: event(.leftMouseUp, col: 8, modifiers: .option))
        #expect(clicks == 2)
        #expect(opened.count == 2)
    }
}
