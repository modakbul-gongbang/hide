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
        #expect(resolution.bindings[.closePane]?.canonical == "command+shift+w")
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
            ("W", 13, [.command, .shift], .closePane),
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
            modifiers: [.command, .shift, .capsLock]
        )
        #expect(PaneShortcutPolicy.command(for: capsLockClose, bindings: bindings) == .closePane)
    }

    @Test func commandWIsReservedForUnifiedTabClose() {
        let closeTab = keyEvent(characters: "w", keyCode: 13, modifiers: [.command])
        let closePane = keyEvent(characters: "W", keyCode: 13, modifiers: [.command, .shift])

        #expect(UnifiedTabShortcutPolicy.isClose(closeTab))
        #expect(!UnifiedTabShortcutPolicy.isClose(closePane))
        #expect(PaneShortcutPolicy.command(for: closeTab, bindings: PaneShortcutPolicy.defaults) == nil)
        #expect(PaneShortcutPolicy.command(for: closePane, bindings: PaneShortcutPolicy.defaults) == .closePane)
    }

    /// Every pane chord was dead in real use while this file's other tests
    /// stayed green, because they all spell the event the way a US layout
    /// delivers it. With a Hangul input source selected macOS reports "ㅇ" as
    /// the D key's `charactersIgnoringModifiers`, and matching on that
    /// character resolved nothing at all - so the operator pressed ⌘D and the
    /// pane did not split, with no error to read.
    ///
    /// The assertion is what the operator observes: press the physical key,
    /// get the command, whatever the active input source spells it as. The
    /// characters below are what a Korean 2-set source actually delivers for
    /// those physical keys.
    @Test func everyChordResolvesFromThePhysicalKeyUnderANonLatinInputSource() {
        let bindings = PaneShortcutPolicy.defaults
        let cases: [(String, UInt16, NSEvent.ModifierFlags, PaneCommand)] = [
            ("ㅇ", 2, [.command], .splitRight),
            ("ㅇ", 2, [.command, .shift], .splitDown),
            ("ㅈ", 13, [.command, .shift], .closePane),
            ("=", 24, [.command], .increaseTextSize),
            ("-", 27, [.command], .decreaseTextSize),
            ("0", 29, [.command], .resetTextSize),
        ]

        for (characters, keyCode, modifiers, expected) in cases {
            let event = keyEvent(characters: characters, keyCode: keyCode, modifiers: modifiers)
            #expect(PaneShortcutPolicy.command(for: event, bindings: bindings) == expected)
        }

        // The same press under a Latin source still resolves, so the fix did
        // not trade one layout for another.
        let latin = keyEvent(characters: "d", keyCode: 2, modifiers: [.command])
        #expect(PaneShortcutPolicy.command(for: latin, bindings: bindings) == .splitRight)
    }

    /// A default bound to a key the physical-key table does not name would
    /// never fire, and would do it silently - the same class of failure the
    /// test above covers, one layer up where a new command is added.
    @Test func everyDefaultChordNamesAKeyThePhysicalTableCanReport() {
        for command in PaneCommand.allCases {
            let key = command.defaultShortcut.key
            #expect(
                key == "return" || PetHotkey.keyCode(for: key) != nil,
                "\(command.rawValue) is bound to \(key), which no physical key reports"
            )
        }
    }

    /// The close shortcut used to close the whole window once nothing was left
    /// to close, which is how the operator lost the application by pressing
    /// `⌘W` one time too many. It now says there is nothing to close instead.
    @Test func closeShortcutClosesTabsAndNeverTheWindow() {
        #expect(CloseShortcutPolicy.action(
            hasWorkspace: true,
            hasActiveFileTab: true,
            hasActiveHerdrTab: true,
            tabCount: 2
        ) == .closeFile)
        // A focused pane is what the operator is looking at, so ⌘W closes
        // it rather than the whole tab with every pane in it.
        #expect(CloseShortcutPolicy.action(
            hasWorkspace: true,
            hasActiveFileTab: false,
            hasActiveHerdrTab: true,
            hasFocusedPane: true,
            tabCount: 1
        ) == .closePane)
        #expect(CloseShortcutPolicy.action(
            hasWorkspace: true,
            hasActiveFileTab: false,
            hasActiveHerdrTab: true,
            tabCount: 1
        ) == .closeHerdr)
        #expect(CloseShortcutPolicy.action(
            hasWorkspace: true,
            hasActiveFileTab: false,
            hasActiveHerdrTab: false,
            tabCount: 0
        ) == .nothingToClose)
        #expect(CloseShortcutPolicy.action(
            hasWorkspace: false,
            hasActiveFileTab: false,
            hasActiveHerdrTab: false,
            tabCount: 0
        ) == .nothingToClose)
        #expect(CloseShortcutPolicy.action(
            hasWorkspace: true,
            hasActiveFileTab: false,
            hasActiveHerdrTab: false,
            tabCount: 3
        ) == .blocked)
    }

    @Test func herdrTabLabelsUseStableTabNumbersWithoutOverwritingCustomNames() {
        #expect(HerdrTabLabelPresentation.displayLabel(rawLabel: "1", fallbackIndex: 4) == "Tab 1")
        #expect(HerdrTabLabelPresentation.displayLabel(rawLabel: nil, fallbackIndex: 1) == "Tab 2")
        #expect(HerdrTabLabelPresentation.displayLabel(rawLabel: "Review", fallbackIndex: 0) == "Review")
        #expect(HerdrTabLabelPresentation.nextLabel(rawLabels: ["1", "Tab 2", "Review"]) == "Tab 3")
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

    @Test func controlTabIgnoresCapsLockButRejectsExtraChordModifiers() {
        let capsLockControlTab = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.control, .capsLock]
        )
        #expect(PaneKeyEventPolicy.isAgentSwitcherAdvance(capsLockControlTab))

        let commandControlTab = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.command, .control]
        )
        #expect(!PaneKeyEventPolicy.isAgentSwitcherAdvance(commandControlTab))
    }

    @Test func commandEIsReservedForSidebarNavigationAcrossFocusedEditors() {
        let commandE = keyEvent(
            characters: "e",
            keyCode: 14,
            modifiers: [.command, .capsLock]
        )
        let commandShiftE = keyEvent(
            characters: "E",
            keyCode: 14,
            modifiers: [.command, .shift]
        )
        let commandF = keyEvent(characters: "f", keyCode: 3, modifiers: [.command])

        #expect(PaneKeyEventPolicy.isSidebarViewToggle(commandE))
        #expect(!PaneKeyEventPolicy.isSidebarViewToggle(commandShiftE))
        #expect(!PaneKeyEventPolicy.isSidebarViewToggle(commandF))
    }

    @Test func theTwoSwitcherChordsNeverMatchTheSameEvent() {
        // The reverse chord is the forward chord plus Shift, so an event must
        // answer to exactly one of them or the monitor would swallow Shift+Tab
        // as a plain advance.
        let controlTab = keyEvent(characters: "\t", keyCode: 48, modifiers: [.control])
        let optionShiftTab = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.control, .shift]
        )

        #expect(PaneKeyEventPolicy.isAgentSwitcherAdvance(controlTab))
        #expect(!PaneKeyEventPolicy.isAgentSwitcherRetreat(controlTab))

        #expect(PaneKeyEventPolicy.isAgentSwitcherRetreat(optionShiftTab))
        #expect(!PaneKeyEventPolicy.isAgentSwitcherAdvance(optionShiftTab))
    }

    @Test func reverseSwitcherChordIgnoresCapsLockButRejectsExtraModifiers() {
        let capsLocked = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.control, .shift, .capsLock]
        )
        #expect(PaneKeyEventPolicy.isAgentSwitcherRetreat(capsLocked))

        let withCommand = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.command, .control, .shift]
        )
        #expect(!PaneKeyEventPolicy.isAgentSwitcherRetreat(withCommand))
    }

    @Test func controlTabChordsAreCheckoutTabSwitchingOnly() {
        let forward = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.option, .capsLock]
        )
        let backward = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.option, .shift]
        )
        let withControl = keyEvent(
            characters: "\t",
            keyCode: 48,
            modifiers: [.option, .control]
        )

        #expect(PaneKeyEventPolicy.isTabSwitcherAdvance(forward))
        #expect(!PaneKeyEventPolicy.isTabSwitcherRetreat(forward))
        #expect(PaneKeyEventPolicy.isTabSwitcherRetreat(backward))
        #expect(!PaneKeyEventPolicy.isTabSwitcherAdvance(backward))
        #expect(!PaneKeyEventPolicy.isTabSwitcherAdvance(withControl))
        #expect(!PaneKeyEventPolicy.isTabSwitcherRetreat(withControl))
        #expect(!PaneKeyEventPolicy.isAgentSwitcherAdvance(forward))
    }

    @Test func tabKeyUpFallbackCommitsOnlyAfterTheGlobalOptionFlagClears() {
        // Accessibility synthesis can leave Option on the Tab keyUp event
        // while the actual global modifier state is already released.
        let tabKeyUp = keyEvent(
            type: .keyUp,
            characters: "\t",
            keyCode: 48,
            modifiers: [.option]
        )
        #expect(PaneKeyEventPolicy.shouldCommitTabSwitcherAfterKeyUp(
            tabKeyUp,
            currentModifiers: []
        ))
        #expect(!PaneKeyEventPolicy.shouldCommitTabSwitcherAfterKeyUp(
            tabKeyUp,
            currentModifiers: [.option]
        ))

        let tabKeyDown = keyEvent(characters: "\t", keyCode: 48, modifiers: [.option])
        let anotherKeyUp = keyEvent(
            type: .keyUp,
            characters: "a",
            keyCode: 0,
            modifiers: []
        )
        #expect(!PaneKeyEventPolicy.shouldCommitTabSwitcherAfterKeyUp(
            tabKeyDown,
            currentModifiers: []
        ))
        #expect(!PaneKeyEventPolicy.shouldCommitTabSwitcherAfterKeyUp(
            anotherKeyUp,
            currentModifiers: []
        ))
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
        // The real viewer is NSScrollView-backed - a SwiftUI ScrollView for
        // markdown, an NSTextView in a scroll view for code - and that is what
        // makes it keep the wheel. A bare NSView would model a decoration
        // instead, which is deliberately scroll-transparent.
        let viewerOverlay = NSScrollView(frame: terminal.frame)
        content.addSubview(terminal)
        content.addSubview(viewerOverlay)
        window.contentView = content
        let pointInsideTerminal = NSPoint(x: terminal.frame.midX, y: terminal.frame.midY)

        #expect(window.terminalView(at: pointInsideTerminal) == nil)

        viewerOverlay.removeFromSuperview()

        #expect(window.terminalView(at: pointInsideTerminal) === terminal)
    }

    /// The regression this whole lookup exists for: a decoration layered over a
    /// terminal takes clicks, so `hitTest` returns it, and walking up from it
    /// never reaches the terminal it covers. Scrolling died wherever one sat.
    @MainActor
    @Test func aDecorationLayeredOverATerminalDoesNotSwallowTheWheel() {
        let window = PaneCommandWindow(
            contentRect: NSRect(x: 0, y: 0, width: 640, height: 480),
            styleMask: .borderless,
            backing: .buffered,
            defer: false
        )
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 640, height: 480))
        let terminal = ImeTerminalView(
            frame: NSRect(x: 0, y: 0, width: 640, height: 480),
            font: NSFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        )
        // The pane resize strip: a sibling of the terminal, drawn above it.
        let resizeStrip = NSView(frame: NSRect(x: 314, y: 0, width: 12, height: 480))
        content.addSubview(terminal)
        content.addSubview(resizeStrip)
        window.contentView = content

        #expect(window.terminalView(at: NSPoint(x: 320, y: 240)) === terminal)
    }

    private func keyEvent(
        type: NSEvent.EventType = .keyDown,
        characters: String,
        keyCode: UInt16,
        modifiers: NSEvent.ModifierFlags
    ) -> NSEvent {
        NSEvent.keyEvent(
            with: type,
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

/// Scrolling died whenever a decoration was layered over a terminal, because
/// the window resolved the target by walking up from `hitTest` and an overlay
/// is the terminal's sibling rather than its child. These cover the lookup that
/// replaced that walk, so the next overlay cannot take the wheel with it.
@Suite("Terminal lookup under overlays")
@MainActor
struct TerminalViewLookupTests {
    private func terminal(frame: CGRect) -> TerminalView {
        ImeTerminalView(
            frame: frame,
            font: NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        )
    }

    private func root(_ subviews: [NSView]) -> NSView {
        let root = NSView(frame: CGRect(x: 0, y: 0, width: 400, height: 300))
        for subview in subviews {
            root.addSubview(subview)
        }
        return root
    }

    @Test func aTerminalIsFoundThroughTheDecorationLayeredOverIt() {
        let pane = terminal(frame: CGRect(x: 0, y: 0, width: 400, height: 300))
        // The resize strip sits above the terminal and takes clicks, which is
        // exactly what used to swallow the wheel.
        let strip = NSView(frame: CGRect(x: 196, y: 0, width: 12, height: 300))
        let container = root([pane, strip])

        let found = PaneCommandWindow.frontmostTerminalView(
            in: container,
            containing: NSPoint(x: 200, y: 150)
        )

        #expect(found === pane)
    }

    @Test func aPointOutsideEveryTerminalFindsNothing() {
        let pane = terminal(frame: CGRect(x: 0, y: 0, width: 100, height: 100))
        let container = root([pane])

        #expect(
            PaneCommandWindow.frontmostTerminalView(
                in: container,
                containing: NSPoint(x: 300, y: 200)
            ) == nil
        )
    }

    @Test func overlappingTerminalsResolveToTheOneDrawnOnTop() {
        let behind = terminal(frame: CGRect(x: 0, y: 0, width: 200, height: 300))
        let inFront = terminal(frame: CGRect(x: 0, y: 0, width: 200, height: 300))
        let container = root([behind, inFront])

        let found = PaneCommandWindow.frontmostTerminalView(
            in: container,
            containing: NSPoint(x: 100, y: 150)
        )

        #expect(found === inFront)
    }

    @Test func aHiddenOrTransparentTerminalIsNotATarget() {
        let hidden = terminal(frame: CGRect(x: 0, y: 0, width: 200, height: 300))
        hidden.isHidden = true
        let transparent = terminal(frame: CGRect(x: 0, y: 0, width: 200, height: 300))
        transparent.alphaValue = 0
        let container = root([hidden, transparent])

        #expect(
            PaneCommandWindow.frontmostTerminalView(
                in: container,
                containing: NSPoint(x: 100, y: 150)
            ) == nil
        )
    }

    @Test func aTerminalNestedInsideAContainerIsStillFound() {
        let pane = terminal(frame: CGRect(x: 0, y: 0, width: 200, height: 200))
        let wrapper = NSView(frame: CGRect(x: 50, y: 50, width: 200, height: 200))
        wrapper.addSubview(pane)
        let container = root([wrapper])

        let found = PaneCommandWindow.frontmostTerminalView(
            in: container,
            containing: NSPoint(x: 100, y: 100)
        )

        #expect(found === pane)
    }
}

