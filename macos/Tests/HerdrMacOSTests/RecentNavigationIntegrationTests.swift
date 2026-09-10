import Combine
import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Recent navigation through the core", .serialized)
struct RecentNavigationIntegrationTests {
    @MainActor @Test func emptyAndSingleItemNavigationDoesNotInterruptTheUser() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent("hide-empty-navigation-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        let model = ShellModel(core: bridge)
        try await eventually("initial checkout") { model.focusedCheckout != nil }
        try await eventually("single terminal") { model.recentSurfaces.count == 1 }
        let paneID = model.focusedPaneID
        // A no-op must also preserve an existing, actionable failure notice.
        model.interactionNotice = "The selected project's device is unavailable. Selection was kept."
        let existingNotice = model.interactionNotice
        for _ in 0..<1000 {
            model.beginOrAdvanceProjectSwitcher(); model.beginOrRetreatProjectSwitcher()
            model.beginOrAdvanceTabSwitcher(); model.beginOrRetreatTabSwitcher()
        }
        #expect(model.interactionNotice == existingNotice)
        #expect(model.focusedPaneID == paneID)
        #expect(model.projectSwitcherCycle == nil)
        #expect(model.tabSwitcherCycle == nil)
        model.interactionNotice = nil
        bridge.dispatch(kind: "session_snapshot", payload: [
            "agents": [], "workspaces": [], "tabs": [], "panes": [], "layouts": [],
        ])
        try await eventually("empty strip") { model.recentSurfaces.isEmpty }
        let projectID = model.focusedWorkspace?.id
        var publications = 0
        let subscription = model.objectWillChange.sink { publications += 1 }
        for _ in 0..<1000 {
            model.beginOrAdvanceProjectSwitcher(); model.beginOrRetreatProjectSwitcher()
            model.beginOrAdvanceTabSwitcher(); model.beginOrRetreatTabSwitcher()
            model.performCloseShortcut()
        }
        #expect(model.interactionNotice == nil)
        #expect(model.projectSwitcherCycle == nil)
        #expect(model.tabSwitcherCycle == nil)
        #expect(model.focusedWorkspace?.id == projectID)
        #expect(publications == 0)
        withExtendedLifetime(subscription) {}


    }

    @MainActor @Test func recentPanelKeepsTheFocusedAgentBrandAcrossSplitFocusAndFileVisits() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent("hide-panel-marks-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        let model = ShellModel(core: bridge)
        try await eventually("initial terminal") { model.recentSurfaces.count == 1 }
        bridge.dispatch(kind: "session_snapshot", payload: [
            "focused_pane_id": "fixture-working",
            "agents": [("fixture-working", "codex"), ("fixture-claude", "claude")].map { pane, kind in
                ["id": pane, "pane_id": pane, "workspace_label": "Review", "agent": kind,
                 "agent_status": "working", "tokens": ["summary": "Review", "status_working": "●", "activity": "1755000003000"]] as [String: Any]
            },
            "workspaces": [["workspace_id": "fixture-workspace", "label": "Review", "active_tab_id": "fixture-tab"]],
            "tabs": [["tab_id": "fixture-tab", "workspace_id": "fixture-workspace", "label": "Review"]],
            "panes": ["fixture-working", "fixture-claude"].map { ["pane_id": $0, "cwd": root.path] },
            "layouts": [[
                "workspace_id": "fixture-workspace", "tab_id": "fixture-tab", "zoomed": false,
                "focused_pane_id": "fixture-working", "area": ["x": 0, "y": 0, "width": 80, "height": 24],
                "panes": [
                    ["pane_id": "fixture-working", "rect": ["x": 0, "y": 0, "width": 40, "height": 24]],
                    ["pane_id": "fixture-claude", "rect": ["x": 40, "y": 0, "width": 40, "height": 24]],
                ], "splits": [["direction": "right", "ratio": 0.5,
                    "rect": ["x": 0, "y": 0, "width": 80, "height": 24]]],
            ]],
        ])
        try await eventually("Codex panel mark") {
            model.recentSurfaces.values.first?.item.focusedAgent?.agentKind == "codex"
        }
        let tabSurfaceID = try #require(model.recentSurfaces.values.first?.id)
        bridge.focusPane("fixture-claude", origin: .operatorChoice)
        try await eventually("Claude panel mark follows core focus before layout acknowledgement") {
            model.recentSurfaces[tabSurfaceID]?.item.focusedAgent?.agentKind == "claude"
        }
        #expect(model.recentSurfaces[tabSurfaceID]?.item.focusedAgent?.paneID == "fixture-claude")
        let file = root.appendingPathComponent("검토.md")
        try "review".write(to: file, atomically: true, encoding: .utf8)
        model.openFile(file)
        try await eventually("file appears alongside agent") { model.recentSurfaces.count == 2 }
        let fileSurface = try #require(model.recentSurfaces.values.first { $0.id != tabSurfaceID })
        #expect(fileSurface.item.focusedAgent == nil)
        #expect(fileSurface.symbol == "doc.text")
        model.beginOrAdvanceTabSwitcher()
        #expect(model.tabSwitcherCycle?.selectedTabID == tabSurfaceID)
        #expect(model.recentSurfaces[tabSurfaceID]?.item.focusedAgent?.agentKind == "claude")
        model.cancelTabSwitcher()
    }

