import AppKit
import SwiftUI
import Testing
@testable import HerdrMacOS

@Suite("Sidebar native scrolling")
@MainActor
struct SidebarListTests {
    @Test func scrollingUsesNativeRowsAndKeepsTheWheelAfterMoving() throws {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 400),
                              styleMask: [.titled], backing: .buffered, defer: false)
        let host = NSHostingView(rootView: SidebarList {
            ForEach(0..<100) { index in
                Button("Project \(index)") {}.buttonStyle(.plain).frame(height: 32)
            }
        })
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        // SwiftUI installs the native document on the next layout transaction.
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        host.layoutSubtreeIfNeeded()
        // This native boundary is intentional: a ScrollView/LazyVStack traverses
        // SwiftUI row responders on every system cursor hit test. A table owns
        // the scroll target and indexes rows without that whole-document walk.
        let table = try #require(descendants(host).compactMap { $0 as? NSTableView }.first)
        let scroll = try #require(table.enclosingScrollView)
        #expect(table.numberOfRows >= 100)
        let initial = scroll.contentView.bounds.origin.y
        scroll.contentView.scroll(to: NSPoint(x: 0, y: 600))
        scroll.reflectScrolledClipView(scroll.contentView)
        #expect(scroll.contentView.bounds.origin.y > initial)
        var target = host.hitTest(NSPoint(x: 150, y: 200))
        while let view = target, view !== scroll { target = view.superview }
        #expect(target === scroll)
        #expect(!table.visibleRect.isEmpty)
    }

    private func descendants(_ view: NSView) -> [NSView] {
        view.subviews.flatMap { [$0] + descendants($0) }
    }
}
