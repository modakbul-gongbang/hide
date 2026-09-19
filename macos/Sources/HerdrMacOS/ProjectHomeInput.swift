import AppKit
import SwiftUI

/// The pointer on the map, answered in AppKit because SwiftUI has no scroll
/// wheel, no magnification and no hover for a window that is not key.
///
/// The view is transparent and sits over the canvas; it turns raw events into
/// the four things the map understands - a hover point, a click, a pan and
/// a zoom - and hands each to a closure with canvas-local points, top-left
/// origin. It decides nothing about the map itself.
struct ProjectHomeInput: NSViewRepresentable {
    var onHover: (CGPoint?) -> Void
    var onClick: (CGPoint, Int) -> Void
    var onPan: (CGSize) -> Void
    /// A zoom factor and the canvas point to keep still.
    var onZoom: (CGFloat, CGPoint) -> Void

    func makeNSView(context: Context) -> ProjectHomeInputView {
        let view = ProjectHomeInputView()
        update(view)
        return view
    }

    func updateNSView(_ view: ProjectHomeInputView, context: Context) {
        update(view)
    }

    private func update(_ view: ProjectHomeInputView) {
        view.onHover = onHover
        view.onClick = onClick
        view.onPan = onPan
        view.onZoom = onZoom
    }
}

/// `PaneWheelOwner`: Home is an overlay above the pane content, and the window
/// would otherwise hand the wheel to the terminal underneath.
final class ProjectHomeInputView: NSView, PaneWheelOwner {
    var onHover: ((CGPoint?) -> Void)?
    var onClick: ((CGPoint, Int) -> Void)?
    var onPan: ((CGSize) -> Void)?
    var onZoom: ((CGFloat, CGPoint) -> Void)?

    private var pressPoint: CGPoint?
    private var lastDragPoint: CGPoint?
    private var dragging = false
    private var trackingArea: NSTrackingArea?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { false }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let trackingArea { removeTrackingArea(trackingArea) }
        // `activeAlways` so the map answers the pointer in a window that is
        // not key, the same as the terminal's link hover.
        let area = NSTrackingArea(
            rect: .zero,
            options: [.mouseMoved, .mouseEnteredAndExited, .activeAlways, .inVisibleRect],
            owner: self, userInfo: nil
        )
        addTrackingArea(area)
        trackingArea = area
    }

    override func mouseMoved(with event: NSEvent) {
        onHover?(convert(event.locationInWindow, from: nil))
    }

    override func mouseExited(with event: NSEvent) {
        onHover?(nil)
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        pressPoint = point
        lastDragPoint = point
        dragging = false
    }

    override func mouseDragged(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard let pressPoint, let lastDragPoint else { return }
        if !dragging, hypot(point.x - pressPoint.x, point.y - pressPoint.y) < HideTheme.Home.dragThreshold { return }
        dragging = true
        onPan?(CGSize(width: point.x - lastDragPoint.x, height: point.y - lastDragPoint.y))
        self.lastDragPoint = point
    }

    override func mouseUp(with event: NSEvent) {
        defer { pressPoint = nil; lastDragPoint = nil; dragging = false }
        guard !dragging else { return }
        onClick?(convert(event.locationInWindow, from: nil), event.clickCount)
    }

    override func scrollWheel(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        if event.modifierFlags.contains(.command) {
            // ⌘-scroll zooms about the pointer, a line at a time on a wheel
            // and by the precise delta on a trackpad.
            let units = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * HideTheme.Home.wheelLineHeight
            onZoom?(1 + units * HideTheme.Home.zoomPerScrollUnit, point)
        } else {
            let factor: CGFloat = event.hasPreciseScrollingDeltas ? 1 : HideTheme.Home.wheelLineHeight
            onPan?(CGSize(width: event.scrollingDeltaX * factor, height: event.scrollingDeltaY * factor))
        }
    }

    override func magnify(with event: NSEvent) {
        onZoom?(1 + event.magnification, convert(event.locationInWindow, from: nil))
    }
}