    @MainActor @Test func controlTabRestoresTheActuallyPreviousPanelAcrossFilesAndTerminal() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent("hide-recent-panels-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let first = root.appendingPathComponent("first.txt")
        let last = root.appendingPathComponent("직전-file.txt")
        for file in [first, last] { try "review\n".write(to: file, atomically: true, encoding: .utf8) }
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        let model = ShellModel(core: bridge)
        try await eventually("initial terminal") { model.recentSurfaces.count == 1 }
        let workspaceID = try #require(model.focusedWorkspace?.id)
        let checkoutID = try #require(model.focusedCheckout?.id)
        let terminalID = try #require(model.focusedCheckout?.activeTabID)
        model.openFile(first)
        try await eventually("first file") { bridge.snapshot?.editor.tabs.contains { $0.path == first.path } == true }
        let firstID = try #require(bridge.snapshot?.editor.activeTabID)
        model.openFile(last)
        try await eventually("last file") { bridge.snapshot?.editor.tabs.contains { $0.path == last.path } == true }
        let lastID = try #require(bridge.snapshot?.editor.activeTabID)
        bridge.focusTab(workspaceID: workspaceID, checkoutID: checkoutID, tabID: terminalID)
        try await eventually("return to terminal") { bridge.snapshot?.editor.activeTabID == nil }

        // Tab-strip order is terminal, first, last. Recent order must be
        // terminal, last, first, so one chord returns to the actual last file.
        model.beginOrAdvanceTabSwitcher()
        let selectedID = try #require(model.tabSwitcherCycle?.selectedTabID)
        let selected = try #require(model.recentSurfaces[selectedID])
        guard case .editor(let file) = selected.item.kind else {
            Issue.record("The previous panel must be the file, not another agent")
            return
        }
        #expect(file.id == lastID)
        model.commitTabSwitcher()
        try await eventually("restore last file") { bridge.snapshot?.editor.activeTabID == lastID }
        model.beginOrAdvanceTabSwitcher()
        model.commitTabSwitcher()
        try await eventually("toggle back to terminal") { bridge.snapshot?.editor.activeTabID == nil }
        model.beginOrAdvanceTabSwitcher()
        model.beginOrAdvanceTabSwitcher()
        model.commitTabSwitcher()
        try await eventually("second recent file") { bridge.snapshot?.editor.activeTabID == firstID }
        #expect(model.focusedWorkspace?.id == workspaceID)
        #expect(model.interactionNotice == nil)
    }

