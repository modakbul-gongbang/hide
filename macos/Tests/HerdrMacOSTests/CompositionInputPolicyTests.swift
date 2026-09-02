import AppKit
import Testing

@testable import HerdrMacOS

@Suite struct CompositionInputPolicyTests {
    private func deliver(
        _ bytes: [UInt8],
        composing: Bool,
        backspace: Bool = false
    ) -> Bool {
        CompositionInputPolicy.shouldDeliver(
            bytes: bytes[...],
            composingAtEvent: composing,
            isPlainBackspaceEvent: backspace
        )
    }

    @Test func composingSuppressesControlByteFallbacks() {
        // The recorded V9 failure: deleteBackward leaked DEL into the pane
        // while Korean composition was active.
        #expect(!deliver([0x7f], composing: true))
        #expect(!deliver([0x0d], composing: true)) // Enter commit fallback
        #expect(!deliver([0x1b], composing: true)) // Escape cancel fallback
        #expect(!deliver([0x09], composing: true)) // Tab fallback
        #expect(!deliver([0x08], composing: true)) // backspaceSendsControlH
    }

    @Test func composingStillDeliversTextAndArrowSequences() {
        #expect(deliver(Array("\u{ac00}".utf8), composing: true)) // committed syllable
        #expect(deliver([0x1b, 0x5b, 0x41], composing: true)) // up-arrow escape
        #expect(deliver(Array("a".utf8), composing: true))
    }

    @Test func backspaceDuringCompositionSwallowsEverything() {
        #expect(!deliver(Array("\u{3134}".utf8), composing: true, backspace: true))
        #expect(!deliver([0x7f], composing: true, backspace: true))
    }

    @Test func notComposingDeliversEverything() {
        #expect(deliver([0x7f], composing: false))
        #expect(deliver([0x0d], composing: false))
        #expect(deliver(Array("\u{d55c}".utf8), composing: false))
        #expect(deliver([0x7f], composing: false, backspace: true))
    }

    @Test func giveUpCommitIsDroppedOnlyForComposedBackspace() {
        #expect(CompositionInputPolicy.shouldDropCommit(
            composingAtEvent: true, isPlainBackspaceEvent: true))
        // A commit while typing the next syllable, pressing space, or any
        // ordinary insert must keep flowing to the pane.
        #expect(!CompositionInputPolicy.shouldDropCommit(
            composingAtEvent: true, isPlainBackspaceEvent: false))
        #expect(!CompositionInputPolicy.shouldDropCommit(
            composingAtEvent: false, isPlainBackspaceEvent: true))
    }

    @Test func plainBackspaceExcludesShortcutChords() {
        #expect(CompositionInputPolicy.isPlainBackspace(keyCode: 51, modifiers: []))
        #expect(CompositionInputPolicy.isPlainBackspace(keyCode: 51, modifiers: [.shift]))
        #expect(!CompositionInputPolicy.isPlainBackspace(keyCode: 51, modifiers: [.command]))
        #expect(!CompositionInputPolicy.isPlainBackspace(keyCode: 51, modifiers: [.control]))
        #expect(!CompositionInputPolicy.isPlainBackspace(keyCode: 51, modifiers: [.option]))
        #expect(!CompositionInputPolicy.isPlainBackspace(keyCode: 49, modifiers: []))
    }

    @Test func commandDeleteProducesOneControlUOnlyOutsideComposition() {
        let commandDelete = NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: .command,
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: "\u{7f}",
            charactersIgnoringModifiers: "\u{7f}",
            isARepeat: false,
            keyCode: 51
        )!
        #expect(ModifiedTerminalInputPolicy.commandDeleteBytes(
            for: commandDelete,
            composing: false
        ) == [0x15])
        #expect(ModifiedTerminalInputPolicy.commandDeleteBytes(
            for: commandDelete,
            composing: true
        ) == nil)

        for modifiers: NSEvent.ModifierFlags in [[], [.command, .shift], [.command, .option], .control] {
            let event = NSEvent.keyEvent(
                with: .keyDown,
                location: .zero,
                modifierFlags: modifiers,
                timestamp: 0,
                windowNumber: 0,
                context: nil,
                characters: "\u{7f}",
                charactersIgnoringModifiers: "\u{7f}",
                isARepeat: false,
                keyCode: 51
            )!
            #expect(ModifiedTerminalInputPolicy.commandDeleteBytes(
                for: event,
                composing: false
            ) == nil)
        }
    }

    /// `⌘←` and `⌘→` reached SwiftTerm as `moveToLeftEndOfLine:` and
    /// `moveToRightEndOfLine:`, which it encodes as `ESC b` / `ESC f` - word
    /// back and word forward. A long agent prompt then needed one press per
    /// word. These pin the line-start and line-end encoding, and pin that the
    /// arrow keys' own `.function` and `.numericPad` flags do not defeat the
    /// modifier comparison, which is what a naive equality check would do.
    @Test func commandArrowsEncodeLineStartAndLineEnd() throws {
        func arrow(_ keyCode: UInt16, _ modifiers: NSEvent.ModifierFlags) throws -> NSEvent {
            try #require(NSEvent.keyEvent(
                with: .keyDown,
                location: .zero,
                modifierFlags: modifiers.union([.function, .numericPad]),
                timestamp: 0,
                windowNumber: 0,
                context: nil,
                characters: "",
                charactersIgnoringModifiers: "",
                isARepeat: false,
                keyCode: keyCode
            ))
        }

        #expect(ModifiedTerminalInputPolicy.lineNavigationBytes(
            for: try arrow(123, .command),
            composing: false
        ) == [0x01])
        #expect(ModifiedTerminalInputPolicy.lineNavigationBytes(
            for: try arrow(124, .command),
            composing: false
        ) == [0x05])
    }

    @Test func plainAndOtherModifiedArrowsAreLeftToTheTerminal() throws {
        func arrow(_ keyCode: UInt16, _ modifiers: NSEvent.ModifierFlags) throws -> NSEvent {
            try #require(NSEvent.keyEvent(
                with: .keyDown,
                location: .zero,
                modifierFlags: modifiers.union([.function, .numericPad]),
                timestamp: 0,
                windowNumber: 0,
                context: nil,
                characters: "",
                charactersIgnoringModifiers: "",
                isARepeat: false,
                keyCode: keyCode
            ))
        }

        for modifiers: NSEvent.ModifierFlags in [[], .option, .control, [.command, .shift], [.command, .option]] {
            for keyCode: UInt16 in [123, 124] {
                #expect(ModifiedTerminalInputPolicy.lineNavigationBytes(
                    for: try arrow(keyCode, modifiers),
                    composing: false
                ) == nil)
            }
        }
        for keyCode: UInt16 in [125, 126] {
            #expect(ModifiedTerminalInputPolicy.lineNavigationBytes(
                for: try arrow(keyCode, .command),
                composing: false
            ) == nil)
        }
        #expect(ModifiedTerminalInputPolicy.lineNavigationBytes(
            for: try arrow(123, .command),
            composing: true
        ) == nil)
    }
}
