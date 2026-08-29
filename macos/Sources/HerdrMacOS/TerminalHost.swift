import AppKit
import SwiftTerm
import SwiftUI

struct TerminalHost: NSViewRepresentable {
    @ObservedObject var bridge: CoreBridge
    let paneID: String

    func makeCoordinator() -> Coordinator {
        Coordinator(bridge: bridge, paneID: paneID)
    }

    func makeNSView(context: Context) -> TerminalView {
        let terminal = ImeTerminalView(
            frame: .zero,
            font: NSFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        )
        terminal.terminalDelegate = context.coordinator
        terminal.nativeForegroundColor = NSColor(calibratedWhite: 0.9, alpha: 1)
        terminal.nativeBackgroundColor = NSColor(calibratedRed: 0.045, green: 0.055, blue: 0.075, alpha: 1)
        terminal.setAccessibilityIdentifier("swiftterm-terminal-\(paneID)")
        context.coordinator.terminal = terminal
        let clickRecognizer = NSClickGestureRecognizer(
            target: context.coordinator,
            action: #selector(Coordinator.terminalClicked(_:))
        )
        terminal.addGestureRecognizer(clickRecognizer)
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
    }

    static func dismantleNSView(_ terminal: TerminalView, coordinator: Coordinator) {
        if let registrationID = coordinator.registrationID {
            coordinator.bridge.unregisterTerminal(
                paneID: coordinator.paneID,
                registrationID: registrationID
            )
        }
        terminal.terminalDelegate = nil
    }

    final class Coordinator: NSObject, TerminalViewDelegate {
        var bridge: CoreBridge
        let paneID: String
        var registrationID: UUID?
        weak var terminal: TerminalView?

        init(bridge: CoreBridge, paneID: String) {
            self.bridge = bridge
            self.paneID = paneID
        }

        @MainActor @objc func terminalClicked(_ recognizer: NSClickGestureRecognizer) {
            guard recognizer.state == .ended else { return }
            bridge.focusPane(paneID)
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
    }
}
