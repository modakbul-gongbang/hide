import AppKit
import SwiftTerm

/// Pure decision table for terminal byte delivery while an IME composition is
/// being handled.
///
/// Evidence basis, from a traced human retest of the Stage 0 spike:
/// on Backspace during Korean composition the macOS IME re-marks the composing
/// text, commits it through `insertText`, and lets the key fall through to
/// SwiftTerm's `doCommand(deleteBackward:)`, which leaks a DEL byte into the
/// pane while the on-screen composition is stale. SwiftTerm's `keyDown` and
/// `doCommand` are not `open`, so the class-level rule is enforced where the
/// bytes leave the view: keys the IME owns during composition must never be
/// encoded into pane bytes.
enum CompositionInputPolicy {
    /// Whether bytes produced while handling a key event may reach the pane.
    ///
    /// - A plain Backspace during composition swallows everything: both the
    ///   IME's give-up commit and the DEL fallback. The user asked to delete
    ///   the composition, not to type it.
    /// - Any other key during composition still delivers text, but a single
    ///   C0 control byte or DEL is the IME-owned key's fallback encoding
    ///   (Enter, Escape, Tab, Backspace repeats) and is suppressed, matching
    ///   Ghostty's composing control-input rule. Multi-byte sequences such as
    ///   arrow-key escapes still pass.
    static func shouldDeliver(
        bytes: ArraySlice<UInt8>,
        composingAtEvent: Bool,
        isPlainBackspaceEvent: Bool
    ) -> Bool {
        guard composingAtEvent else { return true }
        if isPlainBackspaceEvent { return false }
        if bytes.count == 1, let byte = bytes.first, byte < 0x20 || byte == 0x7f {
            return false
        }
        return true
    }

    /// Whether an `insertText` commit arriving mid-event is the IME's give-up
    /// commit for a Backspace pressed during composition.
    static func shouldDropCommit(
        composingAtEvent: Bool,
        isPlainBackspaceEvent: Bool
    ) -> Bool {
        composingAtEvent && isPlainBackspaceEvent
    }

    /// A plain Backspace: keyCode 51 with no command/control/option modifier.
    static func isPlainBackspace(keyCode: UInt16, modifiers: NSEvent.ModifierFlags) -> Bool {
        keyCode == 51 && modifiers.intersection([.command, .control, .option]).isEmpty
    }
}

/// Encodes the one modified-key fallback that cannot be represented by the
/// legacy terminal protocol without an explicit convention.
///
/// SwiftTerm owns kitty keyboard encoding. When the attached application has
/// negotiated kitty mode, its normal `keyDown` path emits `CSI 13;2u` for
/// Shift+Enter before `interpretKeyEvents` is reached. A newly attached Hide
/// view may not have received that earlier negotiation, so the legacy fallback
/// is ESC CR, which agent CLIs interpret as a newline rather than submission.
enum ModifiedTerminalInputPolicy {
    static let shiftEnterFallback: [UInt8] = [0x1b, 0x0d]
    static let commandDeleteBytes: [UInt8] = [0x15]

    static func commandDeleteBytes(
        for event: NSEvent,
        composing: Bool
    ) -> [UInt8]? {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard !composing,
              event.type == .keyDown,
              event.keyCode == 51,
              modifiers == .command
        else { return nil }
        return commandDeleteBytes
    }

    /// `⌘←` and `⌘→` mean line start and line end in every other macOS text
    /// field. AppKit routes them to `moveToLeftEndOfLine:` and
    /// `moveToRightEndOfLine:`, which SwiftTerm encodes as `ESC b` / `ESC f` -
    /// word back and word forward - so a long agent prompt needed one press
    /// per word. `^A` / `^E` is the readline convention every shell and both
    /// agent CLIs already read as line start and line end, and is what
    /// iTerm2's natural-text-editing preset sends for these two chords.
    static let lineStartBytes: [UInt8] = [0x01]
    static let lineEndBytes: [UInt8] = [0x05]

