import AppKit
import SwiftUI
import Testing

@testable import HerdrMacOS

/// Stands in for the shell: a first row of a known height, then the rest.
/// Where that first row lands is what says whether the titlebar band is gone.
private struct ChromeProbeContent: View {
    static let firstRowHeight: CGFloat = 32

    var body: some View {
        VStack(spacing: 0) {
            Color.red.frame(height: Self.firstRowHeight)
            Color.blue
        }
    }
}

/// The window has no system titlebar and its content reaches the top edge,
/// while the title string it reports stays "hide".
@Suite("Main window chrome")
@MainActor
struct MainWindowChromeTests {
    private func chromedWindow() -> (NSWindow, NSHostingView<ChromeProbeContent>) {
        let window = NSWindow(
            contentRect: CGRect(x: 0, y: 0, width: 900, height: 600),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        let hosting = MainWindowChrome.apply(to: window, content: ChromeProbeContent())
        return (window, hosting)
    }

    @Test func theTitlebarIsHiddenAndTheTitleStringSurvives() {
        let (window, _) = chromedWindow()

        #expect(window.styleMask.contains(.fullSizeContentView))
        #expect(window.titlebarAppearsTransparent)
        #expect(window.titleVisibility == .hidden)
        #expect(window.titlebarSeparatorStyle == .none)
        // Mission Control, the accessibility tree, and the pet verification
        // receipt read this; none of them read pixels.
        #expect(window.title == "hide")
    }

    @Test func theContentFillsTheWindowToItsTopEdge() {
        let (window, hosting) = chromedWindow()

        #expect(window.contentView === hosting)
        // AppKit computes this from the style mask, so it is the window
        // agreeing that no band is reserved for a titlebar.
        #expect(window.contentRect(forFrameRect: window.frame) == window.frame)
        // The window still reports a titlebar-sized layout rect; clearing the
        // hosting view's safe area is what stops SwiftUI from insetting the
        // first row by it.
        #expect(hosting.safeAreaRegions.isEmpty)
        #expect(hosting.frame.height == window.frame.height)
        // The one that catches a real regression: clearing the safe area
        // after the view joins the window is accepted and does nothing, and
        // the first row would start 28pt down with no other symptom.
        hosting.layoutSubtreeIfNeeded()
        let firstRow = hosting.subviews.first?.frame ?? .zero
        #expect(firstRow.origin.y == 0)
        #expect(firstRow.height == ChromeProbeContent.firstRowHeight)
    }
}
