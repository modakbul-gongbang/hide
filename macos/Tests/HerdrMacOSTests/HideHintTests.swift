import AppKit
import Testing
@testable import HerdrMacOS

@Suite("Hide hint lifecycle and exposure")
struct HideHintTests {
    func targets() -> [HideHintTarget] {
        let menus: [ShellMenuCommand] = [.search, .newChat, .newWorkspace, .toggleSidebarView,
            .toggleLeftSidebar, .newTab, .toggleRightPanel, .openFile, .findInPane]
        return menus.map { HideHintTarget(id: $0.rawValue, command: .menu($0)) } + [
            HideHintTarget(id: "close-a", command: .menu(.closeTab), tabID: "a"),
            HideHintTarget(id: "close-b", command: .menu(.closeTab), tabID: "b"),
            HideHintTarget(id: "pane-a", command: .pane(.closePane), paneID: "a"),
            HideHintTarget(id: "pane-b", command: .pane(.closePane), paneID: "b"),
            HideHintTarget(id: "zoom-a", command: .pane(.toggleZoom), paneID: "a"),
            HideHintTarget(id: "zoom-b", command: .pane(.toggleZoom), paneID: "b"),
            HideHintTarget(id: "tab-1", command: .tab(1)),
            HideHintTarget(id: "agent-1", command: .agent(1)),
            HideHintTarget(id: "browser", command: .label("Refresh")),
        ]
    }

    @Test func exactMatrixAndEffectiveBindings() {
        let cases: [(Set<PaneShortcut.Modifier>, Set<String>)] = [
            ([.command], ["search", "new_chat", "toggle_sidebar_view", "toggle_left_sidebar", "new_tab", "close-a", "tab-1"]),
            ([.command, .shift], ["new_workspace", "toggle_right_panel", "pane-a"]),
            ([.option], ["agent-1"]), ([.command, .option], ["zoom-a"]),
            ([.control], []), ([.shift], []), ([.control, .command], []),
        ]
        for (modifiers, expected) in cases {
            var state = HideHintState()
            state.update(modifiers, at: 0)
            state.advance(to: 1)
            let result = HideHintTarget.exposed(among: targets(), state: state, bindings: [:], focusedPaneID: "a", activeTabID: "a")
            #expect(Set(result.map(\.id)) == expected)
        }
        var state = HideHintState()
        state.update([.control], at: 0)
        state.advance(to: 1)
        let binding = PaneShortcut(key: "x", modifiers: [.control])
        let result = HideHintTarget.exposed(among: targets(), state: state, bindings: [.closePane: binding], focusedPaneID: "b", activeTabID: "a")
        #expect(Set(result.map(\.id)) == ["pane-b"])
        #expect(HideCommand.pane(.closePane).displayString(bindings: [.closePane: binding]) == binding.displayString)
    }

    @Test func deadlineReleaseReplacementAndSnapshotRetargeting() {
        var state = HideHintState()
        state.update([.command], at: 0)
        state.advance(to: 0.149)
        #expect(!state.revealed)
        state.update([], at: 0.149)
        state.advance(to: 1)
        #expect(!state.revealed)
        state.update([.command], at: 2)
        state.advance(to: 2.150)
        #expect(state.revealed)
        state.update([.command, .shift], at: 3)
        #expect(state.revealed)
        let before = HideHintTarget.exposed(among: targets(), state: state, bindings: [:], focusedPaneID: "a", activeTabID: "a")
        let after = HideHintTarget.exposed(among: targets(), state: state, bindings: [:], focusedPaneID: "b", activeTabID: "b")
        #expect(before.contains { $0.id == "pane-a" })
        #expect(!after.contains { $0.id == "pane-a" })
        #expect(after.contains { $0.id == "pane-b" })
        let removed = HideHintTarget.exposed(among: targets().filter { $0.id != "pane-b" }, state: state, bindings: [:], focusedPaneID: "b", activeTabID: "b")
        #expect(!removed.contains { $0.id == "pane-b" })
        state.update([.command], at: 4)
        let activeTab = HideHintTarget.exposed(among: targets(), state: state, bindings: [:], focusedPaneID: "b", activeTabID: "b")
        #expect(activeTab.contains { $0.id == "close-b" })
        #expect(!activeTab.contains { $0.id == "close-a" })
        state.update([.command, .option], at: 5)
        let unzoomed = HideHintTarget.exposed(among: targets().filter { !$0.id.hasPrefix("zoom") }, state: state, bindings: [:], focusedPaneID: "b", activeTabID: "b")
        #expect(unzoomed.isEmpty)
        state.clear() // resign-active or sheet presentation
        #expect(state.modifiers.isEmpty && !state.revealed && state.deadline == nil)
        state.update([.control], at: 6) // re-read global flags after dismissal
        state.advance(to: 6.150)
        #expect(state.revealed && state.modifiers == [.control])
    }

