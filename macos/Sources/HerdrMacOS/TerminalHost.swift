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
    let allowsInput: Bool
    /// False on a retained canvas that is not the one showing. The NSView is
    /// hidden then, which is what makes AppKit skip its display pass; the
    /// frame, and so the PTY size, is untouched.
    @Environment(\.hideCanvasVisible) private var canvasVisible
    @Environment(\.hideTerminalPaneVisible) private var paneVisible

    init(
        bridge: CoreBridge,
        paneID: String,
        textScale: CGFloat,
        onFocus: @escaping @MainActor @Sendable () -> Void,
        onOpenLink: @escaping @MainActor @Sendable (String) -> Void,
        allowsInput: Bool = true
    ) {
        self.bridge = bridge
        self.paneID = paneID
        self.textScale = textScale
        self.onFocus = onFocus
        self.onOpenLink = onOpenLink
        self.allowsInput = allowsInput
    }

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
        Coordinator(
            bridge: bridge,
            paneID: paneID,
            onOpenLink: onOpenLink,
            allowsInput: allowsInput
        )
    }

    func makeNSView(context: Context) -> TerminalView {
        let terminal = ImeTerminalView(
            frame: .zero,
            font: NSFont.monospacedSystemFont(
                ofSize: HideTheme.terminalBaseFontSize * textScale,
                weight: .regular
            )
        )
        terminal.allowsPaneInput = allowsInput
        terminal.hidePaneID = paneID
        terminal.onAttachment = { [weak bridge] input, bracketed in
            bridge?.pasteTerminalAttachment(input, paneID: paneID, bracketed: bracketed)
        }
        terminal.registerForDraggedTypes([.fileURL])
        terminal.terminalContentsDidDraw = { TerminalLatency.drawn(paneID: paneID) }
        terminal.terminalDisplayTick = { [weak coordinator = context.coordinator] period in
            TerminalLatency.displayPeriod(period, paneID: paneID)
            coordinator?.displayPeriod = period
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
        if allowsInput {
            terminal.onPointerFocus = onFocus
        } else {
            terminal.onPointerFocus = nil
        }
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
                // The caret advances on the next layout pass after a feed;
                // keep an active composition overlay anchored to it.
                DispatchQueue.main.async { [weak terminal] in
                    terminal?.refreshMarkedTextOverlayPosition()
                }
            },
            focus: { [weak coordinator = context.coordinator, weak terminal] in
                guard coordinator?.allowsInput == true else { return }
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
        context.coordinator.allowsInput = allowsInput
        if let terminal = terminal as? ImeTerminalView {
            terminal.allowsPaneInput = allowsInput
            if allowsInput {
                terminal.onPointerFocus = onFocus
            } else {
                terminal.onPointerFocus = nil
            }
            if !allowsInput, terminal.window?.firstResponder === terminal {
                terminal.window?.makeFirstResponder(nil)
            }
        }
        applyPaneFind(to: terminal)
        context.coordinator.replayGeometryIfReady(from: terminal)
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
        (terminal as? ImeTerminalView)?.onAttachment = nil
        (terminal as? ImeTerminalView)?.onOrdinaryClick = nil
    }

    @MainActor final class Coordinator: NSObject, @preconcurrency TerminalViewDelegate {
        var bridge: CoreBridge
        let paneID: String
        var onOpenLink: @MainActor @Sendable (String) -> Void
        var allowsInput: Bool
        var registrationID: UUID?
        weak var terminal: TerminalView?
        private var settledSize = SettledTerminalSize()
        private var reportedFirstSize = false
        private var undrawnSampleGeneration = 0
        /// The last period the display link reported; the undrawn sampler
        /// spaces its two samples by it, the way drawn frames are spaced.
        var displayPeriod: Double = 1.0 / 60.0
        init(
            bridge: CoreBridge,
            paneID: String,
            onOpenLink: @escaping @MainActor @Sendable (String) -> Void,
            allowsInput: Bool
        ) {
            self.bridge = bridge
            self.paneID = paneID
            self.onOpenLink = onOpenLink
            self.allowsInput = allowsInput
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
                settledSize.report(cols: newCols, rows: newRows)
                // This updates the frame guard only. It never resizes the PTY.
                if bridge.reportTerminalViewport(
                    paneID: paneID,
                    cols: newCols,
                    rows: newRows,
                    newView: !reportedFirstSize
                ) == .accepted {
                    reportedFirstSize = true
                }
                sampleSettledSizeWhileUndrawn()
            }
        }

        /// A terminal view can lay itself out before the core has confirmed
        /// Herdr compatibility. The admission boundary rejects that first
        /// geometry so no command reaches an unknown server, but SwiftTerm
        /// may never report the unchanged grid again. A published connected
        /// snapshot rebuilds this representable and replays the exact current
        /// grid once, preserving both the compatibility gate and initial
        /// terminal attachment.
        @MainActor func replayGeometryIfReady(from terminal: TerminalView) {
            guard case .connected = bridge.localHerdrMutationReadiness else { return }
            if !reportedFirstSize {
                let cols = terminal.getTerminal().cols
                let rows = terminal.getTerminal().rows
                guard cols > 0, rows > 0 else { return }
                guard bridge.reportTerminalViewport(
                    paneID: paneID,
                    cols: cols,
                    rows: rows,
                    newView: true
                ) == .accepted else { return }
                reportedFirstSize = true
            }
            flushSettledSize()
        }

        /// A pane that is not drawn never ticks - a hidden tab, a view with
        /// no window - so its PTY kept the old grid while the frame guard
        /// already expected the new one, and every frame was held for a
        /// resize only a drawn frame could send. Two samples one display
        /// period apart at one grid deliver it, exactly as two drawn frames
        /// do; a newer layout restarts the pair, so a drag still sends only
        /// the grid it settles on.
        @MainActor private func sampleSettledSizeWhileUndrawn() {
            undrawnSampleGeneration &+= 1
            let generation = undrawnSampleGeneration
            let period = displayPeriod
            DispatchQueue.main.asyncAfter(deadline: .now() + period) { [weak self] in
                guard let self, self.undrawnSampleGeneration == generation else { return }
                self.flushSettledSize()
                DispatchQueue.main.asyncAfter(deadline: .now() + period) { [weak self] in
                    guard let self, self.undrawnSampleGeneration == generation else { return }
                    self.flushSettledSize()
                }
            }
        }

        @MainActor func flushSettledSize() {
            guard let grid = settledSize.displayTick() else { return }
            if bridge.resizeTerminal(paneID: paneID, cols: grid.cols, rows: grid.rows) == .accepted {
                settledSize.markDelivered(grid)
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

private struct TerminalPaneVisibilityKey: EnvironmentKey {
    static let defaultValue = true
}
extension EnvironmentValues {
    var hideTerminalPaneVisible: Bool {
        get { self[TerminalPaneVisibilityKey.self] }
        set { self[TerminalPaneVisibilityKey.self] = newValue }
    }
}
