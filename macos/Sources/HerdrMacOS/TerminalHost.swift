import AppKit
import SwiftTerm
import SwiftUI

struct TerminalHost: NSViewRepresentable {
    @ObservedObject var bridge: CoreBridge
    let paneID: String
    /// This pane's text scale, from the core's persisted per-pane map.
    let textScale: CGFloat
    let onFocus: @MainActor @Sendable () -> Void
    let onOpenLink: @MainActor @Sendable (String) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(bridge: bridge, paneID: paneID, onOpenLink: onOpenLink)
    }

    func makeNSView(context: Context) -> TerminalView {
        let terminal = ImeTerminalView(
            frame: .zero,
            font: NSFont.monospacedSystemFont(
                ofSize: HideTheme.terminalBaseFontSize * textScale,
                weight: .regular
            )
        )
        terminal.hidePaneID = paneID
        terminal.terminalDelegate = context.coordinator
        terminal.nativeForegroundColor = NSColor(calibratedWhite: 0.9, alpha: 1)
        terminal.nativeBackgroundColor = NSColor(calibratedRed: 0.045, green: 0.055, blue: 0.075, alpha: 1)
        terminal.linkReporting = .implicit
        terminal.linkHighlightMode = .hover
        terminal.searchHighlightColor = HideTheme.Native.searchMatchHighlight
        terminal.setAccessibilityIdentifier("swiftterm-terminal-\(paneID)")
        terminal.onPointerFocus = onFocus
        context.coordinator.terminal = terminal
        context.coordinator.registrationID = bridge.registerTerminal(
            paneID: paneID,
            receive: { [weak terminal] bytes in
                precondition(Thread.isMainThread)
                terminal?.feed(byteArray: bytes[...])
                // The caret advances on the next layout pass after a feed;
                // keep an active composition overlay anchored to it.
                DispatchQueue.main.async { [weak terminal] in
                    terminal?.refreshMarkedTextOverlayPosition()
                }
            },
            focus: { [weak terminal] in
                guard let terminal, let window = terminal.window else { return }
                window.makeFirstResponder(terminal)
            }
        )
        return terminal
    }

    func updateNSView(_ terminal: TerminalView, context: Context) {
        context.coordinator.bridge = bridge
        context.coordinator.onOpenLink = onOpenLink
        (terminal as? ImeTerminalView)?.onPointerFocus = onFocus
        // SwiftTerm's font setter recomputes the cell metrics and resizes the
        // grid, and its delegate reports the new size on to the PTY, so the
        // reflow follows from this assignment. It also clears the selection,
        // which is why it is guarded on an actual change.
        let size = HideTheme.terminalBaseFontSize * textScale
        if terminal.font.pointSize != size {
            terminal.font = NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
        }
    }

    static func dismantleNSView(_ terminal: TerminalView, coordinator: Coordinator) {
        if let registrationID = coordinator.registrationID {
            coordinator.bridge.unregisterTerminal(
                paneID: coordinator.paneID,
                registrationID: registrationID
            )
        }
        terminal.terminalDelegate = nil
        (terminal as? ImeTerminalView)?.onPointerFocus = nil
    }

    final class Coordinator: NSObject, TerminalViewDelegate {
        var bridge: CoreBridge
        let paneID: String
        var onOpenLink: @MainActor @Sendable (String) -> Void
        var registrationID: UUID?
        weak var terminal: TerminalView?

        init(
            bridge: CoreBridge,
            paneID: String,
            onOpenLink: @escaping @MainActor @Sendable (String) -> Void
        ) {
            self.bridge = bridge
            self.paneID = paneID
            self.onOpenLink = onOpenLink
        }

        func send(source: TerminalView, data: ArraySlice<UInt8>) {
            // SwiftTerm delivers delegate sends synchronously from key
            // handling on the main thread; the isolation assumption fails
            // loudly if that ever changes.
            let deliverable = MainActor.assumeIsolated {
                (source as? ImeTerminalView)?.shouldDeliverToPane(data) ?? true
            }
            guard deliverable else { return }
            let bytes = Array(data)
            let paneID = self.paneID
            Task { @MainActor [weak bridge] in
                bridge?.sendTerminalInput(bytes, paneID: paneID)
            }
        }

        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
            guard newCols > 0, newRows > 0 else { return }
            let paneID = self.paneID
            Task { @MainActor [weak bridge] in
                bridge?.resizeTerminal(
                    paneID: paneID,
                    cols: newCols,
                    rows: newRows
                )
            }
        }

        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func scrolled(source: TerminalView, position: Double) {}
        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}

        func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
            // SwiftTerm invokes link delegates synchronously from AppKit mouse
            // handling. Keep the activation marker synchronous so the router
            // can suppress the TUI click replay in this same mouse-up event.
            let handler = onOpenLink
            MainActor.assumeIsolated {
                (source as? ImeTerminalView)?.noteTerminalLinkActivation()
                handler(link)
            }
        }
    }
}
