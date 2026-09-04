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
        summary: "Agent in \(paneID)",
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
            "strip":[{"id":"herdr:\(id)-tab","kind":"herdr","source_id":"\(id)-tab","label":"Tab 1"}],"next_tab_label":"Tab 2",\
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
        summary: "Build the release",
        elapsed: "2m",
        lastActivity: "0000000000001",
        ambient: nil
    )
    let workspace = try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(
            #"{"id":"workspace-1","label":"hide","path":"/tmp/hide","device_id":"local","repo_name":"hide","is_git":true,"registered":true,"temporary":false,"checkouts":[{"id":"checkout-1","workspace_id":"workspace-1","label":"main","path":"/tmp/hide","branch":"main","is_worktree":false,"exists":true,"temporary":false,"strip":[],"next_tab_label":"Tab 1","tabs":[]}]}"#.utf8
        )
    )
    let checkout = try #require(workspace.checkouts.first)
    let entries = [
        HideSearchEntry(
            id: "agent-agent-1",
            title: agent.summary,
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
