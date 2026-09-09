import AppKit
import Combine
import SwiftTerm
import Testing
@testable import HerdrMacOS

@Suite("Terminal viewport signal", .serialized)
@MainActor
struct TerminalViewportSignalTests {
    @Test func historyCollapsesOutputKeepsItCollapsedAndReturnFocusesInput() async throws {
        let terminal = ImeTerminalView(frame: NSRect(x: 0, y: 0, width: 640, height: 320))
        let window = NSWindow(contentRect: terminal.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = terminal
        // This offscreen AppKit fixture never orders a window or launches an app.
        let signal = TerminalViewportSignal()
        var publications = 0
        let subscription = signal.objectWillChange.sink { publications += 1 }
        defer { subscription.cancel(); window.contentView = nil }
        terminal.feed(text: (0..<200).map { "row \($0)\r\n" }.joined())
        signal.observe(terminal)
        await settle()
        #expect(signal.followingBottom)
        terminal.scroll(toPosition: 0)
        signal.observe(terminal)
        #expect(!signal.currentFollowingBottom, "send guard sees scroll before view publication")
        await settle()
        #expect(!signal.followingBottom)
        let collapsedPublications = publications
        for index in 0..<200 {
            terminal.feed(text: "output \(index)\r\n")
            signal.observe(terminal)
        }
        await settle()
        #expect(!signal.followingBottom)
        #expect(publications == collapsedPublications, "retained output must not wake the shelf at every frame")
        #expect(signal.returnToBottomAndFocus())
        await settle()
        #expect(signal.followingBottom)
        #expect(window.firstResponder === terminal)
        for _ in 0..<20_000 { signal.observe(terminal) }
        await settle()
        #expect(publications == collapsedPublications + 1)
        terminal.feed(text: "\u{1b}[?1049h")
        signal.observe(terminal)
        await settle()
        #expect(signal.followingBottom, "alternate buffer without local history is reachable")
    }

    private func settle() async { try? await Task.sleep(for: .milliseconds(10)) }
}