    @MainActor @Test func projectSwitchRestoresItsLastFileAndControlCycleSpansProjects() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("hide-recent-\(UUID().uuidString)")
        let alpha = root.appendingPathComponent("alpha")
        let beta = root.appendingPathComponent("beta")
        try FileManager.default.createDirectory(at: alpha, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: beta, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        // Each fixture is its own repository, so the two are separate projects
        // wherever TMPDIR happens to point. A bare directory is only its own
        // project while nothing above it is one, and a harness that puts TMPDIR
        // inside a checkout - which is where run state lives here - would have
        // both of these resolve to that enclosing checkout and never register
        // beta at all.
        try initRepository(at: alpha)
        try initRepository(at: beta)
        let fileA = alpha.appendingPathComponent("한글-alpha.txt")
        let fileB = beta.appendingPathComponent("beta.txt")
        try "alpha\n".write(to: fileA, atomically: true, encoding: .utf8)
        try "beta\n".write(to: fileB, atomically: true, encoding: .utf8)
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", alpha.path, "--state-path", root.appendingPathComponent("state.json").path])
        let model = ShellModel(core: bridge)
        try await eventually("initial checkout") { model.focusedCheckout != nil }
        let alphaID = try #require(model.focusedWorkspace?.id)
        model.openFile(fileA)
        try await eventually("open alpha file") { bridge.snapshot?.editor.tabs.contains { $0.path == fileA.path } == true }
        let fileAID = try #require(bridge.snapshot?.editor.activeTabID)
        // Project topology enters through the same core event as Herdr's
        // replica. No live server or operator workspace is used by this test.
        let fixtureRows = [("fixture-workspace", "fixture-tab", "fixture-working", alpha),
                           ("fixture-beta", "fixture-beta-tab", "fixture-beta-pane", beta)]
        bridge.dispatch(kind: "session_snapshot", payload: [
            "focused_pane_id": "fixture-working", "agents": [],
            "workspaces": fixtureRows.map { ["workspace_id": $0.0, "label": $0.0, "active_tab_id": $0.1] },
            "tabs": fixtureRows.map { ["tab_id": $0.1, "workspace_id": $0.0, "label": $0.1] },
            "panes": fixtureRows.map { ["pane_id": $0.2, "cwd": $0.3.path] },
            "layouts": fixtureRows.map { row -> [String: Any] in [
                "workspace_id": row.0, "tab_id": row.1, "zoomed": false,
                "focused_pane_id": row.2, "area": ["x": 0, "y": 0, "width": 80, "height": 24],
                "panes": [["pane_id": row.2, "rect": ["x": 0, "y": 0, "width": 80, "height": 24]]],
                "splits": [],
            ] },
        ])
        try await eventually("register beta") { model.workspaces.contains { URL(fileURLWithPath: $0.path).resolvingSymlinksInPath() == beta.resolvingSymlinksInPath() } }
        let betaProject = try #require(model.workspaces.first { URL(fileURLWithPath: $0.path).resolvingSymlinksInPath() == beta.resolvingSymlinksInPath() })
        let betaCheckout = try #require(betaProject.checkouts.first)
        bridge.focusCheckout(workspaceID: betaProject.id, checkoutID: betaCheckout.id)
        try await eventually("focus beta") { model.focusedCheckout?.id == betaCheckout.id }
        model.openFile(fileB)
        try await eventually("open beta file") { bridge.snapshot?.editor.tabs.contains { $0.path == fileB.path } == true }
        let fileBID = try #require(bridge.snapshot?.editor.activeTabID)
        model.beginOrAdvanceProjectSwitcher()
        #expect(model.projectSwitcherCycle?.selectedProjectID == "local:\(alphaID)")
        model.commitProjectSwitcher()
        try await eventually("restore alpha") { bridge.snapshot?.editor.activeTabID == fileAID }
        #expect(model.focusedWorkspace?.id == alphaID)
        // The chord spans projects: the actually previous panel is beta's file,
        // even though the focused project is now alpha.
        model.beginOrAdvanceTabSwitcher()
        let target = try #require(model.tabSwitcherCycle?.selectedTabID)
        let surface = try #require(model.recentSurfaces[target])
        guard case .editor(let previousFile) = surface.item.kind, previousFile.id == fileBID else {
            Issue.record("The previous panel must be the other project's file")
            return
        }
        #expect(surface.projectID != "local:\(alphaID)")
        let betaFileSurfaceID = target
        // Terminals stay in the same cycle alongside the files.
        #expect(model.tabSwitcherCycle?.tabIDs.compactMap { model.recentSurfaces[$0] }.contains {
            if case .herdr = $0.item.kind { return true } else { return false }
        } == true)
        var stepPublications = 0
        let stepSubscription = model.recentNavigation.$tabCycle.dropFirst().sink { _ in stepPublications += 1 }
        var shellPublications = 0
        let shellSubscription = model.objectWillChange.sink { shellPublications += 1 }
        let revisionBeforeHold = bridge.snapshot?.navigationRevision
        for _ in 0..<1000 { model.beginOrAdvanceTabSwitcher() }
        #expect(stepPublications == 1000)
        #expect(shellPublications == 0)
        #expect(bridge.snapshot?.navigationRevision == revisionBeforeHold)
        withExtendedLifetime((stepSubscription, shellSubscription)) {}
        model.cancelTabSwitcher()
        #expect(bridge.snapshot?.editor.activeTabID == fileAID)
        // Committing that cross-project selection moves the focused project too.
        model.beginOrAdvanceTabSwitcher()
        model.commitTabSwitcher()
        try await eventually("restore beta") { bridge.snapshot?.editor.activeTabID == fileBID }
        #expect(model.focusedWorkspace?.id == betaProject.id)

        // A file can close while its MRU row is highlighted. The held cycle
        // converges to the remaining terminal without asking for acknowledgement.
        model.beginOrAdvanceTabSwitcher()
        model.beginOrAdvanceTabSwitcher()
        #expect(model.tabSwitcherCycle != nil)
        bridge.closeFileTab(fileBID)
        try await eventually("closed file pruned") {
            model.tabSwitcherCycle?.tabIDs.contains(betaFileSurfaceID) == false
                && bridge.snapshot?.editor.tabs.contains { $0.id == fileBID } == false
        }
        #expect(model.interactionNotice == nil)
        model.commitTabSwitcher()
        #expect(model.tabSwitcherCycle == nil)
        #expect(model.interactionNotice == nil)

        // A held project's target may also retire before the queued commit.
        // Deliver the stale cycle exactly as a delayed UI callback would.
        let retiredCycle = try #require(ProjectSwitcherCycle(
            originalProjectID: "local:\(betaProject.id)",
            projectIDs: ["local:\(betaProject.id)", "retired-project"]))
        model.recentNavigation.projectCycle = retiredCycle
        model.commitProjectSwitcher()
        #expect(model.projectSwitcherCycle == nil)
        #expect(model.focusedPaneID == "fixture-beta-pane")
        #expect(model.interactionNotice == nil)

        // Repeated cancellation has no state transition and publishes nothing.
        var publications = 0
        let projectSubscription = model.recentNavigation.$projectCycle.dropFirst().sink { _ in publications += 1 }
        let tabSubscription = model.recentNavigation.$tabCycle.dropFirst().sink { _ in publications += 1 }
        for _ in 0..<1000 { model.cancelProjectSwitcher(); model.cancelTabSwitcher() }
        #expect(publications == 0)
        withExtendedLifetime((projectSubscription, tabSubscription)) {}
    }

    private func initRepository(at root: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["-C", root.path, "init", "--quiet"]
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
    }

    @MainActor private func eventually(_ label: String, _ condition: () -> Bool) async throws {
        let clock = ContinuousClock()
        let deadline = clock.now + .seconds(10)
        while !condition(), clock.now < deadline { try await Task.sleep(for: .milliseconds(20)) }
        try #require(condition(), Comment(rawValue: label))
    }
}