    static func lineNavigationBytes(
        for event: NSEvent,
        composing: Bool
    ) -> [UInt8]? {
        // Arrow keys carry `.function` and `.numericPad` of their own, so the
        // comparison is against the four chord modifiers only.
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        guard !composing,
              event.type == .keyDown,
              modifiers == .command
        else { return nil }
        switch event.keyCode {
        case 123: return lineStartBytes
        case 124: return lineEndBytes
        default: return nil
        }
    }

    static func shiftEnterBytes(
        for event: NSEvent,
        kittyKeyboardEnabled: Bool,
        composing: Bool
    ) -> [UInt8]? {
        guard !kittyKeyboardEnabled,
              !composing,
              event.type == .keyDown,
              event.keyCode == 36 || event.keyCode == 76,
              event.modifierFlags.contains(.shift),
              event.modifierFlags.intersection([.command, .control, .option]).isEmpty
        else { return nil }
        return shiftEnterFallback
    }
}

/// SwiftTerm terminal view with deterministic IME composition handling.
/// `interpretKeyEvents` is the IME entry point SwiftTerm routes key events
/// through, so the composition state captured there brackets everything the
/// input method does with the event.
final class ImeTerminalView: TerminalView, HideTerminalPointerRouting {
    private var composingAtEvent = false
    private var plainBackspaceEvent = false
    let pointerRouting = TerminalPointerRoutingState()
    var onPointerFocus: (() -> Void)?
    var hidePaneID: String?
    var onOrdinaryClick: ((Int, Int, Int) -> Void)?

    /// SwiftTerm cannot see this shell's design system, so it hands the bar
    /// over and the shell paints it. Only the parts the vendor exposes are
    /// touched; the bar's layout stays the vendor's.
    /// Told what the core's search of the whole scrollback found, so the
    /// counter reports the buffer rather than the rows this view happens to
    /// hold. `nil` means no result for this pane yet.
    var paneFindSummary: String?

    /// Asks the core to search this pane's scrollback, or to step through what
    /// it found: 0 searches, +1 and -1 move.
    var onPaneFind: (@MainActor @Sendable (String, SearchOptions, Int) -> Void)?

    override func findBarSummary(term: String, index: Int, total: Int) -> String {
        // Herdr keeps this pane's history and hands this view frames, so what
        // it can count is only what is on screen. The core searched the buffer.
        paneFindSummary ?? super.findBarSummary(term: term, index: index, total: total)
    }

    override func findBarTermChanged(_ term: String, options: SearchOptions) {
        onPaneFind?(term, options, 0)
    }

    override func findBarStepRequested(_ forward: Bool, term: String, options: SearchOptions) {
        // The local step still runs, so a match already on screen is selected
        // without waiting for a round trip; the core's step moves the viewport
        // when the next match is not among the rows this view holds.
        super.findBarStepRequested(forward, term: term, options: options)
        onPaneFind?(term, options, forward ? 1 : -1)
    }

    override func findBarDidLoad(_ bar: TerminalFindBarView) {
        bar.material = .windowBackground
        bar.layer?.backgroundColor = HideTheme.Native.elevated.cgColor
        bar.layer?.cornerRadius = HideTheme.radiusMedium
        bar.layer?.borderWidth = HideTheme.Layout.hairlineWidth
        bar.layer?.borderColor = HideTheme.Native.divider.cgColor

        let targets = bar.styleTargets
        targets.field.textColor = HideTheme.Native.primary
        targets.field.font = NSFont.systemFont(ofSize: 11)
        targets.summary.textColor = HideTheme.Native.secondary
        for button in targets.buttons {
            button.contentTintColor = HideTheme.Native.secondary
        }
    }

