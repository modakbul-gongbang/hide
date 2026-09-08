import Combine
import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Recent navigation through the core", .serialized)
struct RecentNavigationIntegrationTests {
    @MainActor @Test func projectSwitchRestoresItsLastFileAndControlCycleIncludesTheTerminal() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("hide-recent-\(UUID().uuidString)")
        let alpha = root.appendingPathComponent("alpha")
        let beta = root.appendingPathComponent("beta")
        try FileManager.default.createDirectory(at: alpha, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: beta, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
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
        model.beginOrAdvanceTabSwitcher()
        let target = try #require(model.tabSwitcherCycle?.selectedTabID)
        let surface = try #require(model.recentSurfaces[target])
        guard case .herdr = surface.item.kind else {
            Issue.record("Control cycle must include the existing terminal alongside the file")
            return
        }
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
        model.beginOrAdvanceProjectSwitcher()
        model.commitProjectSwitcher()
        try await eventually("restore beta") { bridge.snapshot?.editor.activeTabID == fileBID }

        // Repeated cancellation has no state transition and publishes nothing.
        var publications = 0
        let projectSubscription = model.recentNavigation.$projectCycle.dropFirst().sink { _ in publications += 1 }
        let tabSubscription = model.recentNavigation.$tabCycle.dropFirst().sink { _ in publications += 1 }
        for _ in 0..<1000 { model.cancelProjectSwitcher(); model.cancelTabSwitcher() }
        #expect(publications == 0)
        withExtendedLifetime((projectSubscription, tabSubscription)) {}
    }

    @MainActor private func eventually(_ label: String, _ condition: () -> Bool) async throws {
        let clock = ContinuousClock()
        let deadline = clock.now + .seconds(10)
        while !condition(), clock.now < deadline { try await Task.sleep(for: .milliseconds(20)) }
        try #require(condition(), Comment(rawValue: label))
    }
}
