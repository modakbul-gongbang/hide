import Foundation
import Testing

@testable import HerdrMacOS

private func searchAgent(paneID: String) -> SidebarAgent {
    SidebarAgent(
        id: "agent-\(paneID)",
        paneID: paneID,
        workspaceLabel: "Same name",
        agentKind: "codex",
        demand: "none",
        activity: "working",
        group: "working",
        symbol: "\u{25cf}",
        identityLabel: "Agent in \(paneID)",
        elapsed: "1m",
        lastActivity: "0000000000001",
        ambient: nil
    )
}

private func searchWorkspace(id: String, paneID: String) throws -> CoreWorkspaceSnapshot {
    try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(
            """
            {"id":"\(id)","label":"Same name","path":"/tmp/\(id)","device_id":"local",\
            "repo_name":"same","is_git":true,"registered":true,"temporary":false,\
            "checkouts":[{"id":"\(id)-checkout","workspace_id":"\(id)","label":"main",\
            "path":"/tmp/\(id)","branch":"main","is_worktree":false,"exists":true,\
            "temporary":false,\
            "strip":[{"id":"herdr:\(id)-tab","kind":"herdr","source_id":"\(id)-tab","label":"Tab 1"}],"agent_summary":{"representative_pane_id":null,"needs_you":0,"done":0,"working":0,"seen":0,"unknown":0},"next_tab_label":"Tab 2",\
            "tabs":[{"id":"\(id)-tab","workspace_id":"\(id)",\
            "checkout_id":"\(id)-checkout","label":"1","empty":false,\
            "panes":[{"id":"\(paneID)","label":"\(paneID)","cwd":"/tmp/\(id)","status_label":"Working","requires_close_confirmation":true}]}]}]}
            """.utf8
        )
    )
}

@Test func hideSearchFiltersAgentsAndCheckoutsWithoutTerminalEntries() throws {
    let agent = SidebarAgent(
        id: "agent-1",
        paneID: "pane-1",
        workspaceLabel: "hide",
        agentKind: "codex",
        demand: "none",
        activity: "working",
        group: "working",
        symbol: "\u{25cf}",
        identityLabel: "Build the release",
        elapsed: "2m",
        lastActivity: "0000000000001",
        ambient: nil
    )
    let workspace = try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(
            #"{"id":"workspace-1","label":"hide","path":"/tmp/hide","device_id":"local","repo_name":"hide","is_git":true,"registered":true,"temporary":false,"checkouts":[{"id":"checkout-1","workspace_id":"workspace-1","label":"main","path":"/tmp/hide","branch":"main","is_worktree":false,"exists":true,"temporary":false,"strip":[],"agent_summary":{"representative_pane_id":null,"needs_you":0,"done":0,"working":0,"seen":0,"unknown":0},"next_tab_label":"Tab 1","tabs":[]}]}"#.utf8
        )
    )
    let checkout = try #require(workspace.checkouts.first)
    let entries = [
        HideSearchEntry(
            id: "agent-agent-1",
            title: agent.identityLabel,
            subtitle: "Agent · \(agent.workspaceLabel)",
            kind: .agent(agent)
        ),
        HideSearchEntry(
            id: "checkout-checkout-1",
            title: "\(workspace.repoName) / \(checkout.label)",
            subtitle: checkout.path,
            kind: .checkout(workspace, checkout)
        ),
    ]

    #expect(HideSearchEntry.filtered(entries, query: "repo").isEmpty)
    #expect(HideSearchEntry.filtered(entries, query: "release").map(\.id) == ["agent-agent-1"])
    #expect(HideSearchEntry.filtered(entries, query: " / main").map(\.id) == ["checkout-checkout-1"])
    #expect(HideSearchEntry.filtered(entries, query: "  ").map(\.id) == ["agent-agent-1", "checkout-checkout-1"])
    #expect(entries[0].route == .agent(paneID: "pane-1"))
    #expect(entries[1].route == .checkout(workspaceID: "workspace-1", checkoutID: "checkout-1"))
}

@Test func sameLabelWorkspacesStaySeparateAndRouteAgentsByPaneID() throws {
    let groups = HideSearchPresentation.agentGroups(
        workspaces: [
            try searchWorkspace(id: "workspace-1", paneID: "pane-1"),
            try searchWorkspace(id: "workspace-2", paneID: "pane-2"),
        ],
        agents: [searchAgent(paneID: "pane-1"), searchAgent(paneID: "pane-2")],
        query: ""
    )

    #expect(groups.map(\.id) == ["workspace-1", "workspace-2"])
    #expect(groups.map(\.workspace) == ["Same name", "Same name"])
    #expect(groups.map { $0.entries.map(\.route) } == [
        [.agent(paneID: "pane-1")],
        [.agent(paneID: "pane-2")],
    ])
}