    @MainActor @Test func fileSearchSuppressesAndThenRestoresShortcutHints() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-hint-file-search-\(UUID().uuidString)", isDirectory: true)
        let stateURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-hint-file-search-state-\(UUID().uuidString).json")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: stateURL)
        }

        let bridge = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--workspace-root", root.path,
            "--state-path", stateURL.path,
        ])
        let model = ShellModel(core: bridge)

        model.setShortcutModifiersHeld([.command])
        #expect(try await revealed(model))

        model.showFileSearch = true
        #expect(model.hintSheetPresented)
        #expect(model.shortcutHintState.modifiers.isEmpty)
        #expect(!model.shortcutHintState.revealed)
        // A hold while a sheet is up schedules nothing: no deadline means no
        // reveal can arrive later, so this needs no waiting to be proven.
        model.setShortcutModifiersHeld([.command])
        #expect(model.shortcutHintState.deadline == nil)
        #expect(model.shortcutHintState.modifiers.isEmpty)
        #expect(!model.shortcutHintState.revealed)

        model.showFileSearch = false
        #expect(!model.hintSheetPresented)
        model.setShortcutModifiersHeld([.command])
        #expect(try await revealed(model))
    }

    /// The reveal rides a 150 ms task on the main actor, and the actor is
    /// shared with every other main-actor test in the process, so a fixed
    /// sleep passes or fails with the machine's load. Wait for the state
    /// itself, bounded well above anything the scheduler can add.
    @MainActor private func revealed(_ model: ShellModel, within limit: Duration = .seconds(10)) async throws -> Bool {
        let clock = ContinuousClock()
        let deadline = clock.now + limit
        while clock.now < deadline {
            if model.shortcutHintState.revealed { return true }
            try await Task.sleep(for: .milliseconds(20))
        }
        return model.shortcutHintState.revealed
    }

    @MainActor @Test func observerPreservesEveryKeyDownAndOnlyObservesFlags() throws {
        for modifiers: NSEvent.ModifierFlags in [[], .control, .command, [.command, .shift], .option] {
            let event = try #require(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
                timestamp: 0, windowNumber: 0, context: nil, characters: "w", charactersIgnoringModifiers: "w", isARepeat: false, keyCode: 13))
            var updates = 0
            #expect(HideHintEventObserver.observe(event) { _ in updates += 1 } === event)
            #expect(updates == 0)
        }
        #expect(HideHintState.modifiers(in: [.command, .shift, .capsLock]) == [.command, .shift])
    }

    @Test func searchKeycapEqualsRegistryAndRebindingRefreshesPresentation() {
        #expect(HideCommand.menu(.search).displayString(bindings: [:]) == ShellMenuCommand.search.displayShortcut)
        let rebound = PaneShortcut(key: "x", modifiers: [.control, .option])
        let bindings: [PaneCommand: PaneShortcut] = [.closePane: rebound]
        #expect(HideCommand.pane(.closePane).displayString(bindings: bindings) == rebound.displayString)
        #expect(HideCommand.pane(.closePane).tooltipText(label: "Close pane", bindings: bindings) == "Close pane (\(rebound.displayString))")
    }
}
