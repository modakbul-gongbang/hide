import AppKit
import SwiftUI

/// The main window's chrome.
///
/// Hide draws its own first row, so the system titlebar is hidden and the
/// content reaches the very top of the window. The title string stays: Mission
/// Control, the accessibility tree, and the pet verification receipt all read
/// it, and none of them read pixels.
@MainActor
enum MainWindowChrome {
    static let title = "hide"

    /// Hides the titlebar and hangs the content where it used to be.
    ///
    /// Clearing the safe area is the half that is easy to miss. A window with
    /// `fullSizeContentView` reports a 28pt top safe area, and SwiftUI insets
    /// its content by that much, which leaves a band of empty window
    /// background above the first row.
    ///
    /// The order matters and is why the hosting view is built here rather than
    /// handed in: clearing `safeAreaRegions` only takes once the view has not
    /// yet joined a window. Set afterwards, the assignment is accepted and
    /// changes nothing, and the band comes back.
    @discardableResult
    static func apply<Content: View>(
        to window: NSWindow,
        content: Content
    ) -> NSHostingView<Content> {
        window.title = title
        window.styleMask.insert(.fullSizeContentView)
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.titlebarSeparatorStyle = .none
        let hosting = NSHostingView(rootView: content)
        hosting.safeAreaRegions = []
        window.contentView = hosting
        return hosting
    }
}

extension View {
    /// Says that this column draws all the way to the window's top edge.
    ///
    /// Clearing the hosting view's safe area covers the root, but the three
    /// columns sit in an `HSplitView`, and the `NSSplitView` behind it hands
    /// each of its children the window's safe area again. Without this every
    /// column would start 28pt down while its background still filled to the
    /// top, which reads as a stripe of empty chrome above the first row.
    func drawsToWindowTopEdge() -> some View {
        ignoresSafeArea(.container, edges: .top)
    }
}

/// The window's own handle.
///
/// With the system titlebar hidden there is no standard place to grab the
/// window, so the empty area of each surface that reaches the window's top
/// edge does that job. A press that starts on a tab is a reorder and never
/// arrives here, which is what keeps the two gestures from contending for the
/// same point.
///
/// The gesture is handed to AppKit rather than tracked here, so a window move
/// stays indistinguishable from a titlebar drag: it snaps to screen edges and
/// carries between spaces.
struct WindowDragArea: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        WindowDragBackingView()
    }

    func updateNSView(_ view: NSView, context: Context) {}
}

private final class WindowDragBackingView: NSView {
    /// A drag on an inactive window moves it rather than only raising it,
    /// which is what the titlebar this replaces did.
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        true
    }

    override func mouseDown(with event: NSEvent) {
        guard let window else {
            super.mouseDown(with: event)
            return
        }
        window.performDrag(with: event)
    }
}
