import AppKit
import SwiftTerm
import SwiftUI

struct TerminalHost: NSViewRepresentable {
    @ObservedObject var bridge: CoreBridge

    func makeCoordinator() -> Coordinator {
        Coordinator(bridge: bridge)
    }

    func makeNSView(context: Context) -> TerminalView {
        let terminal = TerminalView(
            frame: .zero,
            font: NSFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        )
        terminal.terminalDelegate = context.coordinator
        terminal.nativeForegroundColor = NSColor(calibratedWhite: 0.9, alpha: 1)
        terminal.nativeBackgroundColor = NSColor(calibratedRed: 0.045, green: 0.055, blue: 0.075, alpha: 1)
        terminal.setAccessibilityIdentifier("swiftterm-terminal")
        context.coordinator.terminal = terminal
        bridge.onTerminalBytes = { [weak terminal] bytes in
            precondition(Thread.isMainThread)
            terminal?.feed(byteArray: bytes[...])
        }
        DispatchQueue.main.async {
            if let window = terminal.window, window.isKeyWindow {
                _ = window.makeFirstResponder(terminal)
            }
        }
        return terminal
    }

    func updateNSView(_ terminal: TerminalView, context: Context) {
        context.coordinator.bridge = bridge
    }

    static func dismantleNSView(_ terminal: TerminalView, coordinator: Coordinator) {
        coordinator.bridge.onTerminalBytes = nil
        terminal.terminalDelegate = nil
    }

    final class Coordinator: NSObject, TerminalViewDelegate {
        var bridge: CoreBridge
        weak var terminal: TerminalView?

        init(bridge: CoreBridge) {
            self.bridge = bridge
        }

        func send(source: TerminalView, data: ArraySlice<UInt8>) {
            let bytes = Array(data)
            Task { @MainActor [weak bridge] in
                bridge?.sendTerminalInput(bytes)
            }
        }

        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {}
        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func scrolled(source: TerminalView, position: Double) {}
        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
    }
}
