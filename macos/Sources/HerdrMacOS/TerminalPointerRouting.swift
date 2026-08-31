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

    func forwardMouseDown(_ event: NSEvent, selectingLocally: Bool)
    func forwardMouseDragged(_ event: NSEvent, selectingLocally: Bool)
    func forwardMouseUp(_ event: NSEvent, selectingLocally: Bool)
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
            forwardMouseDown(pressEvent, selectingLocally: false)
            forwardMouseUp(event, selectingLocally: false)
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

/// The remote process transport stays owned by SwiftTerm. Only pointer
/// ownership changes, through the same router used by the local terminal.
final class HideRemoteTerminalView: LocalProcessTerminalView, HideTerminalPointerRouting {
    let pointerRouting = TerminalPointerRoutingState()
    var onPointerFocus: (() -> Void)?
    var onOpenLink: (@MainActor @Sendable (String) -> Void)?

    override func interpretKeyEvents(_ eventArray: [NSEvent]) {
        if let event = eventArray.first,
           let bytes = ModifiedTerminalInputPolicy.shiftEnterBytes(
               for: event,
               kittyKeyboardEnabled: !terminal.keyboardEnhancementFlags.isEmpty,
               composing: hasMarkedText()
           ) {
            // LocalProcessTerminalView owns the SSH child transport, so send
            // the same legacy fallback through its delegate path. When kitty
            // mode is active the shared policy returns nil and SwiftTerm emits
            // CSI 13;2u through its normal keyboard encoder instead.
            send(source: self, data: bytes[...])
            return
        }
        super.interpretKeyEvents(eventArray)
    }

    override func mouseDown(with event: NSEvent) {
        routeMouseDown(event)
    }

    override func mouseDragged(with event: NSEvent) {
        routeMouseDragged(event)
    }

    override func mouseUp(with event: NSEvent) {
        routeMouseUp(event)
    }

    func forwardMouseDown(_ event: NSEvent, selectingLocally: Bool) {
        if selectingLocally {
            withMouseReportingDisabled { super.mouseDown(with: event) }
        } else {
            super.mouseDown(with: event)
        }
    }

    func forwardMouseDragged(_ event: NSEvent, selectingLocally: Bool) {
        if selectingLocally {
            withMouseReportingDisabled { super.mouseDragged(with: event) }
        } else {
            super.mouseDragged(with: event)
        }
    }

    func forwardMouseUp(_ event: NSEvent, selectingLocally: Bool) {
        if selectingLocally {
            withMouseReportingDisabled { super.mouseUp(with: event) }
        } else {
            super.mouseUp(with: event)
        }
    }

    override func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
        noteTerminalLinkActivation()
        onOpenLink?(link)
    }
}