@Suite("Pane divider drag")
struct PaneResizeDragPolicyTests {
    @Test func aDragMovesTheSplitByEachStepRatherThanItsWholeTravel() {
        // Two steps of the same drag: the second must report only the travel
        // since the first, or the split would move 30pt after covering 20.
        let first = PaneResizeDragPolicy.step(
            travel: 10, appliedTravel: 0, span: 1_000, isVertical: true
        )
        let second = PaneResizeDragPolicy.step(
            travel: 20, appliedTravel: 10, span: 1_000, isVertical: true
        )

        #expect(first?.amount == 0.01)
        #expect(second?.amount == 0.01)
        #expect(first?.direction == .right)
        #expect(second?.direction == .right)
    }

    @Test func aMoveTooSmallToSeeIsNotSent() {
        #expect(
            PaneResizeDragPolicy.step(
                travel: 2, appliedTravel: 0, span: 1_000, isVertical: true
            ) == nil
        )
    }

    @Test func directionFollowsTheAxisAndTheSignOfTheStep() {
        #expect(
            PaneResizeDragPolicy.step(
                travel: -10, appliedTravel: 0, span: 1_000, isVertical: true
            )?.direction == .left
        )
        #expect(
            PaneResizeDragPolicy.step(
                travel: 10, appliedTravel: 0, span: 1_000, isVertical: false
            )?.direction == .down
        )
        #expect(
            PaneResizeDragPolicy.step(
                travel: -10, appliedTravel: 0, span: 1_000, isVertical: false
            )?.direction == .up
        )
    }

    @Test func aZeroSpanCannotDivideAndStillReportsAStep() {
        // Guards the max(span, 1) floor: a canvas measured before layout must
        // not produce a division by zero.
        #expect(
            PaneResizeDragPolicy.step(
                travel: 10, appliedTravel: 0, span: 0, isVertical: true
            )?.amount == 0.5
        )
    }
}
