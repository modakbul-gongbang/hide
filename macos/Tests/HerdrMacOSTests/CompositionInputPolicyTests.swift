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
}
