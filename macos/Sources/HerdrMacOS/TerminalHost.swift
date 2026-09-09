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
    var viewportSignal: TerminalViewportSignal? = nil
    /// False on a retained canvas that is not the one showing. The NSView is
    /// hidden then, which is what makes AppKit skip its display pass; the
    /// frame, and so the PTY size, is untouched.
    @Environment(\.hideCanvasVisible) private var canvasVisible
    @Environment(\.hideTerminalPaneVisible) private var paneVisible

    /// Pushes the core's search result into this pane's find bar.
    ///
    /// The result is dropped unless it names this pane: a search runs per pane
    /// and a stale answer would otherwise show its count over a different one.
    private func applyPaneFind(to terminal: TerminalView) {
        guard let terminal = terminal as? ImeTerminalView else { return }
        let find = bridge.paneFind
        guard find.paneID == paneID, !find.term.isEmpty else {
            terminal.paneFindSummary = nil
            terminal.refreshFindBar()
            return
        }
        if let reason = find.unavailableReason {
            terminal.paneFindSummary = reason
        } else {
            terminal.paneFindSummary = PaneFindSummary.text(
                index: find.index,
                total: find.total,
                truncated: find.truncated
            )
        }
        terminal.refreshFindBar()
    }

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
        context.coordinator.viewportSignal = viewportSignal
        viewportSignal?.observe(terminal)
        terminal.hidePaneID = paneID
        terminal.registerForDraggedTypes([.fileURL])
        terminal.onImageDrop = { [weak bridge] urls in
            onFocus()
            bridge?.stageImages(urls, paneID: paneID)
        }
        terminal.terminalContentsDidDraw = { TerminalLatency.drawn(paneID: paneID) }
        terminal.terminalDisplayTick = { [weak coordinator = context.coordinator] period in
            TerminalLatency.displayPeriod(period, paneID: paneID)
            coordinator?.flushSettledSize()
        }
        terminal.terminalDelegate = context.coordinator
        terminal.nativeForegroundColor = HideTheme.Native.primary
        terminal.nativeBackgroundColor = HideTheme.Native.background
        terminal.linkReporting = .implicit
        terminal.linkHighlightMode = .hoverWithModifier
        // The bidi pass was 30% of the draw for a pane streaming an agent's
        // TUI (151 of 507 draw samples, 2026-09-04), and nothing this shell
        // shows is right-to-left. Legacy left-to-right returns from the
        // layout before it scans a row.
        terminal.bidiHostPolicy = .legacyLeftToRight
        terminal.searchHighlightColor = HideTheme.Native.searchMatchHighlight
        terminal.setAccessibilityIdentifier("swiftterm-terminal-\(paneID)")
        terminal.onPointerFocus = onFocus
        terminal.onOrdinaryClick = { [weak coordinator = context.coordinator] column, row, modifiers in
            coordinator?.bridge.clickTerminal(paneID: paneID, column: column, row: row, modifiers: modifiers)
        }
        terminal.onPaneFind = { [weak bridge] term, options, step in
            bridge?.findInPane(
                paneID: paneID,
                term: term,
                caseSensitive: options.caseSensitive,
                wholeWord: options.wholeWord,
                regex: options.regex,
                step: step
            )
        }
        context.coordinator.terminal = terminal
        context.coordinator.registrationID = bridge.registerTerminal(
            paneID: paneID,
            receive: { [weak terminal] delivery in
                precondition(Thread.isMainThread)
                guard let terminal else { return }
                if let sent = delivery.inputSent {
                    TerminalLatency.inputSent(sent, paneID: paneID)
                    return
                }
                let bytes = delivery.bytes
                if let frame = delivery.frame,
                   frame.width != terminal.getTerminal().cols || frame.height != terminal.getTerminal().rows {
                    return
                }
                if !terminal.isHiddenOrHasHiddenAncestor, bytes != [0x1b, 0x63] {
                    TerminalLatency.begin(.receiveToDraw, paneID: paneID)
                }
                terminal.feed(byteArray: bytes[...])
                viewportSignal?.observe(terminal)
                // The caret advances on the next layout pass after a feed;
                // keep an active composition overlay anchored to it.
                DispatchQueue.main.async { [weak terminal] in
                    terminal?.refreshMarkedTextOverlayPosition()
                }
            },
            focus: { [weak terminal] in
                guard let terminal, let window = terminal.window else { return }
                // The core names a focused pane only inside the visible tab,
                // so a hidden view asked for the keyboard is one whose
                // canvas is coming forward in the same pass. A hidden view
                // cannot become first responder, so it is shown first.
                terminal.isHidden = false
                window.makeFirstResponder(terminal)
            }
        )
        return terminal
    }

    func updateNSView(_ terminal: TerminalView, context: Context) {
        context.coordinator.bridge = bridge
        context.coordinator.onOpenLink = onOpenLink
        (terminal as? ImeTerminalView)?.onPointerFocus = onFocus
        applyPaneFind(to: terminal)
        let showing = canvasVisible && paneVisible
        if terminal.isHidden == showing {
            // Applied on the next run-loop turn: hiding an NSView inside
            // SwiftUI's update pass re-enters the layout that is running and
            // trips the attribute graph's cycle detector.
            let visible = showing
            DispatchQueue.main.async { [weak terminal] in
                guard let terminal, terminal.isHidden == visible else { return }
                terminal.isHidden = !visible
                if !visible { TerminalLatency.hidden(paneID: paneID) }
                // Bytes fed while hidden were parsed but not drawn, so the
                // first frame after coming forward is drawn from the buffer.
                if visible {
                    terminal.needsDisplay = true
                }
            }
        }
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
        terminal.terminalDisplayTick = nil
        TerminalLatency.release(paneID: coordinator.paneID)
        terminal.terminalContentsDidDraw = nil
        terminal.terminalDelegate = nil
        (terminal as? ImeTerminalView)?.onPointerFocus = nil
        (terminal as? ImeTerminalView)?.onOrdinaryClick = nil
    }

    @MainActor final class Coordinator: NSObject, @preconcurrency TerminalViewDelegate {
        var bridge: CoreBridge
        let paneID: String
        var onOpenLink: @MainActor @Sendable (String) -> Void
        var registrationID: UUID?
        weak var terminal: TerminalView?
        weak var viewportSignal: TerminalViewportSignal?
        private var settledSize = SettledTerminalSize()
        private var reportedFirstSize = false
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
            MainActor.assumeIsolated {
                guard (source as? ImeTerminalView)?.shouldDeliverToPane(data) ?? true else { return }
                bridge.sendTerminalInput(Array(data), paneID: paneID)
            }
        }

        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
            MainActor.assumeIsolated {
                viewportSignal?.observe(source)
                settledSize.report(cols: newCols, rows: newRows)
                // This updates the frame guard only. It never resizes the PTY.
                bridge.reportTerminalViewport(paneID: paneID, cols: newCols, rows: newRows, newView: !reportedFirstSize)
                reportedFirstSize = true
            }
        }

        @MainActor func flushSettledSize() {
            guard let grid = settledSize.displayTick() else { return }
            bridge.resizeTerminal(paneID: paneID, cols: grid.cols, rows: grid.rows)
        }

        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func scrolled(source: TerminalView, position: Double) {
            MainActor.assumeIsolated { viewportSignal?.observe(source) }
        }
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

private struct TerminalPaneVisibilityKey: EnvironmentKey {
    static let defaultValue = true
}
extension EnvironmentValues {
    var hideTerminalPaneVisible: Bool {
        get { self[TerminalPaneVisibilityKey.self] }
        set { self[TerminalPaneVisibilityKey.self] = newValue }
    }
}
