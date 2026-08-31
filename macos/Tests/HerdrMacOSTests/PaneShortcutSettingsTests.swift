import AppKit
import SwiftTerm
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

        let capsLockClose = keyEvent(
            characters: "W",
            keyCode: 13,
            modifiers: [.command, .capsLock]
        )
        #expect(PaneShortcutPolicy.command(for: capsLockClose, bindings: bindings) == .closePane)
    }

    @Test func nativeCloseWindowMenuReleasesCommandWForPaneRouting() {
        let mainMenu = NSMenu()
        let fileItem = NSMenuItem(title: "File", action: nil, keyEquivalent: "")
        let fileMenu = NSMenu(title: "File")
        let closeItem = NSMenuItem(title: "Close Window", action: nil, keyEquivalent: "w")
        closeItem.keyEquivalentModifierMask = .command
        fileMenu.addItem(closeItem)
        fileItem.submenu = fileMenu
        mainMenu.addItem(fileItem)

        #expect(PaneMenuPolicy.reserveCloseShortcut(in: mainMenu))
        #expect(closeItem.keyEquivalent.isEmpty)
        #expect(closeItem.keyEquivalentModifierMask.isEmpty)
        #expect(!PaneMenuPolicy.reserveCloseShortcut(in: mainMenu))
    }

    @Test func optionTabIgnoresCapsLockButRejectsExtraChordModifiers() {
        let capsLockOptionTab = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.option, .capsLock]
        )
        #expect(PaneKeyEventPolicy.isAgentSwitcherAdvance(capsLockOptionTab))

        let commandOptionTab = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.command, .option]
        )
        #expect(!PaneKeyEventPolicy.isAgentSwitcherAdvance(commandOptionTab))
    }

    @Test func ordinaryScrollRoutesLocallyWhileOptionScrollKeepsTerminalMouseReporting() {
        let ordinary = scrollEvent(deltaY: 3, modifiers: [.capsLock])
        let option = scrollEvent(deltaY: 3, modifiers: [.option, .capsLock])

        #expect(PaneScrollPolicy.routesToLocalScroll(ordinary))
        #expect(!PaneScrollPolicy.routesToLocalScroll(option))
    }

    @MainActor
    @Test func localTerminalScrollSuspendsAndResumesFollowTail() {
        let terminal = ImeTerminalView(
            frame: NSRect(x: 0, y: 0, width: 720, height: 360),
            font: NSFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        )
        terminal.feed(text: (1...160).map { "line-\($0)\r\n" }.joined())

        #expect(terminal.canScroll)
        #expect(terminal.scrollPosition == 1)

        terminal.scrollUp(lines: 12)
        let heldPosition = terminal.scrollPosition
        #expect(heldPosition < 1)

        terminal.feed(text: "output-while-reading-history\r\n")
        #expect(terminal.scrollPosition < 1)

        terminal.scroll(toPosition: 1)
        #expect(terminal.scrollPosition == 1)

        terminal.feed(text: "output-after-returning-to-tail\r\n")
        #expect(terminal.scrollPosition == 1)
    }

    @MainActor
    @Test func terminalHitTestingRecoversAfterViewerOverlayIsRemoved() {
        let window = PaneCommandWindow(
            contentRect: NSRect(x: 0, y: 0, width: 640, height: 480),
            styleMask: .borderless,
            backing: .buffered,
            defer: false
        )
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 640, height: 480))
        let terminal = ImeTerminalView(
            frame: NSRect(x: 80, y: 60, width: 420, height: 300),
            font: NSFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        )
        let viewerOverlay = NSView(frame: terminal.frame)
        content.addSubview(terminal)
        content.addSubview(viewerOverlay)
        window.contentView = content
        let pointInsideTerminal = NSPoint(x: terminal.frame.midX, y: terminal.frame.midY)

        #expect(window.terminalView(at: pointInsideTerminal) == nil)

        viewerOverlay.removeFromSuperview()

        #expect(window.terminalView(at: pointInsideTerminal) === terminal)
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

    private func scrollEvent(
        deltaY: CGFloat,
        modifiers: NSEvent.ModifierFlags
    ) -> NSEvent {
        CGEvent(
            scrollWheelEvent2Source: nil,
            units: .line,
            wheelCount: 1,
            wheel1: Int32(deltaY),
            wheel2: 0,
            wheel3: 0
        )!.withFlags(modifiers).flatMap(NSEvent.init(cgEvent:))!
    }
}

private extension CGEvent {
    func withFlags(_ modifiers: NSEvent.ModifierFlags) -> CGEvent? {
        flags = CGEventFlags(rawValue: UInt64(modifiers.rawValue))
        return self
    }
}
