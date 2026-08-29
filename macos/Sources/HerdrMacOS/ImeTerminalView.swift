import AppKit
import SwiftTerm

/// Pure decision table for terminal byte delivery while an IME composition is
/// being handled.
///
/// Evidence basis (`spikes/swift-shell-pivot/evidence/v9-backspace-adapter-human-retest-trace.json`):
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

/// SwiftTerm terminal view with deterministic IME composition handling.
/// `interpretKeyEvents` is the IME entry point SwiftTerm routes key events
/// through, so the composition state captured there brackets everything the
/// input method does with the event.
final class ImeTerminalView: TerminalView {
    private var composingAtEvent = false
    private var plainBackspaceEvent = false

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
        super.interpretKeyEvents(eventArray)
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
    }
}
