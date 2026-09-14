import AppKit
import Testing
@testable import HerdrMacOS

/// The reported defect was a chord the operator believed was bound and that
/// nothing claimed, so it fell through to the terminal and vanished. These
/// assert what the menu claims, which is the only thing that decides whether a
/// keypress reaches the shell at all.
@Suite("Shell menu shortcuts")
struct ShellMenuCommandTests {
    @MainActor
    @Test func reopenCommandInstallsBeforeTheWindowList() {
        final class Target: NSObject {
            @objc func reopen(_ sender: NSMenuItem) {}
        }

        let menu = NSMenu(title: "Window")
        menu.addItem(withTitle: "Minimize", action: nil, keyEquivalent: "m")
        menu.addItem(.separator())
        menu.addItem(withTitle: "hide", action: nil, keyEquivalent: "")
        let target = Target()

        let item = ReopenWindowMenuPolicy.install(
            in: menu,
            target: target,
            action: #selector(Target.reopen(_:))
        )

        #expect(menu.items.map(\.title) == ["Minimize", "Reopen Closed Tab", "", "hide"])
        #expect(item.identifier == ReopenWindowMenuPolicy.itemIdentifier)
        #expect(item.keyEquivalent == "z")
        #expect(item.keyEquivalentModifierMask == [.command, .shift])
        #expect(item.target === target)

        let sameItem = ReopenWindowMenuPolicy.install(
            in: menu,
            target: target,
            action: #selector(Target.reopen(_:))
        )
        #expect(sameItem === item)
        #expect(menu.items.filter { $0.identifier == ReopenWindowMenuPolicy.itemIdentifier }.count == 1)
    }

    @Test func theRightPanelTogglesOnCommandShiftB() {
        #expect(ShellMenuCommand.toggleRightPanel.shortcut.canonical == "command+shift+b")
        #expect(ShellMenuCommand.toggleRightPanel.displayShortcut == "⇧⌘B")
    }

