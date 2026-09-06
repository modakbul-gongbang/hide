#if os(macOS)
import AppKit
import QuartzCore

/// AppKit may ask for display more than once between refreshes. Damage left
/// after the first draw is carried to the next display tick.
struct TerminalFrameGate {
    private var frame: UInt64 = 0
    private var lastDraw: UInt64?

    mutating func tick() { frame &+= 1 }
    mutating func draw(visible: Bool) -> Bool {
        guard visible, lastDraw != frame else { return false }
        lastDraw = frame
        return true
    }
}

/// The display link retains its target; the target must not retain the view.
///
/// The view's own display link (macOS 14) pauses while the view is hidden or
/// its window is occluded, so a pane nobody can see costs no display pass.
final class TerminalDisplayClock: NSObject {
    weak var view: TerminalView?
    private var link: CADisplayLink?

    init(view: TerminalView) {
        self.view = view
        super.init()
        let link = view.displayLink(target: self, selector: #selector(tick(_:)))
        self.link = link
        link.add(to: .main, forMode: .common)
    }

    @objc private func tick(_ link: CADisplayLink) {
        view?.displayFrame(period: link.targetTimestamp - link.timestamp)
    }

    func invalidate() {
        link?.invalidate()
        link = nil
    }
}
#endif
