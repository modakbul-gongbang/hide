import AppKit
import SwiftTerm

/// Owns one terminal pointer gesture from press through release.
///
/// Hide deliberately reverses the usual terminal convention at the user's
/// request: an ordinary drag always selects locally, while Option+drag is the
/// explicit escape hatch that lets a mouse-aware TUI own the gesture. A single
/// click is delayed until mouse-up so the first drag event can claim the whole
/// gesture without leaking a press into the TUI.
final class TerminalPointerRoutingState {
    enum PressRoute {
        case deferredClick
        case localSelection
        case application
    }

    private(set) var pressRoute: PressRoute?
    private(set) var pressEvent: NSEvent?
    private(set) var didDrag = false
    private(set) var activatedLink = false

    func begin(_ event: NSEvent) -> PressRoute {
        didDrag = false
        activatedLink = false
        pressEvent = event
        let route: PressRoute
        if event.modifierFlags.contains(.option) {
            route = .application
        } else if event.clickCount == 1 {
            route = .deferredClick
        } else {
            route = .localSelection
        }
        pressRoute = route
        return route
    }

    func dragRoute() -> PressRoute {
        didDrag = true
        return pressRoute == .application ? .application : .localSelection
    }

    func noteLinkActivation() {
        activatedLink = true
    }

    func finish() {
        pressRoute = nil
        pressEvent = nil
        didDrag = false
        activatedLink = false
    }
}

@MainActor
protocol HideTerminalPointerRouting: AnyObject {
    var pointerRouting: TerminalPointerRoutingState { get }
    var onPointerFocus: (() -> Void)? { get set }
    /// The pane this view renders, so window-level routing can name it when it
    /// forwards a gesture to the core.
    var hidePaneID: String? { get set }

    func forwardMouseDown(_ event: NSEvent, selectingLocally: Bool)
    func forwardMouseDragged(_ event: NSEvent, selectingLocally: Bool)
    func forwardMouseUp(_ event: NSEvent, selectingLocally: Bool)
    func replayOrdinaryClick(_ event: NSEvent)
}

extension HideTerminalPointerRouting {
    func routeMouseDown(_ event: NSEvent) {
        onPointerFocus?()
        switch pointerRouting.begin(event) {
        case .deferredClick, .localSelection:
            forwardMouseDown(event, selectingLocally: true)
        case .application:
            forwardMouseDown(event, selectingLocally: false)
        }
    }

    func routeMouseDragged(_ event: NSEvent) {
        switch pointerRouting.dragRoute() {
        case .deferredClick:
            assertionFailure("A drag cannot remain a deferred click")
        case .localSelection:
            forwardMouseDragged(event, selectingLocally: true)
        case .application:
            forwardMouseDragged(event, selectingLocally: false)
        }
    }

    func routeMouseUp(_ event: NSEvent) {
        defer { pointerRouting.finish() }
        switch pointerRouting.pressRoute {
        case .localSelection:
            forwardMouseUp(event, selectingLocally: true)
        case .application:
            forwardMouseUp(event, selectingLocally: false)
        case .deferredClick:
            if pointerRouting.didDrag {
                forwardMouseUp(event, selectingLocally: true)
                return
            }

            // Let SwiftTerm resolve links before replaying a click to the TUI.
            // The delegate marks synchronous link activation, which wins over
            // application mouse reporting exactly as the product contract says.
            forwardMouseUp(event, selectingLocally: true)
            guard !pointerRouting.activatedLink,
                  let pressEvent = pointerRouting.pressEvent
            else { return }
            replayOrdinaryClick(pressEvent)
        case nil:
            forwardMouseUp(event, selectingLocally: false)
        }
    }

    func noteTerminalLinkActivation() {
        pointerRouting.noteLinkActivation()
    }
}

extension TerminalView {
    func withMouseReportingDisabled(_ body: () -> Void) {
        let previous = allowMouseReporting
        allowMouseReporting = false
        defer { allowMouseReporting = previous }
        body()
    }
}