    @MainActor
    @Test func reopenUsesCommandShiftZButYieldsToTextRedo() {
        #expect(ShellMenuCommand.reopenClosedTab.shortcut.canonical == "command+shift+z")
        #expect(ShellMenuCommand.reopenClosedTab.title == "Reopen Closed Tab")
        let event = NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [.command, .shift],
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: "Z",
            charactersIgnoringModifiers: "z",
            isARepeat: false,
            keyCode: 6
        )!
        #expect(ReopenShortcutPolicy.shouldReopen(event, firstResponder: nil))
        #expect(!ReopenShortcutPolicy.shouldReopen(event, firstResponder: NSTextView()))
        #expect(!ReopenShortcutPolicy.shouldReopen(event, firstResponder: NSSearchField()))
    }

    @Test func theRetiredOptionChordIsClaimedByNothing() {
        let retired = PaneShortcut(key: "b", modifiers: [.command, .option])
        #expect(!ShellMenuCommand.allCases.contains { $0.shortcut == retired })
        #expect(!PaneCommand.allCases.contains { $0.defaultShortcut == retired })
    }

    @Test func recentNavigationAndDirectSelectionFollowThePublicContract() {
        #expect(ShellMenuCommand.recentTab.shortcut.canonical == "control+tab")
        #expect(ShellMenuCommand.previousRecentTab.shortcut.canonical == "control+shift+tab")
        #expect(ShellMenuCommand.recentProject.shortcut.canonical == "option+tab")
        #expect(ShellMenuCommand.previousRecentProject.shortcut.canonical == "option+shift+tab")
        for number in 1...9 {
            #expect(HideCommand.agent(number).shortcut(bindings: [:])?.canonical == "option+\(number)")
            #expect(HideCommand.tab(number).shortcut(bindings: [:])?.canonical == "command+\(number)")
        }
    }

    /// D-04: ⌘⌫ is declared so the collision checks and the pane-rebind
    /// reservation see it, and scoped to the tree so the application menu
    /// never builds an item for it: outside the tree the chord must reach
    /// the terminal untouched.
    @Test func moveToTrashIsDeclaredForTheExplorerTreeAndReservedFromPaneRebinding() {
        #expect(ShellMenuCommand.moveToTrash.shortcut.canonical == "command+delete")
        #expect(ShellMenuCommand.moveToTrash.displayShortcut == "⌘⌫")
        #expect(ShellMenuCommand.moveToTrash.shortcut.menuKeyEquivalent == "\u{8}")
        #expect(ShellMenuCommand.moveToTrash.shortcut.modifierFlags == [.command])
        #expect(ShellMenuCommand.moveToTrash.scope == .explorerTree)
        #expect(ShellMenuCommand.allCases.filter { $0.scope == .explorerTree } == [.moveToTrash])
        // A stored pane binding on the chord is refused: the parser takes
        // one printable key or Return, and the reservation stands behind it.
        let stored = ["close_pane": "command+delete"]
        let resolution = PaneShortcutPolicy.resolve(stored: stored)
        #expect(resolution.bindings == PaneShortcutPolicy.defaults)
        #expect(resolution.diagnostic != nil)
    }

    @Test func noTwoMenuCommandsClaimTheSameChord() {
        let chords = ShellMenuCommand.allCases.map(\.shortcut.canonical)
        #expect(Set(chords).count == chords.count)
    }

    /// The direct-select chords are generated by a loop rather than declared
    /// here, so they are the one set the uniqueness check above cannot see.
    @Test func noMenuCommandCollidesWithADirectSelectionChord() {
        let selectionChords = ShellMenuCommandTests.directSelectionChords
        for command in ShellMenuCommand.allCases {
            #expect(!selectionChords.contains(command.shortcut.canonical))
        }
    }

    /// ⌘n numbers the tabs and ⌥n numbers the agent rows, and the keycaps
    /// drawn on both surfaces claim exactly that. A chord catalogue that
    /// disagreed with the keycap would print a shortcut that does nothing.
    static let directSelectionChords: Set<String> = {
        let tabChords = (1...TabShortcutNumbering.capacity).map {
            PaneShortcut(key: "\($0)", modifiers: [.command]).canonical
        }
        let agentChords = (1...AgentShortcutNumbering.capacity).map {
            PaneShortcut(key: "\($0)", modifiers: [.option]).canonical
        }
        return Set(tabChords + agentChords)
    }()

    @Test func onlyRecentNavigationUsesNonCommandMenuChords() {
        for command in ShellMenuCommand.allCases {
            #expect(command.shortcut.modifiers.contains(.command) || command.shortcut.key == "tab")
        }
    }

    /// Menu chords and pane chords are declared in two separate catalogs and
    /// resolved by two different paths, so nothing but this check stops one
    /// from shadowing the other. `⌘0` next to the `⌘1`-`⌘9` tab chords is
    /// exactly the kind of neighbour that makes the gap worth holding open.
    @Test func noPaneCommandCollidesWithAMenuOrSelectionChord() {
        let menuChords = Set(ShellMenuCommand.allCases.map(\.shortcut.canonical))
        let selectionChords = ShellMenuCommandTests.directSelectionChords
        let paneChords = PaneCommand.allCases.map(\.defaultShortcut.canonical)
        #expect(Set(paneChords).count == paneChords.count)
        for chord in paneChords {
            #expect(!menuChords.contains(chord))
            #expect(!selectionChords.contains(chord))
        }
    }

    /// The three text-size commands are text size, not the layout zoom that
    /// `toggleZoom` already means. Naming them apart is the whole reason the
    /// two can coexist as pane commands.
    @Test func onlyTheTextSizeCommandsCarryAScaleDirection() {
        #expect(PaneCommand.increaseTextSize.textScaleDirection == .in)
        #expect(PaneCommand.decreaseTextSize.textScaleDirection == .out)
        #expect(PaneCommand.resetTextSize.textScaleDirection == .reset)
        #expect(PaneCommand.toggleZoom.textScaleDirection == nil)
        #expect(PaneCommand.closePane.textScaleDirection == nil)
        #expect(PaneCommand.splitRight.textScaleDirection == nil)
        #expect(PaneCommand.splitDown.textScaleDirection == nil)
    }
}