/// PRD D-15, B17: an agent result is titled by its identity, subtitled by the
/// core's sentence (the status word when there is none), and still found by
/// its pane id, which the row no longer prints.
@Test func agentSearchRowsShowIdentityAndSentenceAndMatchThePaneID() throws {
    let workspace = try searchWorkspace(id: "workspace-1", paneID: "w7J:p2P")
    let question = SidebarAgent(
        id: "q", paneID: "w7J:p2P", workspaceLabel: "Same name", agentKind: "claude",
        demand: "question", unread: true, group: "needs_you", symbol: "?", emphasized: true,
        statusLabel: "Question",
        identityLabel: "결제 멱등키 PR", detail: "A/B 선택 후 DB 마이그레이션 승인", statusWordVisible: true,
        elapsed: "2m", lastActivity: "", ambient: nil
    )
    let entries = HideSearchPresentation.agentGroups(
        workspaces: [workspace], agents: [question], query: ""
    ).flatMap(\.entries)
    #expect(entries.map(\.title) == ["결제 멱등키 PR"])
    #expect(entries.map(\.subtitle) == ["A/B 선택 후 DB 마이그레이션 승인"])
    #expect(entries.map(\.match) == ["w7J:p2P"])
    #expect(HideSearchPresentation.agentGroups(
        workspaces: [workspace], agents: [question], query: "p2p"
    ).flatMap(\.entries).map(\.id) == ["agent-w7J:p2P"])
    #expect(HideSearchPresentation.agentGroups(
        workspaces: [workspace], agents: [question], query: "마이그레이션"
    ).flatMap(\.entries).count == 1)

    let quiet = searchAgent(paneID: "w7J:p2P")
    #expect(HideSearchPresentation.agentSubtitle(quiet) == "Idle")
    let tasked = SidebarAgent(
        id: "t", paneID: "w7J:p3", workspaceLabel: "Same name", agentKind: "claude",
        group: "seen", symbol: "\u{25cb}", statusLabel: "Idle",
        identityLabel: "hook-bug-check", task: "hook 보고 경로 교체",
        elapsed: "2m", lastActivity: "", ambient: nil
    )
    #expect(HideSearchPresentation.agentSubtitle(tasked) == "hook 보고 경로 교체")
    let named = SidebarAgent(
        id: "n", paneID: "w7J:p4", workspaceLabel: "Same name", agentKind: "claude",
        group: "seen", symbol: "\u{25cb}", statusLabel: "Idle",
        identityLabel: "hook 보고 경로 교체", task: "hook 보고 경로 교체",
        elapsed: "2m", lastActivity: "", ambient: nil
    )
    #expect(HideSearchPresentation.agentSubtitle(named) == "Idle", "the task is already the title")
}

@Test func foldedProjectSearchRoutesToPrimaryWithoutExpandingItsGroup() throws {
    let workspace = try searchWorkspace(id: "workspace-1", paneID: "pane-1")
    let collapsed = CoreInactiveProjectGroupSnapshot(
        deviceID: "local",
        expanded: false,
        projectIDs: [workspace.id]
    )
    let expanded = CoreInactiveProjectGroupSnapshot(
        deviceID: "local",
        expanded: true,
        projectIDs: [workspace.id]
    )

    let entries = HideSearchPresentation.foldedProjectEntries(
        workspaces: [workspace],
        groups: [collapsed],
        query: "same"
    )

    #expect(entries.map(\.id) == ["project-workspace-1"])
    #expect(entries.map(\.route) == [
        .project(
            workspaceID: "workspace-1",
            primaryCheckoutID: "workspace-1-checkout"
        ),
    ])
    #expect(HideSearchPresentation.foldedProjectEntries(
        workspaces: [workspace],
        groups: [expanded],
        query: ""
    ).isEmpty)
}

@Test func searchKeyboardSelectionRoutesHighlightedResultAndReconcilesRetirement() throws {
    let rows = ["a", "b", "c"].map { id in
        HideSearchEntry(id: id, title: id, subtitle: id, kind: .agent(searchAgent(paneID: id)))
    }
    var selection = HideSearchSelection()
    selection.reconcile(rows.map(\.id))
    #expect(selection.entry(in: rows)?.route == .agent(paneID: "a"))
    selection.move(.down, among: rows.map(\.id))
    #expect(selection.entry(in: rows)?.route == .agent(paneID: "b"))
    selection.move(.up, among: rows.map(\.id))
    #expect(selection.selectedID == "a")
    selection.move(.up, among: rows.map(\.id))
    #expect(selection.selectedID == "a")
    for _ in 0..<1000 { selection.move(.down, among: rows.map(\.id)) }
    #expect(selection.selectedID == "c")
    // Return cannot dispatch a row that disappeared before reconciliation.
    #expect(selection.entry(in: Array(rows.prefix(2))) == nil)
    selection.reconcile(["a", "b"])
    #expect(selection.selectedID == "a")
    selection.move(.down, among: ["a", "b"])
    selection.reconcile(["b", "a"])
    #expect(selection.selectedID == "b")

    let filtered = HideSearchEntry.filtered(rows, query: "no matching result")
    selection.reconcile(filtered.map(\.id))
    for _ in 0..<1000 {
        selection.move(.down, among: [])
        selection.move(.up, among: [])
    }
    #expect(selection.selectedID == nil)
    #expect(selection.entry(in: filtered) == nil)
    selection.reconcile(["a"])
    #expect(selection.selectedID == "a")
}