    /// Consulted by the terminal delegate before bytes are forwarded.
    func shouldDeliverToPane(_ bytes: ArraySlice<UInt8>) -> Bool {
        CompositionInputPolicy.shouldDeliver(
            bytes: bytes,
            composingAtEvent: composingAtEvent,
            isPlainBackspaceEvent: plainBackspaceEvent
        )
    }

    override func interpretKeyEvents(_ eventArray: [NSEvent]) {
        composingAtEvent = hasMarkedText()
        plainBackspaceEvent = eventArray.contains { event in
            event.type == .keyDown
                && CompositionInputPolicy.isPlainBackspace(
                    keyCode: event.keyCode,
                    modifiers: event.modifierFlags
                )
        }
        defer {
            composingAtEvent = false
            plainBackspaceEvent = false
        }
        if let event = eventArray.first,
           let bytes = ModifiedTerminalInputPolicy.commandDeleteBytes(
               for: event,
               composing: composingAtEvent
           ) {
            terminalDelegate?.send(source: self, data: bytes[...])
            return
        }
        if let event = eventArray.first,
           let bytes = ModifiedTerminalInputPolicy.lineNavigationBytes(
               for: event,
               composing: composingAtEvent
           ) {
            terminalDelegate?.send(source: self, data: bytes[...])
            return
        }
        if let event = eventArray.first,
           let bytes = ModifiedTerminalInputPolicy.shiftEnterBytes(
               for: event,
               kittyKeyboardEnabled: !terminal.keyboardEnhancementFlags.isEmpty,
               composing: composingAtEvent
           ) {
            terminalDelegate?.send(source: self, data: bytes[...])
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

    func replayOrdinaryClick(_ event: NSEvent) {
        let cell = mouseCell(with: event)
        onOrdinaryClick?(cell.column, cell.row, PaneScrollPolicy.modifiers(event.modifierFlags))
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

    /// Repositions the marked-text (preedit) overlay to the current caret.
    ///
    /// SwiftTerm places the overlay only when the marked text itself changes,
    /// using the caret position at that moment. Committed text in this app
    /// round-trips through the pane PTY, so the caret advances a few
    /// milliseconds later, after the echo is fed back - and the overlay for
    /// the next syllable is left covering the character that was just
    /// committed. Re-asserting the same marked text after terminal output
    /// re-runs SwiftTerm's overlay layout against the advanced caret.
    func refreshMarkedTextOverlayPosition() {
        syncCaretVisibilityWithComposition()
        guard hasMarkedText() else { return }
        let range = markedRange()
        guard range.length > 0,
              let marked = attributedSubstring(forProposedRange: range, actualRange: nil)
        else { return }
        super.setMarkedText(
            marked,
            selectedRange: NSRange(location: marked.length, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
    }

    override func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        super.setMarkedText(string, selectedRange: selectedRange, replacementRange: replacementRange)
        syncCaretVisibilityWithComposition()
    }

    override func unmarkText() {
        super.unmarkText()
        syncCaretVisibilityWithComposition()
    }

    override func insertText(_ string: Any, replacementRange: NSRange) {
        if CompositionInputPolicy.shouldDropCommit(
            composingAtEvent: composingAtEvent,
            isPlainBackspaceEvent: plainBackspaceEvent
        ) {
            // Deleting the composition: clear the marked state without
            // encoding the IME's give-up commit into pane bytes.
            unmarkText()
            return
        }
        super.insertText(string, replacementRange: replacementRange)
        syncCaretVisibilityWithComposition()
    }

    /// While composing, only the underlined preedit should mark the input
    /// position; the block caret reappears when the composition commits or
    /// cancels. SwiftTerm keeps its caret view internal and never touches
    /// `isHidden`, so the view is located by class name - an isolated
    /// boundary heuristic to replace once SwiftTerm exposes caret visibility.
    private func syncCaretVisibilityWithComposition() {
        let caret = subviews.first { String(describing: type(of: $0)).contains("CaretView") }
        caret?.isHidden = hasMarkedText()
    }
}
