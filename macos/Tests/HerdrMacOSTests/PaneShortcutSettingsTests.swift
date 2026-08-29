import AppKit
import Testing
@testable import HerdrMacOS

@Suite("Pane shortcut settings")
struct PaneShortcutSettingsTests {
    @Test func missingStateUsesTheFourDocumentedDefaults() {
        let resolution = PaneShortcutPolicy.resolve(stored: [:])

        #expect(resolution.diagnostic == nil)
        #expect(resolution.bindings[.splitRight]?.canonical == "command+d")
        #expect(resolution.bindings[.splitDown]?.canonical == "command+shift+d")
        #expect(resolution.bindings[.toggleZoom]?.canonical == "command+option+return")
        #expect(resolution.bindings[.closePane]?.canonical == "command+w")
    }

    @Test func corruptReservedOrConflictingStoredBindingsFallBackAsOneSafeSet() {
        for stored in [
            ["split_right": "not-a-chord"],
            ["split_right": "command+q"],
            ["split_right": "command+x", "split_down": "command+x"],
        ] {
            let resolution = PaneShortcutPolicy.resolve(stored: stored)

            #expect(resolution.diagnostic != nil)
            #expect(resolution.bindings == PaneShortcutPolicy.defaults)
        }
    }

    @Test func validPersistedBindingsRestoreWithNativeMenuEquivalents() {
        let resolution = PaneShortcutPolicy.resolve(stored: [
            "split_right": "cmd+option+r",
            "split_down": "command+control+j",
            "toggle_zoom": "command+shift+return",
            "close_pane": "command+option+w",
        ])

        #expect(resolution.diagnostic == nil)
        #expect(resolution.bindings[.splitRight]?.canonical == "command+option+r")
        #expect(resolution.bindings[.splitRight]?.keyEquivalent.character == "r")
        #expect(resolution.bindings[.splitRight]?.eventModifiers == [.command, .option])
    }

    @Test func rebindRejectsDuplicateAndUpdatesOnlyTheRequestedAction() {
        let duplicate = PaneShortcutPolicy.updating(
            command: .splitRight,
            raw: "command+shift+d",
            current: PaneShortcutPolicy.defaults
        )
        guard case let .failure(error) = duplicate else {
            Issue.record("duplicate shortcut should be rejected")
            return
        }
        #expect(error == .duplicate("Split Down"))

        let updated = PaneShortcutPolicy.updating(
            command: .splitRight,
            raw: "command+option+r",
            current: PaneShortcutPolicy.defaults
        )
        guard case let .success(bindings) = updated else {
            Issue.record("valid shortcut should update")
            return
        }
        #expect(bindings[.splitRight]?.canonical == "command+option+r")
        #expect(bindings[.splitDown] == PaneCommand.splitDown.defaultShortcut)
    }

    @Test func nativeWindowRoutingMatchesAllFourPaneCommandsAndNothingReserved() {
        let bindings = PaneShortcutPolicy.defaults
        let cases: [(String, UInt16, NSEvent.ModifierFlags, PaneCommand)] = [
            ("d", 2, [.command], .splitRight),
            ("D", 2, [.command, .shift], .splitDown),
            ("\r", 36, [.command, .option], .toggleZoom),
            ("w", 13, [.command], .closePane),
        ]

        for (characters, keyCode, modifiers, expected) in cases {
            let event = keyEvent(characters: characters, keyCode: keyCode, modifiers: modifiers)
            #expect(PaneShortcutPolicy.command(for: event, bindings: bindings) == expected)
        }
        let settings = keyEvent(characters: ",", keyCode: 43, modifiers: [.command])
        #expect(PaneShortcutPolicy.command(for: settings, bindings: bindings) == nil)
    }

    private func keyEvent(
        characters: String,
        keyCode: UInt16,
        modifiers: NSEvent.ModifierFlags
    ) -> NSEvent {
        NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: modifiers,
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters.lowercased(),
            isARepeat: false,
            keyCode: keyCode
        )!
    }
}
